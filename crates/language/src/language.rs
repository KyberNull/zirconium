//! The `language` crate provides a large chunk of Zed's language-related
//! features (the other big contributors being project and lsp crates that revolve around LSP features).
//! Namely, this crate:
//! - Provides [`Language`], [`Grammar`] and [`LanguageRegistry`] types that
//!   use Tree-sitter to provide syntax highlighting to the editor; note though that `language` doesn't perform the highlighting by itself. It only maps ranges in a buffer to colors. Treesitter is also used for buffer outlines (lists of symbols in a buffer)
//! - Exposes [`LanguageConfig`] that describes how constructs (like brackets or line comments) should be handled by the editor for a source file of a particular language.
//!
//! Notably we do *not* assign a single language to a single file; in real world a single file can consist of multiple programming languages - HTML is a good example of that - and `language` crate tends to reflect that status quo in its API.
mod available_languages;
mod buffer;
mod diagnostic;
mod diagnostic_set;
mod file_content;
mod language_registry;

pub mod language_settings;
mod manifest;
pub mod modeline;
mod outline;
pub mod proto;
mod runnable;
mod syntax_map;
mod task_context;
mod text_diff;
mod toolchain;

#[cfg(test)]
pub mod buffer_tests;

pub use crate::language_settings::{
    AutoIndentMode, EditPredictionPromptFormat, EditPredictionsMode, IndentGuideSettings,
    ZetaVersion,
};
use anyhow::Result;
use collections::HashSet;
use gpui::Entity;

pub use language_core::{
    SymbolKind,
    highlight_map::{HighlightId, HighlightMap},
};

pub use language_core::{
    BlockCommentConfig, BracketPair, BracketPairConfig, BracketPairContent, BracketsConfig,
    BracketsPatternConfig, CodeLabel, CodeLabelBuilder, DecreaseIndentConfig, Grammar, GrammarId,
    HighlightsConfig, IndentConfig, InjectionConfig, InjectionPatternConfig, JsxTagAutoCloseConfig,
    LanguageConfig, LanguageConfigOverride, LanguageId, LanguageMatcher, OrderedListConfig,
    OutlineConfig, Override, OverrideConfig, OverrideEntry, RedactionConfig, RunnableCapture,
    RunnableConfig, SoftWrap, Symbol, TaskListConfig, TextObject, TextObjectConfig,
    WrapCharactersConfig, default_true, deserialize_regex, deserialize_regex_vec,
    regex_json_schema, regex_vec_json_schema, serialize_regex,
};
pub use language_registry::{LanguageLoader, LanguageName, LoadedLanguage};
pub use manifest::{ManifestDelegate, ManifestName, ManifestProvider, ManifestQuery};
pub use modeline::{ModelineSettings, parse_modeline};
use parking_lot::Mutex;
use regex::Regex;
pub use runnable::{ResolvedRunnable, RunnableMatchCapture, RunnableRange, RunnableResolver};
use std::{
    fmt::Debug,
    hash::Hash,
    ops::{DerefMut, Range},
    str,
    sync::{Arc, LazyLock},
};
use syntax_map::{QueryCursorHandle, SyntaxSnapshot};
use task::RunnableTag;
pub use task_context::{ContextLocation, ContextProvider};
pub use text_diff::{
    DiffOptions, apply_diff_patch, apply_reversed_diff_patch, char_diff, line_diff, text_diff,
    text_diff_with_options, unified_diff, unified_diff_with_context, unified_diff_with_offsets,
    word_diff_ranges,
};
use theme::SyntaxTheme;
pub use toolchain::{
    LanguageToolchainStore, LocalLanguageToolchainStore, Toolchain, ToolchainList, ToolchainLister,
    ToolchainMetadata, ToolchainScope,
};
use tree_sitter::{self, QueryCursor, WasmStore, wasmtime};

