use crate::available_languages::AvailableLanguage;
use crate::{
    File, Language, LanguageConfig, LanguageId, LanguageMatcher, ManifestName, PLAIN_TEXT,
    ToolchainLister, available_languages::AvailableLanguages,
    language_settings::all_language_settings, task_context::ContextProvider, with_parser,
};
use anyhow::{Context as _, Result, anyhow};
use collections::{FxHashMap, HashMap, hash_map};
pub use language_core::{LanguageName, LanguageQueries, QueryFile, QueryFileContents, QueryFiles};
use settings::{AllLanguageSettingsContent, LanguageSettingsContent};

use futures::{
    Future,
    channel::oneshot,
    future::{BoxFuture, FutureExt as _},
};
use globset::GlobSet;
use gpui::{App, BackgroundExecutor};
use lsp::LanguageServerId;
use parking_lot::RwLock;
use postage::watch;

use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
    sync::Arc,
};
use text::Rope;
use theme::Theme;

use util::{maybe, post_inc};

pub struct LanguageRegistry {
    state: RwLock<LanguageRegistryState>,
    executor: BackgroundExecutor,
}

struct LanguageRegistryState {
    next_language_server_id: usize,
    languages: Vec<Arc<Language>>,
    language_settings: AllLanguageSettingsContent,
    available_languages: AvailableLanguages,
    grammars: HashMap<Arc<str>, AvailableGrammar>,
    loading_languages: HashMap<LanguageId, Vec<oneshot::Sender<Result<Arc<Language>>>>>,
    subscription: (watch::Sender<()>, watch::Receiver<()>),
    theme: Option<Arc<Theme>>,
    version: usize,
    reload_count: usize,
}

enum AvailableGrammar {
    Native(tree_sitter::Language),
    Loaded(#[allow(unused)] PathBuf, tree_sitter::Language),
    Loading(
        #[allow(unused)] PathBuf,
        Vec<oneshot::Sender<Result<tree_sitter::Language, Arc<anyhow::Error>>>>,
    ),
    Unloaded(PathBuf),
    LoadFailed(Arc<anyhow::Error>),
}

#[derive(Debug)]
pub struct LanguageNotFound;

impl std::fmt::Display for LanguageNotFound {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "language not found")
    }
}

pub struct LoadedLanguage {
    pub config: LanguageConfig,
    pub queries: LanguageQueries,
    pub context_provider: Option<Arc<dyn ContextProvider>>,
    pub toolchain_provider: Option<Arc<dyn ToolchainLister>>,
    pub manifest_name: Option<ManifestName>,
}

pub type LanguageLoader =
    Arc<dyn Fn() -> BoxFuture<'static, Result<LoadedLanguage>> + Send + Sync + 'static>;

