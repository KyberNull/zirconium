use crate::{
    ExtensionIndex, ExtensionIndexEntry, ExtensionIndexLanguageEntry, ExtensionIndexThemeEntry,
    ExtensionManifest, ExtensionStore, GrammarManifestEntry, RELOAD_DEBOUNCE_DURATION,
    SchemaVersion, load_plugin_queries,
};
use collections::BTreeMap;
use extension::ExtensionHostProxy;
use fs::FakeFs;
use gpui::{AppContext as _, BackgroundExecutor, TaskExt, TestAppContext};
use http_client::FakeHttpClient;
use language::{LanguageMatcher, LanguageName, LanguageRegistry, QueryFiles};
use node_runtime::NodeRuntime;
use serde_json::json;
use settings::SettingsStore;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use theme::ThemeRegistry;
use util::rel_path::rel_path_buf;

#[cfg(test)]
#[ctor::ctor(unsafe)]
fn init_logger() {
    zlog::init_test();
}

#[gpui::test]
async fn test_load_plugin_queries(executor: BackgroundExecutor) {
    let fs = FakeFs::new(executor);
    fs.insert_tree(
        "/queries",
        json!({
            "highlights.scm": "highlight query",
            "outline.scm": "outline query",
            "highlights_extra.scm": "ignored query",
            "locals.scm": "unrelated query",
        }),
    )
    .await;

    let queries = load_plugin_queries(
        fs.clone(),
        Path::new("/queries"),
        Some(QueryFiles::HIGHLIGHTS),
    )
    .await;
    assert_eq!(queries.highlights.as_deref(), Some("highlight query"));
    assert!(queries.outline.is_none());
    assert!(queries.brackets.is_none());

    let queries = load_plugin_queries(fs, Path::new("/queries"), None).await;
    assert_eq!(queries.highlights.as_deref(), Some("highlight query"));
    assert_eq!(queries.outline.as_deref(), Some("outline query"));
    assert!(queries.brackets.is_none());
}