pub use available_languages::AvailableLanguage;
pub use buffer::Operation;
pub use buffer::*;
pub use diagnostic::{Diagnostic, DiagnosticSourceKind, RelatedInformation, RelatedLocation};
pub use diagnostic_set::{DiagnosticEntry, DiagnosticEntryRef, DiagnosticGroup};
pub use file_content::{
    ByteContent, DecodedText, FILE_ANALYSIS_BYTES, analyze_byte_content, decode_text, encode_text,
};
pub use language_registry::{
    LanguageNotFound, LanguageQueries, LanguageRegistry, QueryFile, QueryFileContents, QueryFiles,
};
pub use lsp::{LanguageServerId, LanguageServerName};
pub use outline::*;
pub use syntax_map::{
    OwnedSyntaxLayer, SyntaxLayer, SyntaxMapMatches, ToTreeSitterPoint, TreeSitterOptions,
};
pub use text::{AnchorRangeExt, LineEnding};
pub use tree_sitter::{Node, Parser, QueryCapture, Tree, TreeCursor};

pub(crate) fn to_settings_soft_wrap(value: language_core::SoftWrap) -> settings::SoftWrap {
    match value {
        language_core::SoftWrap::None => settings::SoftWrap::None,
        language_core::SoftWrap::PreferLine => settings::SoftWrap::PreferLine,
        language_core::SoftWrap::EditorWidth => settings::SoftWrap::EditorWidth,
        language_core::SoftWrap::Bounded => settings::SoftWrap::Bounded,
    }
}

static QUERY_CURSORS: Mutex<Vec<QueryCursor>> = Mutex::new(vec![]);
static PARSERS: Mutex<Vec<Parser>> = Mutex::new(vec![]);

#[ztracing::instrument(skip_all)]
pub fn with_parser<F, R>(func: F) -> R
where
    F: FnOnce(&mut Parser) -> R,
{
    let mut parser = PARSERS.lock().pop().unwrap_or_else(|| {
        let mut parser = Parser::new();
        parser
            .set_wasm_store(WasmStore::new(&WASM_ENGINE).unwrap())
            .unwrap();
        parser
    });
    // Tree-sitter auto-resets the parser at the end of a successful parse,
    // but the cancellation paths (progress callback returning `Break`,
    // cancelled balancing) leave outstanding state on the parser. The next
    // call to `parse_with_options` would then *resume* that cancelled parse
    // instead of starting fresh.
    parser.reset();
    parser.set_included_ranges(&[]).unwrap();
    let result = func(&mut parser);
    PARSERS.lock().push(parser);
    result
}

pub fn with_query_cursor<F, R>(func: F) -> R
where
    F: FnOnce(&mut QueryCursor) -> R,
{
    let mut cursor = QueryCursorHandle::new();
    func(cursor.deref_mut())
}

static WASM_ENGINE: LazyLock<wasmtime::Engine> = LazyLock::new(|| {
    wasmtime::Engine::new(&wasmtime::Config::new()).expect("Failed to create Wasmtime engine")
});

/// A shared grammar for plain text, exposed for reuse by downstream crates.
pub static PLAIN_TEXT: LazyLock<Arc<Language>> = LazyLock::new(|| {
    Arc::new(Language::new(
        LanguageConfig {
            name: "Plain Text".into(),
            soft_wrap: Some(SoftWrap::EditorWidth),
            autoclose_before: ")]}".into(),
            matcher: (LanguageMatcher {
                path_suffixes: vec!["txt".to_owned()],
                first_line_pattern: None,
                modeline_aliases: vec!["text".to_owned(), "txt".to_owned()],
            })
            .into(),
            brackets: BracketPairConfig {
                pairs: vec![
                    BracketPair {
                        start: "(".to_string(),
                        end: ")".to_string(),
                        close: true,
                        surround: true,
                        newline: false,
                    },
                    BracketPair {
                        start: "[".to_string(),
                        end: "]".to_string(),
                        close: true,
                        surround: true,
                        newline: false,
                    },
                    BracketPair {
                        start: "{".to_string(),
                        end: "}".to_string(),
                        close: true,
                        surround: true,
                        newline: false,
                    },
                    BracketPair {
                        start: "\"".to_string(),
                        end: "\"".to_string(),
                        close: true,
                        surround: true,
                        newline: false,
                    },
                    BracketPair {
                        start: "'".to_string(),
                        end: "'".to_string(),
                        close: true,
                        surround: true,
                        newline: false,
                    },
                ],
                disabled_scopes_by_bracket_ix: Default::default(),
            },
            ..Default::default()
        },
        None,
    ))
});

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Location {
    pub buffer: Entity<Buffer>,
    pub range: Range<Anchor>,
}