impl LanguageRegistry {
    pub fn new(executor: BackgroundExecutor) -> Self {
        let this = Self {
            state: RwLock::new(LanguageRegistryState {
                next_language_server_id: 0,
                languages: Vec::new(),
                available_languages: AvailableLanguages::default(),
                grammars: Default::default(),
                language_settings: Default::default(),
                loading_languages: Default::default(),
                subscription: watch::channel(),
                theme: Default::default(),
                version: 0,
                reload_count: 0,
            }),
            executor,
        };
        this.add(PLAIN_TEXT.clone());
        this
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn test(executor: BackgroundExecutor) -> Self {
        Self::new(executor)
    }

    /// Clears out all of the loaded languages and reload them from scratch.
    pub fn reload(&self) {
        self.state.write().reload();
    }

    /// Removes the specified languages and grammars from the registry.
    pub fn remove_languages(
        &self,
        languages_to_remove: &[LanguageName],
        grammars_to_remove: &[Arc<str>],
    ) {
        self.state
            .write()
            .remove_languages(languages_to_remove, grammars_to_remove)
    }

    #[cfg(any(feature = "test-support", test))]
    pub fn register_test_language(&self, config: LanguageConfig) {
        self.register_language(
            config.name.clone(),
            config.grammar.clone(),
            config.matcher.clone(),
            config.hidden,
            None,
            Arc::new(move || {
                let config = config.clone();
                async move {
                    Ok(LoadedLanguage {
                        config,
                        queries: Default::default(),
                        toolchain_provider: None,
                        context_provider: None,
                        manifest_name: None,
                    })
                }
                .boxed()
            }),
        )
    }

    /// Adds a language to the registry, which can be loaded if needed.
    pub fn register_language(
        &self,
        name: LanguageName,
        grammar_name: Option<Arc<str>>,
        matcher: Arc<LanguageMatcher>,
        hidden: bool,
        manifest_name: Option<ManifestName>,
        load: LanguageLoader,
    ) {
        let state = &mut *self.state.write();

        let was_added = state.available_languages.register(
            name,
            grammar_name,
            matcher,
            hidden,
            manifest_name,
            load,
        );
        if !was_added {
            return;
        }

        state.version += 1;
        state.reload_count += 1;
        *state.subscription.0.borrow_mut() = ();
    }

    /// Adds grammars to the registry. Language configurations reference a grammar by name. The
    /// grammar controls how the source code is parsed.
    pub fn register_native_grammars(
        &self,
        grammars: impl IntoIterator<Item = (impl Into<Arc<str>>, impl Into<tree_sitter::Language>)>,
    ) {
        self.state.write().grammars.extend(
            grammars
                .into_iter()
                .map(|(name, grammar)| (name.into(), AvailableGrammar::Native(grammar.into()))),
        );
    }

    /// Adds paths to WASM grammar files, which can be loaded if needed.
    pub fn register_wasm_grammars(&self, grammars: Vec<(Arc<str>, PathBuf)>) {
        if grammars.is_empty() {
            return;
        }

        let mut state = self.state.write();
        state.grammars.extend(
            grammars
                .into_iter()
                .map(|(name, path)| (name, AvailableGrammar::Unloaded(path))),
        );
        state.version += 1;
        state.reload_count += 1;
        *state.subscription.0.borrow_mut() = ();
    }

    pub fn language_settings(&self) -> AllLanguageSettingsContent {
        self.state.read().language_settings.clone()
    }

    pub fn language_names(&self) -> Vec<LanguageName> {
        let state = self.state.read();
        let mut result = state.available_languages.unloaded_language_names();
        result.extend(
            state
                .languages
                .iter()
                .map(|language| language.config.name.clone()),
        );
        result.sort_unstable_by_key(|language_name| language_name.as_ref().to_lowercase());
        result
    }

    pub fn grammar_names(&self) -> Vec<Arc<str>> {
        let state = self.state.read();
        let mut result = state.grammars.keys().cloned().collect::<Vec<_>>();
        result.sort_unstable_by_key(|grammar_name| grammar_name.to_lowercase());
        result
    }

    /// Add a pre-loaded language to the registry.
    pub fn add(&self, language: Arc<Language>) {
        let mut state = self.state.write();
        state.available_languages.add(AvailableLanguage {
            id: language.id,
            name: language.name(),
            grammar: language.config.grammar.clone(),
            matcher: language.config.matcher.clone(),
            hidden: language.config.hidden,
            manifest_name: None,
            load: Arc::new(|| async { Err(anyhow!("already loaded")) }.boxed()),
            loaded: true,
        });
        state.add(language);
    }

    pub fn subscribe(&self) -> watch::Receiver<()> {
        self.state.read().subscription.1.clone()
    }

    /// Returns the number of times that the registry has been changed,
    /// by adding languages or reloading.
    pub fn version(&self) -> usize {
        self.state.read().version
    }

    /// Returns the number of times that the registry has been reloaded.
    pub fn reload_count(&self) -> usize {
        self.state.read().reload_count
    }

    pub fn set_theme(&self, theme: Arc<Theme>) {
        let mut state = self.state.write();
        state.theme = Some(theme.clone());
        for language in &state.languages {
            language.set_theme(theme.syntax());
        }
    }

    pub fn language_for_name(
        self: &Arc<Self>,
        name: &str,
    ) -> impl Future<Output = Result<Arc<Language>>> + use<> {
        let language_id = self.state.read().available_languages.find_by_name(name);
        let rx = self.get_or_load_language(language_id);
        async move { rx.await? }
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn language_name_for_id(&self, id: LanguageId) -> Option<LanguageName> {
        self.state.read().available_languages.name_for_id(id)
    }

    pub fn language_name_for_extension(self: &Arc<Self>, extension: &str) -> Option<LanguageName> {
        self.state
            .try_read()
            .and_then(|state| state.available_languages.find_name_by_extension(extension))
    }

    pub fn language_for_name_or_extension(
        self: &Arc<Self>,
        string: &str,
    ) -> impl Future<Output = Result<Arc<Language>>> {
        let language_id = self
            .state
            .read()
            .available_languages
            .find_by_name_or_extension(string);
        let rx = self.get_or_load_language(language_id);
        async move { rx.await? }
    }

    pub fn available_language_for_name(self: &Arc<Self>, name: &str) -> Option<AvailableLanguage> {
        let state = self.state.read();
        state.available_languages.find_by_exact_name(name)
    }

    /// Look up a language by its modeline name (vim filetype or emacs mode).
    ///
    /// This performs a case-insensitive match against:
    /// 1. Explicit modeline aliases defined in the language config
    /// 2. The language's grammar name
    /// 3. The language name itself
    pub fn available_language_for_modeline_name(
        self: &Arc<Self>,
        modeline_name: &str,
    ) -> Option<AvailableLanguage> {
        self.state
            .read()
            .available_languages
            .find_by_modeline_name(modeline_name)
    }

    pub fn language_for_file(
        self: &Arc<Self>,
        file: &Arc<dyn File>,
        content: Option<&Rope>,
        cx: &App,
    ) -> Option<LanguageId> {
        let user_file_types = all_language_settings(Some(file), cx);

        self.language_for_file_internal(
            &file.full_path(cx),
            content,
            Some(&user_file_types.file_types),
        )
    }

    pub fn language_for_file_path(self: &Arc<Self>, path: &Path) -> Option<LanguageId> {
        self.language_for_file_internal(path, None, None)
    }

    #[ztracing::instrument(skip_all)]
    pub fn load_language_for_file_path<'a>(
        self: &Arc<Self>,
        path: &'a Path,
    ) -> impl Future<Output = Result<Arc<Language>>> + 'a {
        let language = self.language_for_file_path(path);

        let this = self.clone();
        async move {
            if let Some(language_id) = language {
                this.load_language(language_id).await?
            } else {
                Err(anyhow!(LanguageNotFound))
            }
        }
    }