#[gpui::test]
async fn test_extension_store(cx: &mut TestAppContext) {
    init_test(cx);

    let fs = FakeFs::new(cx.executor());
    let http_client = FakeHttpClient::with_200_response();

    fs.insert_tree(
        "/the-extension-dir",
        json!({
            "installed": {
                "zed-monokai": {
                    "extension.json": r#"{
                        "id": "zed-monokai",
                        "name": "Zed Monokai",
                        "version": "2.0.0",
                        "themes": {
                            "Monokai Dark": "themes/monokai.json",
                            "Monokai Light": "themes/monokai.json",
                            "Monokai Pro Dark": "themes/monokai-pro.json",
                            "Monokai Pro Light": "themes/monokai-pro.json"
                        }
                    }"#,
                    "themes": {
                        "monokai.json": r#"{
                            "name": "Monokai",
                            "author": "Someone",
                            "themes": [
                                {
                                    "name": "Monokai Dark",
                                    "appearance": "dark",
                                    "style": {}
                                },
                                {
                                    "name": "Monokai Light",
                                    "appearance": "light",
                                    "style": {}
                                }
                            ]
                        }"#,
                        "monokai-pro.json": r#"{
                            "name": "Monokai Pro",
                            "author": "Someone",
                            "themes": [
                                {
                                    "name": "Monokai Pro Dark",
                                    "appearance": "dark",
                                    "style": {}
                                },
                                {
                                    "name": "Monokai Pro Light",
                                    "appearance": "light",
                                    "style": {}
                                }
                            ]
                        }"#,
                    }
                },
                "zed-ruby": {
                    "extension.json": r#"{
                        "id": "zed-ruby",
                        "name": "Zed Ruby",
                        "version": "1.0.0",
                        "grammars": {
                            "ruby": "grammars/ruby.wasm",
                            "embedded_template": "grammars/embedded_template.wasm"
                        },
                        "languages": {
                            "ruby": "languages/ruby",
                            "erb": "languages/erb"
                        }
                    }"#,
                    "grammars": {
                        "ruby.wasm": "",
                        "embedded_template.wasm": "",
                    },
                    "languages": {
                        "ruby": {
                            "config.toml": r#"
                                name = "Ruby"
                                grammar = "ruby"
                                path_suffixes = ["rb"]
                            "#,
                            "highlights.scm": "",
                            "outline.scm": "",
                        },
                        "erb": {
                            "config.toml": r#"
                                name = "ERB"
                                grammar = "embedded_template"
                                path_suffixes = ["erb"]
                            "#,
                            "highlights.scm": "",
                        }
                    },
                }
            }
        }),
    )
    .await;

    let mut expected_index = ExtensionIndex {
        extensions: [
            (
                "zed-ruby".into(),
                ExtensionIndexEntry {
                    manifest: Arc::new(ExtensionManifest {
                        id: "zed-ruby".into(),
                        name: "Zed Ruby".into(),
                        version: "1.0.0".into(),
                        schema_version: SchemaVersion::ZERO,
                        description: None,
                        authors: Vec::new(),
                        repository: None,
                        themes: Default::default(),
                        icon_themes: Vec::new(),
                        lib: Default::default(),
                        languages: vec![
                            rel_path_buf("languages/erb"),
                            rel_path_buf("languages/ruby"),
                        ],
                        grammars: [
                            ("embedded_template".into(), GrammarManifestEntry::default()),
                            ("ruby".into(), GrammarManifestEntry::default()),
                        ]
                        .into_iter()
                        .collect(),
                        context_servers: BTreeMap::default(),
                        slash_commands: BTreeMap::default(),
                        snippets: None,
                        capabilities: Vec::new(),
                        language_model_providers: BTreeMap::default(),
                    }),
                    dev: false,
                },
            ),
            (
                "zed-monokai".into(),
                ExtensionIndexEntry {
                    manifest: Arc::new(ExtensionManifest {
                        id: "zed-monokai".into(),
                        name: "Zed Monokai".into(),
                        version: "2.0.0".into(),
                        schema_version: SchemaVersion::ZERO,
                        description: None,
                        authors: vec![],
                        repository: None,
                        themes: vec![
                            rel_path_buf("themes/monokai-pro.json"),
                            rel_path_buf("themes/monokai.json"),
                        ],
                        icon_themes: Vec::new(),
                        lib: Default::default(),
                        languages: Default::default(),
                        grammars: BTreeMap::default(),
                        context_servers: BTreeMap::default(),
                        slash_commands: BTreeMap::default(),
                        snippets: None,
                        capabilities: Vec::new(),
                        language_model_providers: BTreeMap::default(),
                    }),
                    dev: false,
                },
            ),
        ]
        .into_iter()
        .collect(),
        languages: [
            (
                "ERB".into(),
                ExtensionIndexLanguageEntry {
                    extension: "zed-ruby".into(),
                    path: "languages/erb".into(),
                    grammar: Some("embedded_template".into()),
                    hidden: false,
                    matcher: (LanguageMatcher {
                        path_suffixes: vec!["erb".into()],
                        first_line_pattern: None,
                        ..LanguageMatcher::default()
                    })
                    .into(),
                    query_files: Some(QueryFiles::HIGHLIGHTS),
                },
            ),
            (
                "Ruby".into(),
                ExtensionIndexLanguageEntry {
                    extension: "zed-ruby".into(),
                    path: "languages/ruby".into(),
                    grammar: Some("ruby".into()),
                    hidden: false,
                    matcher: (LanguageMatcher {
                        path_suffixes: vec!["rb".into()],
                        first_line_pattern: None,
                        ..LanguageMatcher::default()
                    })
                    .into(),
                    query_files: Some(QueryFiles::HIGHLIGHTS | QueryFiles::OUTLINE),
                },
            ),
        ]
        .into_iter()
        .collect(),
        themes: [
            (
                "Monokai Dark".into(),
                ExtensionIndexThemeEntry {
                    extension: "zed-monokai".into(),
                    path: "themes/monokai.json".into(),
                },
            ),
            (
                "Monokai Light".into(),
                ExtensionIndexThemeEntry {
                    extension: "zed-monokai".into(),
                    path: "themes/monokai.json".into(),
                },
            ),
            (
                "Monokai Pro Dark".into(),
                ExtensionIndexThemeEntry {
                    extension: "zed-monokai".into(),
                    path: "themes/monokai-pro.json".into(),
                },
            ),
            (
                "Monokai Pro Light".into(),
                ExtensionIndexThemeEntry {
                    extension: "zed-monokai".into(),
                    path: "themes/monokai-pro.json".into(),
                },
            ),
        ]
        .into_iter()
        .collect(),
        icon_themes: BTreeMap::default(),
    };

    let proxy = Arc::new(ExtensionHostProxy::new());
    let theme_registry = Arc::new(ThemeRegistry::new(Box::new(())));
    theme_extension::init(proxy.clone(), theme_registry.clone(), cx.executor());
    let language_registry = Arc::new(LanguageRegistry::test(cx.executor()));
    language_extension::init(proxy.clone(), language_registry.clone());
    let node_runtime = NodeRuntime::unavailable();

    let store = cx.new(|cx| {
        ExtensionStore::new(
            PathBuf::from("/the-extension-dir"),
            None,
            proxy.clone(),
            fs.clone(),
            http_client.clone(),
            http_client.clone(),
            None,
            node_runtime.clone(),
            cx,
        )
    });

    cx.executor().advance_clock(RELOAD_DEBOUNCE_DURATION);
    store.read_with(cx, |store, _| {
        let index = &store.extension_index;
        assert_eq!(index.extensions, expected_index.extensions);

        for ((actual_key, actual_language), (expected_key, expected_language)) in
            index.languages.iter().zip(expected_index.languages.iter())
        {
            assert_eq!(actual_key, expected_key);
            assert_eq!(actual_language.grammar, expected_language.grammar);
            assert_eq!(actual_language.matcher, expected_language.matcher);
            assert_eq!(actual_language.hidden, expected_language.hidden);
        }
        assert_eq!(index.themes, expected_index.themes);

        assert_eq!(
            language_registry.language_names(),
            [
                LanguageName::new_static("ERB"),
                LanguageName::new_static("Plain Text"),
                LanguageName::new_static("Ruby"),
            ]
        );
        assert_eq!(
            theme_registry.list_names(),
            [
                "Monokai Dark",
                "Monokai Light",
                "Monokai Pro Dark",
                "Monokai Pro Light",
                "One Dark",
            ]
        );
    });

    fs.insert_tree(
        "/the-extension-dir/installed/zed-gruvbox",
        json!({
            "extension.json": r#"{
                "id": "zed-gruvbox",
                "name": "Zed Gruvbox",
                "version": "1.0.0",
                "themes": {
                    "Gruvbox": "themes/gruvbox.json"
                }
            }"#,
            "themes": {
                "gruvbox.json": r#"{
                    "name": "Gruvbox",
                    "author": "Someone Else",
                    "themes": [
                        {
                            "name": "Gruvbox",
                            "appearance": "dark",
                            "style": {}
                        }
                    ]
                }"#,
            }
        }),
    )
    .await;

    expected_index.extensions.insert(
        "zed-gruvbox".into(),
        ExtensionIndexEntry {
            manifest: Arc::new(ExtensionManifest {
                id: "zed-gruvbox".into(),
                name: "Zed Gruvbox".into(),
                version: "1.0.0".into(),
                schema_version: SchemaVersion::ZERO,
                description: None,
                authors: vec![],
                repository: None,
                themes: vec![rel_path_buf("themes/gruvbox.json")],
                icon_themes: Vec::new(),
                lib: Default::default(),
                languages: Default::default(),
                grammars: BTreeMap::default(),
                context_servers: BTreeMap::default(),
                slash_commands: BTreeMap::default(),
                snippets: None,
                capabilities: Vec::new(),
                language_model_providers: BTreeMap::default(),
            }),
            dev: false,
        },
    );
    expected_index.themes.insert(
        "Gruvbox".into(),
        ExtensionIndexThemeEntry {
            extension: "zed-gruvbox".into(),
            path: "themes/gruvbox.json".into(),
        },
    );

    #[allow(clippy::let_underscore_future)]
    let _ = store.update(cx, |store, cx| store.reload(None, cx));

    cx.executor().advance_clock(RELOAD_DEBOUNCE_DURATION);
    store.read_with(cx, |store, _| {
        let index = &store.extension_index;

        for ((actual_key, actual_language), (expected_key, expected_language)) in
            index.languages.iter().zip(expected_index.languages.iter())
        {
            assert_eq!(actual_key, expected_key);
            assert_eq!(actual_language.grammar, expected_language.grammar);
            assert_eq!(actual_language.matcher, expected_language.matcher);
            assert_eq!(actual_language.hidden, expected_language.hidden);
        }

        assert_eq!(index.extensions, expected_index.extensions);
        assert_eq!(index.themes, expected_index.themes);

        assert_eq!(
            theme_registry.list_names(),
            [
                "Gruvbox",
                "Monokai Dark",
                "Monokai Light",
                "Monokai Pro Dark",
                "Monokai Pro Light",
                "One Dark",
            ]
        );
    });

    let prev_fs_metadata_call_count = fs.metadata_call_count();
    let prev_fs_read_dir_call_count = fs.read_dir_call_count();

    // Create new extension store, as if Zed were restarting.
    drop(store);
    let store = cx.new(|cx| {
        ExtensionStore::new(
            PathBuf::from("/the-extension-dir"),
            None,
            proxy,
            fs.clone(),
            http_client.clone(),
            http_client.clone(),
            None,
            node_runtime.clone(),
            cx,
        )
    });

    cx.executor().run_until_parked();
    store.read_with(cx, |store, _| {
        assert_eq!(store.extension_index.extensions, expected_index.extensions);
        assert_eq!(store.extension_index.themes, expected_index.themes);
        assert_eq!(
            store.extension_index.icon_themes,
            expected_index.icon_themes
        );

        for ((actual_key, actual_language), (expected_key, expected_language)) in store
            .extension_index
            .languages
            .iter()
            .zip(expected_index.languages.iter())
        {
            assert_eq!(actual_key, expected_key);
            assert_eq!(actual_language.grammar, expected_language.grammar);
            assert_eq!(actual_language.matcher, expected_language.matcher);
            assert_eq!(actual_language.hidden, expected_language.hidden);
        }

        assert_eq!(
            language_registry.language_names(),
            [
                LanguageName::new_static("ERB"),
                LanguageName::new_static("Plain Text"),
                LanguageName::new_static("Ruby"),
            ]
        );
        assert_eq!(
            language_registry.grammar_names(),
            ["embedded_template".into(), "ruby".into()]
        );
        assert_eq!(
            theme_registry.list_names(),
            [
                "Gruvbox",
                "Monokai Dark",
                "Monokai Light",
                "Monokai Pro Dark",
                "Monokai Pro Light",
                "One Dark",
            ]
        );

        // The on-disk manifest limits the number of FS calls that need to be made
        // on startup.
        assert_eq!(fs.read_dir_call_count(), prev_fs_read_dir_call_count);
        assert_eq!(fs.metadata_call_count(), prev_fs_metadata_call_count + 2);
    });

    store.update(cx, |store, cx| {
        store
            .uninstall_extension("zed-ruby".into(), cx)
            .detach_and_log_err(cx);
    });

    cx.executor().advance_clock(RELOAD_DEBOUNCE_DURATION);
    expected_index.extensions.remove("zed-ruby");
    expected_index.languages.remove("Ruby");
    expected_index.languages.remove("ERB");

    store.read_with(cx, |store, _| {
        assert_eq!(store.extension_index.extensions, expected_index.extensions);
        assert_eq!(store.extension_index.themes, expected_index.themes);
        assert_eq!(
            store.extension_index.icon_themes,
            expected_index.icon_themes
        );

        for ((actual_key, actual_language), (expected_key, expected_language)) in store
            .extension_index
            .languages
            .iter()
            .zip(expected_index.languages.iter())
        {
            assert_eq!(actual_key, expected_key);
            assert_eq!(actual_language.grammar, expected_language.grammar);
            assert_eq!(actual_language.matcher, expected_language.matcher);
            assert_eq!(actual_language.hidden, expected_language.hidden);
        }

        assert_eq!(
            language_registry.language_names(),
            [LanguageName::new_static("Plain Text")]
        );
        assert_eq!(language_registry.grammar_names(), []);
    });
}

fn init_test(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let store = SettingsStore::test(cx);
        cx.set_global(store);
        release_channel::init(semver::Version::new(0, 0, 0), cx);
        extension::init(cx);
        theme_settings::init(theme::LoadThemes::JustBase, cx);
        gpui_tokio::init(cx);
    });
}