/// Represents a language for the given range. Some languages (e.g. HTML)
/// interleave several languages together, thus a single buffer might actually contain
/// several nested scopes.
#[derive(Clone, Debug)]
pub struct LanguageScope {
    language: Arc<Language>,
    override_id: Option<u32>,
}

pub struct Language {
    pub(crate) id: LanguageId,
    pub(crate) config: LanguageConfig,
    pub(crate) grammar: Option<Arc<Grammar>>,
    pub(crate) context_provider: Option<Arc<dyn ContextProvider>>,
    pub(crate) toolchain: Option<Arc<dyn ToolchainLister>>,
    pub(crate) manifest_name: Option<ManifestName>,
}

impl Language {
    pub fn new(config: LanguageConfig, ts_language: Option<tree_sitter::Language>) -> Self {
        Self::new_with_id(LanguageId::new(), config, ts_language)
    }

    pub fn id(&self) -> LanguageId {
        self.id
    }

    fn new_with_id(
        id: LanguageId,
        config: LanguageConfig,
        ts_language: Option<tree_sitter::Language>,
    ) -> Self {
        Self {
            id,
            config,
            grammar: ts_language.map(|ts_language| Arc::new(Grammar::new(ts_language))),
            context_provider: None,
            toolchain: None,
            manifest_name: None,
        }
    }

    pub fn with_context_provider(mut self, provider: Option<Arc<dyn ContextProvider>>) -> Self {
        self.context_provider = provider;
        self
    }

    pub fn with_toolchain_lister(mut self, provider: Option<Arc<dyn ToolchainLister>>) -> Self {
        self.toolchain = provider;
        self
    }

    pub fn with_manifest(mut self, name: Option<ManifestName>) -> Self {
        self.manifest_name = name;
        self
    }

    pub fn with_queries(mut self, queries: LanguageQueries) -> Result<Self> {
        if let Some(grammar) = self.grammar.take() {
            let grammar =
                Arc::try_unwrap(grammar).map_err(|_| anyhow::anyhow!("cannot mutate grammar"))?;
            let grammar = grammar.with_queries(queries, &mut self.config)?;
            self.grammar = Some(Arc::new(grammar));
        }
        Ok(self)
    }

    pub fn with_highlights_query(self, source: &str) -> Result<Self> {
        self.with_grammar_query(|grammar| grammar.with_highlights_query(source))
    }

    pub fn with_runnable_query(self, source: &str) -> Result<Self> {
        self.with_grammar_query(|grammar| grammar.with_runnable_query(source))
    }

    pub fn with_outline_query(self, source: &str) -> Result<Self> {
        self.with_grammar_query_and_name(|grammar, name| grammar.with_outline_query(source, name))
    }

    pub fn with_text_object_query(self, source: &str) -> Result<Self> {
        self.with_grammar_query_and_name(|grammar, name| {
            grammar.with_text_object_query(source, name)
        })
    }

    pub fn with_brackets_query(self, source: &str) -> Result<Self> {
        self.with_grammar_query_and_name(|grammar, name| grammar.with_brackets_query(source, name))
    }

    pub fn with_indents_query(self, source: &str) -> Result<Self> {
        self.with_grammar_query_and_name(|grammar, name| grammar.with_indents_query(source, name))
    }

    pub fn with_injection_query(self, source: &str) -> Result<Self> {
        self.with_grammar_query_and_name(|grammar, name| grammar.with_injection_query(source, name))
    }