    fn language_for_file_internal(
        self: &Arc<Self>,
        path: &Path,
        content: Option<&Rope>,
        user_file_types: Option<&FxHashMap<Arc<str>, (GlobSet, Vec<String>)>>,
    ) -> Option<LanguageId> {
        self.state
            .read()
            .available_languages
            .find_for_file(path, content, user_file_types)
    }

    #[ztracing::instrument(skip_all)]
    pub fn load_language(
        self: &Arc<Self>,
        language_id: LanguageId,
    ) -> oneshot::Receiver<Result<Arc<Language>>> {
        let (tx, rx) = oneshot::channel();

        let mut state = self.state.write();

        // If the language is already loaded, resolve with it immediately.
        for loaded_language in state.languages.iter() {
            if loaded_language.id == language_id {
                tx.send(Ok(loaded_language.clone())).unwrap();
                return rx;
            }
        }

        let Some(available_language) = state.available_languages.get_language(language_id) else {
            tx.send(Err(anyhow!(LanguageNotFound))).ok();
            return rx;
        };

        let (language_name, language_load) = (
            available_language.name.clone(),
            available_language.load.clone(),
        );

        match state.loading_languages.entry(language_id) {
            // If the language is already being loaded, then add this
            // channel to a list that will be sent to when the load completes.
            hash_map::Entry::Occupied(mut entry) => entry.get_mut().push(tx),

            // Otherwise, start loading the language.
            hash_map::Entry::Vacant(entry) => {
                let this = self.clone();

                self.executor
                    .spawn(async move {
                        let language = async {
                            let loaded_language = (language_load)().await?;
                            if let Some(grammar) = loaded_language.config.grammar.clone() {
                                let grammar = Some(this.get_or_load_grammar(grammar).await?);

                                Language::new_with_id(language_id, loaded_language.config, grammar)
                                    .with_context_provider(loaded_language.context_provider)
                                    .with_toolchain_lister(loaded_language.toolchain_provider)
                                    .with_manifest(loaded_language.manifest_name)
                                    .with_queries(loaded_language.queries)
                            } else {
                                Ok(
                                    Language::new_with_id(
                                        language_id,
                                        loaded_language.config,
                                        None,
                                    )
                                    .with_context_provider(loaded_language.context_provider)
                                    .with_manifest(loaded_language.manifest_name)
                                    .with_toolchain_lister(loaded_language.toolchain_provider),
                                )
                            }
                        }
                        .await;

                        match language {
                            Ok(language) => {
                                let language = Arc::new(language);
                                let mut state = this.state.write();

                                state.add(language.clone());
                                state.mark_language_loaded(language_id);
                                if let Some(mut txs) = state.loading_languages.remove(&language_id)
                                {
                                    for tx in txs.drain(..) {
                                        let _ = tx.send(Ok(language.clone()));
                                    }
                                }
                            }
                            Err(e) => {
                                log::error!("failed to load language {language_name}:\n{e:?}");
                                let mut state = this.state.write();
                                state.mark_language_loaded(language_id);
                                if let Some(mut txs) = state.loading_languages.remove(&language_id)
                                {
                                    for tx in txs.drain(..) {
                                        let _ = tx.send(Err(anyhow!(
                                            "failed to load language {language_name}: {e}",
                                        )));
                                    }
                                }
                            }
                        };
                    })
                    .detach();

                entry.insert(vec![tx]);
            }
        }

        drop(state);
        rx
    }

    #[ztracing::instrument(skip_all)]
    fn get_or_load_language(
        self: &Arc<Self>,
        language_id: Option<LanguageId>,
    ) -> oneshot::Receiver<Result<Arc<Language>>> {
        let Some(language_id) = language_id else {
            let (tx, rx) = oneshot::channel();
            let _ = tx.send(Err(anyhow!(LanguageNotFound)));
            return rx;
        };

        self.load_language(language_id)
    }

