use futures::FutureExt as _;
use gpui::{App, SharedString, UpdateGlobal};
use node_runtime::NodeRuntime;
use project::Fs;
use python::PyprojectTomlManifestProvider;
use rust::CargoManifestProvider;
use settings::SettingsStore;
use smol::stream::StreamExt;
use std::sync::Arc;
use util::ResultExt;

pub use language::*;

use crate::json::JsonTaskProvider;

mod bash;
#[cfg(test)]
mod c;
#[cfg(test)]
mod cpp;
#[cfg(test)]
mod css;
mod go;
mod json;
mod package_json;
mod python;
mod rust;
mod typescript;

pub(crate) use package_json::{PackageJson, PackageJsonData};

/// A shared grammar for plain text, exposed for reuse by downstream crates.
#[cfg(feature = "tree-sitter-gitcommit")]
pub static LANGUAGE_GIT_COMMIT: std::sync::LazyLock<Arc<Language>> =
    std::sync::LazyLock::new(|| {
        Arc::new(Language::new(
            LanguageConfig {
                name: "Git Commit".into(),
                soft_wrap: Some(language::SoftWrap::EditorWidth),
                matcher: (LanguageMatcher {
                    path_suffixes: vec!["COMMIT_EDITMSG".to_owned()],
                    first_line_pattern: None,
                    ..LanguageMatcher::default()
                })
                .into(),
                line_comments: vec![Arc::from("#")],
                ..LanguageConfig::default()
            },
            Some(tree_sitter_gitcommit::LANGUAGE.into()),
        ))
    });

pub fn init(languages: Arc<LanguageRegistry>, fs: Arc<dyn Fs>, _node: NodeRuntime, cx: &mut App) {
    #[cfg(feature = "load-grammars")]
    languages.register_native_grammars(grammars::native_grammars());

    let go_context_provider = Arc::new(go::GoContextProvider);
    let json_context_provider = Arc::new(JsonTaskProvider);
    let python_context_provider = Arc::new(python::PythonContextProvider);
    let python_toolchain_provider = Arc::new(python::PythonToolchainProvider::new(fs.clone()));
    let rust_context_provider = Arc::new(rust::RustContextProvider);
    let typescript_context = Arc::new(typescript::TypeScriptContextProvider::new(fs.clone()));

    let built_in_languages = [
        LanguageInfo {
            name: "bash",
            context: Some(Arc::new(bash::bash_task_context())),
            ..Default::default()
        },
        LanguageInfo {
            name: "c",
            ..Default::default()
        },
        LanguageInfo {
            name: "cpp",
            ..Default::default()
        },
        LanguageInfo {
            name: "css",
            ..Default::default()
        },
        LanguageInfo {
            name: "diff",
            ..Default::default()
        },
        LanguageInfo {
            name: "go",
            context: Some(go_context_provider.clone()),
            ..Default::default()
        },
        LanguageInfo {
            name: "gomod",
            context: Some(go_context_provider.clone()),
            ..Default::default()
        },
        LanguageInfo {
            name: "gowork",
            context: Some(go_context_provider),
            ..Default::default()
        },
        LanguageInfo {
            name: "json",
            context: Some(json_context_provider.clone()),
            ..Default::default()
        },
        LanguageInfo {
            name: "jsonc",
            context: Some(json_context_provider),
            ..Default::default()
        },
        LanguageInfo {
            name: "markdown",
            ..Default::default()
        },
        LanguageInfo {
            name: "markdown-inline",
            ..Default::default()
        },
        LanguageInfo {
            name: "python",
            context: Some(python_context_provider),
            toolchain: Some(python_toolchain_provider),
            manifest_name: Some(SharedString::new_static("pyproject.toml").into()),
        },
        LanguageInfo {
            name: "rust",
            context: Some(rust_context_provider),
            manifest_name: Some(SharedString::new_static("Cargo.toml").into()),
            ..Default::default()
        },
        LanguageInfo {
            name: "tsx",
            context: Some(typescript_context.clone()),
            ..Default::default()
        },
        LanguageInfo {
            name: "typescript",
            context: Some(typescript_context.clone()),
            ..Default::default()
        },
        LanguageInfo {
            name: "javascript",
            context: Some(typescript_context),
            ..Default::default()
        },
        LanguageInfo {
            name: "jsdoc",
            ..Default::default()
        },
        LanguageInfo {
            name: "regex",
            ..Default::default()
        },
        LanguageInfo {
            name: "yaml",
            ..Default::default()
        },
        LanguageInfo {
            name: "gitcommit",
            ..Default::default()
        },
        LanguageInfo {
            name: "zed-keybind-context",
            ..Default::default()
        },
    ];

    for registration in built_in_languages {
        register_language(
            &languages,
            registration.name,
            registration.context,
            registration.toolchain,
            registration.manifest_name,
            cx,
        );
    }

    let mut subscription = languages.subscribe();
    let mut prev_language_settings = languages.language_settings();

    cx.spawn(async move |cx| {
        while subscription.next().await.is_some() {
            let language_settings = languages.language_settings();
            if language_settings != prev_language_settings {
                cx.update(|cx| {
                    SettingsStore::update_global(cx, |settings, cx| {
                        settings
                            .set_extension_settings(
                                settings::ExtensionsSettingsContent {
                                    all_languages: language_settings.clone(),
                                },
                                cx,
                            )
                            .log_err();
                    });
                });
                prev_language_settings = language_settings;
            }
        }
        anyhow::Ok(())
    })
    .detach();
    let manifest_providers: [Arc<dyn ManifestProvider>; 2] = [
        Arc::from(CargoManifestProvider),
        Arc::from(PyprojectTomlManifestProvider),
    ];
    for provider in manifest_providers {
        project::ManifestProvidersStore::global(cx).register(provider);
    }
}

#[derive(Default)]
struct LanguageInfo {
    name: &'static str,
    context: Option<Arc<dyn ContextProvider>>,
    toolchain: Option<Arc<dyn ToolchainLister>>,
    manifest_name: Option<ManifestName>,
}

fn register_language(
    languages: &LanguageRegistry,
    name: &'static str,
    context: Option<Arc<dyn ContextProvider>>,
    toolchain: Option<Arc<dyn ToolchainLister>>,
    manifest_name: Option<ManifestName>,
    _cx: &mut App,
) {
    let config = load_config(name);
    languages.register_language(
        config.name.clone(),
        config.grammar.clone(),
        config.matcher.clone(),
        config.hidden,
        manifest_name.clone(),
        Arc::new(move || {
            let config = config.clone();
            let context = context.clone();
            let toolchain = toolchain.clone();
            let manifest_name = manifest_name.clone();
            async move {
                Ok(LoadedLanguage {
                    config,
                    queries: grammars::load_queries(name),
                    context_provider: context,
                    toolchain_provider: toolchain,
                    manifest_name,
                })
            }
            .boxed()
        }),
    );
}

#[cfg(any(test, feature = "test-support"))]
pub fn language(name: &str, grammar: tree_sitter::Language) -> Arc<Language> {
    Arc::new(
        Language::new(grammars::load_config(name), Some(grammar))
            .with_queries(grammars::load_queries(name))
            .unwrap(),
    )
}

fn load_config(name: &str) -> LanguageConfig {
    let grammars_loaded = cfg!(any(feature = "load-grammars", test));
    grammars::load_config_for_feature(name, grammars_loaded)
}