    pub fn with_override_query(mut self, source: &str) -> Result<Self> {
        if let Some(grammar_arc) = self.grammar.take() {
            let grammar = Arc::try_unwrap(grammar_arc)
                .map_err(|_| anyhow::anyhow!("cannot mutate grammar"))?;
            let grammar = grammar.with_override_query(
                source,
                &self.config.name,
                &self.config.overrides,
                &mut self.config.brackets,
                &self.config.scope_opt_in_language_servers,
            )?;
            self.grammar = Some(Arc::new(grammar));
        }
        Ok(self)
    }

    pub fn with_redaction_query(self, source: &str) -> Result<Self> {
        self.with_grammar_query_and_name(|grammar, name| grammar.with_redaction_query(source, name))
    }

    fn with_grammar_query(
        mut self,
        build: impl FnOnce(Grammar) -> Result<Grammar>,
    ) -> Result<Self> {
        if let Some(grammar_arc) = self.grammar.take() {
            let grammar = Arc::try_unwrap(grammar_arc)
                .map_err(|_| anyhow::anyhow!("cannot mutate grammar"))?;
            self.grammar = Some(Arc::new(build(grammar)?));
        }
        Ok(self)
    }

    fn with_grammar_query_and_name(
        mut self,
        build: impl FnOnce(Grammar, &LanguageName) -> Result<Grammar>,
    ) -> Result<Self> {
        if let Some(grammar_arc) = self.grammar.take() {
            let grammar = Arc::try_unwrap(grammar_arc)
                .map_err(|_| anyhow::anyhow!("cannot mutate grammar"))?;
            self.grammar = Some(Arc::new(build(grammar, &self.config.name)?));
        }
        Ok(self)
    }

    pub fn name(&self) -> LanguageName {
        self.config.name.clone()
    }
    pub fn manifest(&self) -> Option<&ManifestName> {
        self.manifest_name.as_ref()
    }

    pub fn code_fence_block_name(&self) -> Arc<str> {
        self.config
            .code_fence_block_name
            .clone()
            .unwrap_or_else(|| self.config.name.as_ref().to_lowercase().into())
    }

    pub fn context_provider(&self) -> Option<Arc<dyn ContextProvider>> {
        self.context_provider.clone()
    }

    pub fn toolchain_lister(&self) -> Option<Arc<dyn ToolchainLister>> {
        self.toolchain.clone()
    }

    pub fn highlight_text<'a>(
        self: &'a Arc<Self>,
        text: &'a Rope,
        range: Range<usize>,
    ) -> Vec<(Range<usize>, HighlightId)> {
        let mut result = Vec::new();
        if let Some(grammar) = &self.grammar {
            let tree = parse_text(grammar, text, None);
            let captures =
                SyntaxSnapshot::single_tree_captures(range.clone(), text, &tree, self, |grammar| {
                    grammar
                        .highlights_config
                        .as_ref()
                        .map(|config| &config.query)
                });
            let highlight_maps = vec![grammar.highlight_map()];
            let mut offset = 0;
            for chunk in
                BufferChunks::new(text, range, Some((captures, highlight_maps)), false, None)
            {
                let end_offset = offset + chunk.text.len();
                if let Some(highlight_id) = chunk.syntax_highlight_id {
                    result.push((offset..end_offset, highlight_id));
                }
                offset = end_offset;
            }
        }
        result
    }

    pub fn path_suffixes(&self) -> &[String] {
        &self.config.matcher.path_suffixes
    }

    pub fn should_autoclose_before(&self, c: char) -> bool {
        c.is_whitespace() || self.config.autoclose_before.contains(c)
    }

    pub fn set_theme(&self, theme: &SyntaxTheme) {
        if let Some(grammar) = self.grammar.as_ref()
            && let Some(highlights_config) = &grammar.highlights_config
        {
            *grammar.highlight_map.lock() =
                build_highlight_map(highlights_config.query.capture_names(), theme);
        }
    }

    pub fn grammar(&self) -> Option<&Arc<Grammar>> {
        self.grammar.as_ref()
    }

    pub fn default_scope(self: &Arc<Self>) -> LanguageScope {
        LanguageScope {
            language: self.clone(),
            override_id: None,
        }
    }

    pub fn lsp_id(&self) -> String {
        self.config.name.lsp_id()
    }

    pub fn snippet_scope_id(&self) -> String {
        self.config.name.snippet_scope_id()
    }

    pub fn prettier_parser_name(&self) -> Option<&str> {
        self.config.prettier_parser_name.as_deref()
    }

    pub fn config(&self) -> &LanguageConfig {
        &self.config
    }
}

