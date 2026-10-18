use std::path::PathBuf;
use std::sync::Arc;

use extension::{ExtensionGrammarProxy, ExtensionHostProxy, ExtensionLanguageProxy};
use language::{LanguageLoader, LanguageMatcher, LanguageName, LanguageRegistry};

pub fn init(extension_host_proxy: Arc<ExtensionHostProxy>, language_registry: Arc<LanguageRegistry>) {
    let language_registry_proxy = LanguageRegistryProxy { language_registry };
    extension_host_proxy.register_grammar_proxy(language_registry_proxy.clone());
    extension_host_proxy.register_language_proxy(language_registry_proxy);
}

#[derive(Clone)]
struct LanguageRegistryProxy {
    language_registry: Arc<LanguageRegistry>,
}

impl ExtensionGrammarProxy for LanguageRegistryProxy {
    #[ztracing::instrument(skip_all)]
    fn register_grammars(&self, grammars: Vec<(Arc<str>, PathBuf)>) {
        self.language_registry.register_wasm_grammars(grammars)
    }
}

impl ExtensionLanguageProxy for LanguageRegistryProxy {
    fn register_language(
        &self,
        language: LanguageName,
        grammar: Option<Arc<str>>,
        matcher: Arc<LanguageMatcher>,
        hidden: bool,
        load: LanguageLoader,
    ) {
        self.language_registry
            .register_language(language, grammar, matcher, hidden, None, load);
    }

    fn remove_languages(
        &self,
        languages_to_remove: &[LanguageName],
        grammars_to_remove: &[Arc<str>],
    ) {
        self.language_registry
            .remove_languages(languages_to_remove, grammars_to_remove);
    }
}