    fn get_or_load_grammar(
        self: &Arc<Self>,
        name: Arc<str>,
    ) -> impl Future<Output = Result<tree_sitter::Language>> {
        let span = ztracing::debug_span!("get_or_load_grammar", name = &*name.clone());
        let _enter = span.enter();
        let (tx, rx) = oneshot::channel();
        let mut state = self.state.write();

        if let Some(grammar) = state.grammars.get_mut(name.as_ref()) {
            match grammar {
                AvailableGrammar::LoadFailed(error) => {
                    tx.send(Err(error.clone())).ok();
                }
                AvailableGrammar::Native(grammar) | AvailableGrammar::Loaded(_, grammar) => {
                    tx.send(Ok(grammar.clone())).ok();
                }
                AvailableGrammar::Loading(_, txs) => {
                    txs.push(tx);
                }
                AvailableGrammar::Unloaded(wasm_path) => {
                    log::trace!("start loading grammar {name:?}");
                    let this = self.clone();
                    let wasm_path = wasm_path.clone();
                    *grammar = AvailableGrammar::Loading(wasm_path.clone(), vec![tx]);
                    self.executor
                        .spawn(async move {
                            let grammar_result = maybe!({
                                let wasm_bytes = std::fs::read(&wasm_path)?;
                                let grammar_name = wasm_path
                                    .file_stem()
                                    .and_then(OsStr::to_str)
                                    .context("invalid grammar filename")?;
                                anyhow::Ok(with_parser(|parser| {
                                    let mut store = parser.take_wasm_store().unwrap();
                                    let grammar = store.load_language(grammar_name, &wasm_bytes);
                                    parser.set_wasm_store(store).unwrap();
                                    grammar
                                })?)
                            })
                            .map_err(Arc::new);

                            let value = match &grammar_result {
                                Ok(grammar) => AvailableGrammar::Loaded(wasm_path, grammar.clone()),
                                Err(error) => AvailableGrammar::LoadFailed(error.clone()),
                            };

                            log::trace!("finish loading grammar {name:?}");
                            let old_value = this.state.write().grammars.insert(name, value);
                            if let Some(AvailableGrammar::Loading(_, txs)) = old_value {
                                for tx in txs {
                                    tx.send(grammar_result.clone()).ok();
                                }
                            }
                        })
                        .detach();
                }
            }
        } else {
            tx.send(Err(Arc::new(anyhow!("no such grammar {name}"))))
                .ok();
        }

        async move { rx.await?.map_err(|e| anyhow!(e)) }
    }

    pub fn to_vec(&self) -> Vec<Arc<Language>> {
        self.state.read().languages.to_vec()
    }

    pub fn next_language_server_id(&self) -> LanguageServerId {
        self.state.write().next_language_server_id()
    }
}

impl LanguageRegistryState {
    fn next_language_server_id(&mut self) -> LanguageServerId {
        LanguageServerId(post_inc(&mut self.next_language_server_id))
    }

    fn add(&mut self, language: Arc<Language>) {
        if let Some(theme) = self.theme.as_ref() {
            language.set_theme(theme.syntax());
        }
        self.language_settings.languages.0.insert(
            language.name().0.to_string(),
            LanguageSettingsContent {
                tab_size: language.config.tab_size,
                hard_tabs: language.config.hard_tabs,
                soft_wrap: language.config.soft_wrap.map(crate::to_settings_soft_wrap),
                auto_indent_on_paste: language.config.auto_indent_on_paste,
                ..Default::default()
            },
        );
        self.languages.push(language);
        self.version += 1;
        *self.subscription.0.borrow_mut() = ();
    }

    fn reload(&mut self) {
        self.languages.clear();
        self.version += 1;
        self.reload_count += 1;
        self.available_languages.mark_all_unloaded();
        *self.subscription.0.borrow_mut() = ();
    }

    fn remove_languages(
        &mut self,
        languages_to_remove: &[LanguageName],
        grammars_to_remove: &[Arc<str>],
    ) {
        if languages_to_remove.is_empty() && grammars_to_remove.is_empty() {
            return;
        }

        self.languages
            .retain(|language| !languages_to_remove.contains(&language.name()));
        self.available_languages.remove(languages_to_remove);
        self.grammars
            .retain(|name, _| !grammars_to_remove.contains(name));
        self.version += 1;
        self.reload_count += 1;
        *self.subscription.0.borrow_mut() = ();
    }

    /// Mark the given language as having been loaded, so that the
    /// language registry won't try to load it again.
    fn mark_language_loaded(&mut self, id: LanguageId) {
        self.available_languages.mark_loaded(id);
    }
}