#[inline]
pub fn build_highlight_map(capture_names: &[&str], theme: &SyntaxTheme) -> HighlightMap {
    HighlightMap::from_ids(
        capture_names
            .iter()
            .map(|capture_name| theme.highlight_id(capture_name).map(HighlightId::new)),
    )
}

impl LanguageScope {
    pub fn path_suffixes(&self) -> &[String] {
        self.language.path_suffixes()
    }

    pub fn language_name(&self) -> LanguageName {
        self.language.config.name.clone()
    }

    pub fn collapsed_placeholder(&self) -> &str {
        self.language.config.collapsed_placeholder.as_ref()
    }

    /// Returns line prefix that is inserted in e.g. line continuations or
    /// in `toggle comments` action.
    pub fn line_comment_prefixes(&self) -> &[Arc<str>] {
        Override::as_option(
            self.config_override().map(|o| &o.line_comments),
            Some(&self.language.config.line_comments),
        )
        .map_or([].as_slice(), |e| e.as_slice())
    }

    /// Config for block comments for this language.
    pub fn block_comment(&self) -> Option<&BlockCommentConfig> {
        Override::as_option(
            self.config_override().map(|o| &o.block_comment),
            self.language.config.block_comment.as_ref(),
        )
    }

    /// Config for documentation-style block comments for this language.
    pub fn documentation_comment(&self) -> Option<&BlockCommentConfig> {
        self.language.config.documentation_comment.as_ref()
    }

    /// Returns list markers that are inserted unchanged on newline (e.g., `- `, `* `, `+ `).
    pub fn unordered_list(&self) -> &[Arc<str>] {
        &self.language.config.unordered_list
    }

    /// Returns configuration for ordered lists with auto-incrementing numbers (e.g., `1. ` becomes `2. `).
    pub fn ordered_list(&self) -> &[OrderedListConfig] {
        &self.language.config.ordered_list
    }

    /// Returns configuration for task list continuation, if any (e.g., `- [x] ` continues as `- [ ] `).
    pub fn task_list(&self) -> Option<&TaskListConfig> {
        self.language.config.task_list.as_ref()
    }

    /// Returns additional regex patterns that act as prefix markers for creating
    /// boundaries during rewrapping.
    ///
    /// By default, Zed treats as paragraph and comment prefixes as boundaries.
    pub fn rewrap_prefixes(&self) -> &[Regex] {
        &self.language.config.rewrap_prefixes
    }

    /// Returns a list of language-specific word characters.
    ///
    /// By default, Zed treats alphanumeric characters (and '_') as word characters for
    /// the purpose of actions like 'move to next word end` or whole-word search.
    /// It additionally accounts for language's additional word characters.
    pub fn word_characters(&self) -> Option<&HashSet<char>> {
        Override::as_option(
            self.config_override().map(|o| &o.word_characters),
            Some(&self.language.config.word_characters),
        )
    }

    /// Returns a list of language-specific characters that are considered part of
    /// a completion query.
    pub fn completion_query_characters(&self) -> Option<&HashSet<char>> {
        Override::as_option(
            self.config_override()
                .map(|o| &o.completion_query_characters),
            Some(&self.language.config.completion_query_characters),
        )
    }

    /// Returns a list of language-specific characters that are considered part of
    /// identifiers during linked editing operations.
    pub fn linked_edit_characters(&self) -> Option<&HashSet<char>> {
        Override::as_option(
            self.config_override().map(|o| &o.linked_edit_characters),
            Some(&self.language.config.linked_edit_characters),
        )
    }

    /// Returns whether to prefer snippet `label` over `new_text` to replace text when
    /// completion is accepted.
    ///
    /// In cases like when cursor is in string or renaming existing function,
    /// you don't want to expand function signature instead just want function name
    /// to replace existing one.
    pub fn prefers_label_for_snippet_in_completion(&self) -> bool {
        self.config_override()
            .and_then(|o| o.prefer_label_for_snippet)
            .unwrap_or(false)
    }

    /// Returns a list of bracket pairs for a given language with an additional
    /// piece of information about whether the particular bracket pair is currently active for a given language.
    pub fn brackets(&self) -> impl Iterator<Item = (&BracketPair, bool)> {
        let mut disabled_ids = self
            .config_override()
            .map_or(&[] as _, |o| o.disabled_bracket_ixs.as_slice());
        self.language
            .config
            .brackets
            .pairs
            .iter()
            .enumerate()
            .map(move |(ix, bracket)| {
                let mut is_enabled = true;
                if let Some(next_disabled_ix) = disabled_ids.first()
                    && ix == *next_disabled_ix as usize
                {
                    disabled_ids = &disabled_ids[1..];
                    is_enabled = false;
                }
                (bracket, is_enabled)
            })
    }

    pub fn should_autoclose_before(&self, c: char) -> bool {
        c.is_whitespace() || self.language.config.autoclose_before.contains(c)
    }

    pub fn override_name(&self) -> Option<&str> {
        let id = self.override_id?;
        let grammar = self.language.grammar.as_ref()?;
        let override_config = grammar.override_config.as_ref()?;
        override_config.values.get(&id).map(|e| e.name.as_str())
    }

    fn config_override(&self) -> Option<&LanguageConfigOverride> {
        let id = self.override_id?;
        let grammar = self.language.grammar.as_ref()?;
        let override_config = grammar.override_config.as_ref()?;
        override_config.values.get(&id).map(|e| &e.value)
    }
}

impl Hash for Language {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state)
    }
}

impl PartialEq for Language {
    fn eq(&self, other: &Self) -> bool {
        self.id.eq(&other.id)
    }
}

impl Eq for Language {}

impl Debug for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Language")
            .field("name", &self.config.name)
            .finish()
    }
}

pub(crate) fn parse_text(grammar: &Grammar, text: &Rope, old_tree: Option<Tree>) -> Tree {
    with_parser(|parser| {
        parser
            .set_language(&grammar.ts_language)
            .expect("incompatible grammar");
        let mut chunks = text.chunks_in_range(0..text.len());
        parser
            .parse_with_options(
                &mut move |offset, _| {
                    chunks.seek(offset);
                    chunks.next().unwrap_or("").as_bytes()
                },
                old_tree.as_ref(),
                None,
            )
            .unwrap()
    })
}

pub fn point_to_lsp(point: PointUtf16) -> lsp::Position {
    lsp::Position::new(point.row, point.column)
}

pub fn point_from_lsp(point: lsp::Position) -> Unclipped<PointUtf16> {
    Unclipped(PointUtf16::new(point.line, point.character))
}

#[doc(hidden)]
#[cfg(any(test, feature = "test-support"))]
pub fn rust_lang() -> Arc<Language> {
    test_language("rust", tree_sitter_rust::LANGUAGE.into())
}

#[doc(hidden)]
#[cfg(any(test, feature = "test-support"))]
pub fn json_lang() -> Arc<Language> {
    test_language("json", tree_sitter_json::LANGUAGE.into())
}

#[doc(hidden)]
#[cfg(any(test, feature = "test-support"))]
pub fn markdown_lang() -> Arc<Language> {
    test_language("markdown", tree_sitter_md::LANGUAGE.into())
}

#[cfg(any(test, feature = "test-support"))]
fn test_language(name: &str, grammar: tree_sitter::Language) -> Arc<Language> {
    Arc::new(
        Language::new(grammars::load_config(name), Some(grammar))
            .with_queries(grammars::load_queries(name))
            .unwrap_or_else(|error| panic!("could not parse queries for language {name}: {error}")),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{TestAppContext, rgba};
    use pretty_assertions::assert_matches;

    #[test]
    fn test_highlight_map() {
        let theme = SyntaxTheme::new(
            [
                ("function", rgba(0x100000ff)),
                ("function.method", rgba(0x200000ff)),
                ("function.async", rgba(0x300000ff)),
                ("variable.builtin.self.rust", rgba(0x400000ff)),
                ("variable.builtin", rgba(0x500000ff)),
                ("variable", rgba(0x600000ff)),
            ]
            .iter()
            .map(|(name, color)| (name.to_string(), (*color).into())),
        );

        let capture_names = &[
            "function.special",
            "function.async.rust",
            "variable.builtin.self",
        ];

        let map = build_highlight_map(capture_names, &theme);
        assert_eq!(
            theme.get_capture_name(map.get(0).unwrap()),
            Some("function")
        );
        assert_eq!(
            theme.get_capture_name(map.get(1).unwrap()),
            Some("function.async")
        );
        assert_eq!(
            theme.get_capture_name(map.get(2).unwrap()),
            Some("variable.builtin")
        );
    }

    #[test]
    fn test_with_parser_resets_after_cancellation() {
        use std::ops::ControlFlow;
        use tree_sitter::{Language as TsLanguage, ParseOptions};

        let rust_language: TsLanguage = tree_sitter_rust::LANGUAGE.into();

        // Drain the shared pool so this test sees a deterministic LIFO order:
        // the parser we push at the end of the first `with_parser` call is the
        // one we pop at the start of the second call.
        PARSERS.lock().clear();

        // Large enough that tree-sitter invokes the progress callback before
        // the parse completes; otherwise the cancellation never fires.
        let large_input = format!("fn a() {{ {} }}", "b(c, d); e(f, g); ".repeat(5000));
        let small_input = "fn z() {}";

        // Cancel a parse via the progress callback. Tree-sitter retains the
        // in-progress parse state on the parser (its `canceled_balancing` flag
        // and/or non-empty parse stack), and the next call to
        // `parse_with_options` will *resume* that parse unless the parser is
        // reset first.
        let cancelled = with_parser(|parser| {
            parser.set_language(&rust_language).unwrap();
            let bytes = large_input.as_bytes();
            let mut break_immediately = |_: &_| ControlFlow::Break(());
            parser.parse_with_options(
                &mut |offset, _| {
                    if offset < bytes.len() {
                        &bytes[offset..]
                    } else {
                        &[]
                    }
                },
                None,
                Some(ParseOptions {
                    progress_callback: Some(&mut break_immediately),
                }),
            )
        });
        assert!(
            cancelled.is_none(),
            "first parse should be cancelled by the progress callback"
        );

        // Deliberately do NOT call `set_language` here: tree-sitter's
        // `ts_parser_set_language` internally calls `ts_parser_reset`, which
        // would mask the very bug we're checking for. Instead we rely on the
        // language being preserved across `parser.reset()` (it is) and verify
        // that `with_parser` itself produces a clean parser for the next user.
        let tree = with_parser(|parser| {
            let bytes = small_input.as_bytes();
            parser
                .parse_with_options(
                    &mut |offset, _| {
                        if offset < bytes.len() {
                            &bytes[offset..]
                        } else {
                            &[]
                        }
                    },
                    None,
                    None,
                )
                .expect("parse of small_input should succeed")
        });

        assert_eq!(tree.root_node().byte_range(), 0..small_input.len());
        assert_eq!(tree.root_node().kind(), "source_file");
        assert!(
            !tree.root_node().has_error(),
            "tree should be error-free, got: {}",
            tree.root_node().to_sexp()
        );
    }

    #[gpui::test(iterations = 10)]

    async fn test_language_loading(cx: &mut TestAppContext) {
        let languages = LanguageRegistry::test(cx.executor());
        let languages = Arc::new(languages);
        languages.register_native_grammars([
            ("json", tree_sitter_json::LANGUAGE),
            ("rust", tree_sitter_rust::LANGUAGE),
        ]);
        languages.register_test_language(LanguageConfig {
            name: "JSON".into(),
            grammar: Some("json".into()),
            matcher: (LanguageMatcher {
                path_suffixes: vec!["json".into()],
                ..Default::default()
            })
            .into(),
            ..Default::default()
        });
        languages.register_test_language(LanguageConfig {
            name: "Rust".into(),
            grammar: Some("rust".into()),
            matcher: (LanguageMatcher {
                path_suffixes: vec!["rs".into()],
                ..Default::default()
            })
            .into(),
            ..Default::default()
        });
        assert_eq!(
            languages.language_names(),
            &[
                LanguageName::new_static("JSON"),
                LanguageName::new_static("Plain Text"),
                LanguageName::new_static("Rust"),
            ]
        );

        let rust1 = languages.language_for_name("Rust");
        let rust2 = languages.language_for_name("Rust");

        // Ensure language is still listed even if it's being loaded.
        assert_eq!(
            languages.language_names(),
            &[
                LanguageName::new_static("JSON"),
                LanguageName::new_static("Plain Text"),
                LanguageName::new_static("Rust"),
            ]
        );

        let (rust1, rust2) = futures::join!(rust1, rust2);
        assert!(Arc::ptr_eq(&rust1.unwrap(), &rust2.unwrap()));

        // Ensure language is still listed even after loading it.
        assert_eq!(
            languages.language_names(),
            &[
                LanguageName::new_static("JSON"),
                LanguageName::new_static("Plain Text"),
                LanguageName::new_static("Rust"),
            ]
        );

        // Loading an unknown language returns an error.
        assert!(languages.language_for_name("Unknown").await.is_err());
    }

    #[test]
    fn test_deserializing_comments_backwards_compat() {
        // current version of `block_comment` and `documentation_comment` work
        {
            let config: LanguageConfig = ::toml::from_str(
                r#"
                name = "Foo"
                block_comment = { start = "a", end = "b", prefix = "c", tab_size = 1 }
                documentation_comment = { start = "d", end = "e", prefix = "f", tab_size = 2 }
                "#,
            )
            .unwrap();
            assert_matches!(config.block_comment, Some(BlockCommentConfig { .. }));
            assert_matches!(
                config.documentation_comment,
                Some(BlockCommentConfig { .. })
            );

            let block_config = config.block_comment.unwrap();
            assert_eq!(block_config.start.as_ref(), "a");
            assert_eq!(block_config.end.as_ref(), "b");
            assert_eq!(block_config.prefix.as_ref(), "c");
            assert_eq!(block_config.tab_size, 1);

            let doc_config = config.documentation_comment.unwrap();
            assert_eq!(doc_config.start.as_ref(), "d");
            assert_eq!(doc_config.end.as_ref(), "e");
            assert_eq!(doc_config.prefix.as_ref(), "f");
            assert_eq!(doc_config.tab_size, 2);
        }

        // former `documentation` setting is read into `documentation_comment`
        {
            let config: LanguageConfig = ::toml::from_str(
                r#"
                name = "Foo"
                documentation = { start = "a", end = "b", prefix = "c", tab_size = 1}
                "#,
            )
            .unwrap();
            assert_matches!(
                config.documentation_comment,
                Some(BlockCommentConfig { .. })
            );

            let config = config.documentation_comment.unwrap();
            assert_eq!(config.start.as_ref(), "a");
            assert_eq!(config.end.as_ref(), "b");
            assert_eq!(config.prefix.as_ref(), "c");
            assert_eq!(config.tab_size, 1);
        }

        // old block_comment format is read into BlockCommentConfig
        {
            let config: LanguageConfig = ::toml::from_str(
                r#"
                name = "Foo"
                block_comment = ["a", "b"]
                "#,
            )
            .unwrap();
            assert_matches!(config.block_comment, Some(BlockCommentConfig { .. }));

            let config = config.block_comment.unwrap();
            assert_eq!(config.start.as_ref(), "a");
            assert_eq!(config.end.as_ref(), "b");
            assert_eq!(config.prefix.as_ref(), "");
            assert_eq!(config.tab_size, 0);
        }
    }
}
