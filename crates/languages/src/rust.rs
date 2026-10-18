use anyhow::Result;
use collections::HashMap;
use gpui::{App, AppContext, Entity, SharedString, Task};
pub use language::*;

use std::fmt::Display;
use std::{
    borrow::Cow,
    path::{Path, PathBuf},
    sync::Arc,
};
use task::{TaskTemplate, TaskTemplates, TaskVariables, VariableName};
use util::rel_path::RelPath;
use util::{ResultExt, maybe};

use crate::language_settings::LanguageSettings;

pub(crate) struct CargoManifestProvider;

impl ManifestProvider for CargoManifestProvider {
    fn name(&self) -> ManifestName {
        SharedString::new_static("Cargo.toml").into()
    }

    fn search(
        &self,
        ManifestQuery {
            path,
            depth,
            delegate,
        }: ManifestQuery,
    ) -> Option<Arc<RelPath>> {
        let mut outermost_cargo_toml = None;
        for path in path.ancestors().take(depth) {
            let p = path.join(RelPath::from_unix_str("Cargo.toml").unwrap());
            if delegate.exists(&p, Some(false)) {
                outermost_cargo_toml = Some(Arc::from(path));
            }
        }

        outermost_cargo_toml
    }
}

pub(crate) struct RustContextProvider;

const RUST_PACKAGE_TASK_VARIABLE: VariableName =
    VariableName::Custom(Cow::Borrowed("RUST_PACKAGE"));

/// The bin name corresponding to the current file in Cargo.toml
const RUST_BIN_NAME_TASK_VARIABLE: VariableName =
    VariableName::Custom(Cow::Borrowed("RUST_BIN_NAME"));

/// The bin kind (bin/example) corresponding to the current file in Cargo.toml
const RUST_BIN_KIND_TASK_VARIABLE: VariableName =
    VariableName::Custom(Cow::Borrowed("RUST_BIN_KIND"));

/// The flag to list required features for executing a bin, if any
const RUST_BIN_REQUIRED_FEATURES_FLAG_TASK_VARIABLE: VariableName =
    VariableName::Custom(Cow::Borrowed("RUST_BIN_REQUIRED_FEATURES_FLAG"));

/// The list of required features for executing a bin, if any
const RUST_BIN_REQUIRED_FEATURES_TASK_VARIABLE: VariableName =
    VariableName::Custom(Cow::Borrowed("RUST_BIN_REQUIRED_FEATURES"));

const RUST_TEST_FRAGMENT_TASK_VARIABLE: VariableName =
    VariableName::Custom(Cow::Borrowed("RUST_TEST_FRAGMENT"));

const RUST_DOC_TEST_NAME_TASK_VARIABLE: VariableName =
    VariableName::Custom(Cow::Borrowed("RUST_DOC_TEST_NAME"));

const RUST_TEST_NAME_TASK_VARIABLE: VariableName =
    VariableName::Custom(Cow::Borrowed("RUST_TEST_NAME"));

const RUST_MANIFEST_DIRNAME_TASK_VARIABLE: VariableName =
    VariableName::Custom(Cow::Borrowed("RUST_MANIFEST_DIRNAME"));

impl ContextProvider for RustContextProvider {
    fn build_context(
        &self,
        task_variables: &TaskVariables,
        location: ContextLocation<'_>,
        project_env: Option<HashMap<String, String>>,
        _: Arc<dyn LanguageToolchainStore>,
        cx: &mut gpui::App,
    ) -> Task<Result<TaskVariables>> {
        let local_abs_path = location
            .file_location
            .buffer
            .read(cx)
            .file()
            .and_then(|file| Some(file.as_local()?.abs_path(cx)));

        let mut variables = TaskVariables::default();

        if let (Some(path), Some(stem)) = (&local_abs_path, task_variables.get(&VariableName::Stem))
        {
            let fragment = test_fragment(&variables, path, stem);
            variables.insert(RUST_TEST_FRAGMENT_TASK_VARIABLE, fragment);
        };
        if let Some(test_name) =
            task_variables.get(&VariableName::Custom(Cow::Borrowed("_test_name")))
        {
            variables.insert(RUST_TEST_NAME_TASK_VARIABLE, test_name.into());
        }
        if let Some(doc_test_name) =
            task_variables.get(&VariableName::Custom(Cow::Borrowed("_doc_test_name")))
        {
            variables.insert(RUST_DOC_TEST_NAME_TASK_VARIABLE, doc_test_name.into());
        }
        cx.background_spawn(async move {
            if let Some(path) = local_abs_path
                .as_deref()
                .and_then(|local_abs_path| local_abs_path.parent())
                && let Some(package_name) =
                    human_readable_package_name(path, project_env.as_ref()).await
            {
                variables.insert(RUST_PACKAGE_TASK_VARIABLE.clone(), package_name);
            }
            if let Some(path) = local_abs_path.as_ref()
                && let Some((target, manifest_path)) =
                    target_info_from_abs_path(path, project_env.as_ref()).await?
            {
                if let Some(target) = target {
                    variables.extend(TaskVariables::from_iter([
                        (RUST_PACKAGE_TASK_VARIABLE.clone(), target.package_name),
                        (RUST_BIN_NAME_TASK_VARIABLE.clone(), target.target_name),
                        (
                            RUST_BIN_KIND_TASK_VARIABLE.clone(),
                            target.target_kind.to_string(),
                        ),
                    ]));
                    if target.required_features.is_empty() {
                        variables.insert(RUST_BIN_REQUIRED_FEATURES_FLAG_TASK_VARIABLE, "".into());
                        variables.insert(RUST_BIN_REQUIRED_FEATURES_TASK_VARIABLE, "".into());
                    } else {
                        variables.insert(
                            RUST_BIN_REQUIRED_FEATURES_FLAG_TASK_VARIABLE.clone(),
                            "--features".to_string(),
                        );
                        variables.insert(
                            RUST_BIN_REQUIRED_FEATURES_TASK_VARIABLE.clone(),
                            target.required_features.join(","),
                        );
                    }
                }
                variables.extend(TaskVariables::from_iter([(
                    RUST_MANIFEST_DIRNAME_TASK_VARIABLE.clone(),
                    manifest_path.to_string_lossy().into_owned(),
                )]));
            }
            Ok(variables)
        })
    }

    fn associated_tasks(
        &self,
        buffer: Option<Entity<Buffer>>,
        cx: &App,
    ) -> Task<Option<TaskTemplates>> {
        const DEFAULT_RUN_NAME_STR: &str = "RUST_DEFAULT_PACKAGE_RUN";
        const CUSTOM_TARGET_DIR: &str = "RUST_TARGET_DIR";

        let language = LanguageName::new_static("Rust");
        let settings = LanguageSettings::resolve(buffer.map(|b| b.read(cx)), Some(&language), cx);
        let package_to_run = settings.tasks.variables.get(DEFAULT_RUN_NAME_STR).cloned();
        let custom_target_dir = settings.tasks.variables.get(CUSTOM_TARGET_DIR).cloned();
        let run_task_args = if let Some(package_to_run) = package_to_run {
            vec!["run".into(), "-p".into(), package_to_run]
        } else {
            vec!["run".into()]
        };
        let mut task_templates = vec![
            TaskTemplate {
                label: format!(
                    "Check (package: {})",
                    RUST_PACKAGE_TASK_VARIABLE.template_value(),
                ),
                command: "cargo".into(),
                args: vec![
                    "check".into(),
                    "-p".into(),
                    RUST_PACKAGE_TASK_VARIABLE.template_value(),
                ],
                cwd: Some("$ZED_DIRNAME".to_owned()),
                ..TaskTemplate::default()
            },
            TaskTemplate {
                label: "Check all targets (workspace)".into(),
                command: "cargo".into(),
                args: vec!["check".into(), "--workspace".into(), "--all-targets".into()],
                cwd: Some("$ZED_DIRNAME".to_owned()),
                ..TaskTemplate::default()
            },
            TaskTemplate {
                label: format!(
                    "Test '{}' (package: {})",
                    RUST_TEST_NAME_TASK_VARIABLE.template_value(),
                    RUST_PACKAGE_TASK_VARIABLE.template_value(),
                ),
                command: "cargo".into(),
                args: vec![
                    "test".into(),
                    "-p".into(),
                    RUST_PACKAGE_TASK_VARIABLE.template_value(),
                    "--".into(),
                    "--nocapture".into(),
                    "--include-ignored".into(),
                    RUST_TEST_NAME_TASK_VARIABLE.template_value(),
                ],
                tags: vec!["rust-test".to_owned()],
                cwd: Some(RUST_MANIFEST_DIRNAME_TASK_VARIABLE.template_value()),
                ..TaskTemplate::default()
            },
            TaskTemplate {
                label: format!(
                    "Doc test '{}' (package: {})",
                    RUST_DOC_TEST_NAME_TASK_VARIABLE.template_value(),
                    RUST_PACKAGE_TASK_VARIABLE.template_value(),
                ),
                command: "cargo".into(),
                args: vec![
                    "test".into(),
                    "--doc".into(),
                    "-p".into(),
                    RUST_PACKAGE_TASK_VARIABLE.template_value(),
                    "--".into(),
                    "--nocapture".into(),
                    "--include-ignored".into(),
                    RUST_DOC_TEST_NAME_TASK_VARIABLE.template_value(),
                ],
                tags: vec!["rust-doc-test".to_owned()],
                cwd: Some(RUST_MANIFEST_DIRNAME_TASK_VARIABLE.template_value()),
                ..TaskTemplate::default()
            },
            TaskTemplate {
                label: format!(
                    "Test mod '{}' (package: {})",
                    VariableName::Stem.template_value(),
                    RUST_PACKAGE_TASK_VARIABLE.template_value(),
                ),
                command: "cargo".into(),
                args: vec![
                    "test".into(),
                    "-p".into(),
                    RUST_PACKAGE_TASK_VARIABLE.template_value(),
                    "--".into(),
                    RUST_TEST_FRAGMENT_TASK_VARIABLE.template_value(),
                ],
                tags: vec!["rust-mod-test".to_owned()],
                cwd: Some(RUST_MANIFEST_DIRNAME_TASK_VARIABLE.template_value()),
                ..TaskTemplate::default()
            },
            TaskTemplate {
                label: format!(
                    "Run {} {} (package: {})",
                    RUST_BIN_KIND_TASK_VARIABLE.template_value(),
                    RUST_BIN_NAME_TASK_VARIABLE.template_value(),
                    RUST_PACKAGE_TASK_VARIABLE.template_value(),
                ),
                command: "cargo".into(),
                args: vec![
                    "run".into(),
                    "-p".into(),
                    RUST_PACKAGE_TASK_VARIABLE.template_value(),
                    format!("--{}", RUST_BIN_KIND_TASK_VARIABLE.template_value()),
                    RUST_BIN_NAME_TASK_VARIABLE.template_value(),
                    RUST_BIN_REQUIRED_FEATURES_FLAG_TASK_VARIABLE.template_value(),
                    RUST_BIN_REQUIRED_FEATURES_TASK_VARIABLE.template_value(),
                ],
                cwd: Some(RUST_MANIFEST_DIRNAME_TASK_VARIABLE.template_value()),
                tags: vec!["rust-main".to_owned()],
                ..TaskTemplate::default()
            },
            TaskTemplate {
                label: format!(
                    "Test (package: {})",
                    RUST_PACKAGE_TASK_VARIABLE.template_value()
                ),
                command: "cargo".into(),
                args: vec![
                    "test".into(),
                    "-p".into(),
                    RUST_PACKAGE_TASK_VARIABLE.template_value(),
                ],
                cwd: Some(RUST_MANIFEST_DIRNAME_TASK_VARIABLE.template_value()),
                ..TaskTemplate::default()
            },
            TaskTemplate {
                label: "Run".into(),
                command: "cargo".into(),
                args: run_task_args,
                cwd: Some(RUST_MANIFEST_DIRNAME_TASK_VARIABLE.template_value()),
                ..TaskTemplate::default()
            },
            TaskTemplate {
                label: "Clean".into(),
                command: "cargo".into(),
                args: vec!["clean".into()],
                cwd: Some(RUST_MANIFEST_DIRNAME_TASK_VARIABLE.template_value()),
                ..TaskTemplate::default()
            },
        ];

        if let Some(custom_target_dir) = custom_target_dir {
            task_templates = task_templates
                .into_iter()
                .map(|mut task_template| {
                    let mut args = task_template.args.split_off(1);
                    task_template.args.append(&mut vec![
                        "--target-dir".to_string(),
                        custom_target_dir.clone(),
                    ]);
                    task_template.args.append(&mut args);

                    task_template
                })
                .collect();
        }

        Task::ready(Some(TaskTemplates(task_templates)))
    }
}

/// Part of the data structure of Cargo metadata
#[derive(Debug, serde::Deserialize)]
struct CargoMetadata {
    packages: Vec<CargoPackage>,
}

#[derive(Debug, serde::Deserialize)]
struct CargoPackage {
    id: String,
    targets: Vec<CargoTarget>,
    manifest_path: Arc<Path>,
}

#[derive(Debug, serde::Deserialize)]
struct CargoTarget {
    name: String,
    kind: Vec<String>,
    src_path: String,
    #[serde(rename = "required-features", default)]
    required_features: Vec<String>,
}

#[derive(Debug, PartialEq)]
enum TargetKind {
    Bin,
    Example,
}

impl Display for TargetKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TargetKind::Bin => write!(f, "bin"),
            TargetKind::Example => write!(f, "example"),
        }
    }
}

impl TryFrom<&str> for TargetKind {
    type Error = ();
    fn try_from(value: &str) -> Result<Self, ()> {
        match value {
            "bin" => Ok(Self::Bin),
            "example" => Ok(Self::Example),
            _ => Err(()),
        }
    }
}
/// Which package and binary target are we in?
#[derive(Debug, PartialEq)]
struct TargetInfo {
    package_name: String,
    target_name: String,
    target_kind: TargetKind,
    required_features: Vec<String>,
}

async fn target_info_from_abs_path(
    abs_path: &Path,
    project_env: Option<&HashMap<String, String>>,
) -> Result<Option<(Option<TargetInfo>, Arc<Path>)>> {
    let mut command = util::command::new_command("cargo");
    if let Some(envs) = project_env {
        command.envs(envs);
    }
    let output = command
        .current_dir(
            abs_path
                .parent()
                .ok_or_else(|| anyhow::anyhow!("failed to get parent directory"))?,
        )
        .arg("metadata")
        .arg("--no-deps")
        .arg("--format-version")
        .arg("1")
        .output()
        .await?;

    if !output.status.success() {
        let stderr_msg = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Cargo metadata failed\n {stderr_msg}");
    }

    let metadata: CargoMetadata = serde_json::from_slice(&output.stdout)?;
    Ok(target_info_from_metadata(metadata, abs_path))
}

fn target_info_from_metadata(
    metadata: CargoMetadata,
    abs_path: &Path,
) -> Option<(Option<TargetInfo>, Arc<Path>)> {
    let mut manifest_path = None;
    for package in metadata.packages {
        let Some(manifest_dir_path) = package.manifest_path.parent() else {
            continue;
        };

        let Some(path_from_manifest_dir) = abs_path.strip_prefix(manifest_dir_path).ok() else {
            continue;
        };
        let candidate_path_length = path_from_manifest_dir.components().count();
        // Pick the most specific manifest path
        if let Some((path, current_length)) = &mut manifest_path {
            if candidate_path_length > *current_length {
                *path = Arc::from(manifest_dir_path);
                *current_length = candidate_path_length;
            }
        } else {
            manifest_path = Some((Arc::from(manifest_dir_path), candidate_path_length));
        };

        for target in package.targets {
            let Some(bin_kind) = target
                .kind
                .iter()
                .find_map(|kind| TargetKind::try_from(kind.as_ref()).ok())
            else {
                continue;
            };
            let target_path = PathBuf::from(target.src_path);
            if target_path == abs_path {
                return manifest_path.map(|(path, _)| {
                    (
                        package_name_from_pkgid(&package.id).map(|package_name| TargetInfo {
                            package_name: package_name.to_owned(),
                            target_name: target.name,
                            required_features: target.required_features,
                            target_kind: bin_kind,
                        }),
                        path,
                    )
                });
            }
        }
    }

    manifest_path.map(|(path, _)| (None, path))
}

async fn human_readable_package_name(
    package_directory: &Path,
    project_env: Option<&HashMap<String, String>>,
) -> Option<String> {
    let mut command = util::command::new_command("cargo");
    if let Some(envs) = project_env {
        command.envs(envs);
    }
    let pkgid = String::from_utf8(
        command
            .current_dir(package_directory)
            .arg("pkgid")
            .output()
            .await
            .log_err()?
            .stdout,
    )
    .ok()?;
    Some(package_name_from_pkgid(&pkgid)?.to_owned())
}

// For providing local `cargo check -p $pkgid` task, we do not need most of the information we have returned.
// Output example in the root of Zed project:
// ```sh
// ❯ cargo pkgid zed
// path+file:///absolute/path/to/project/zed/crates/zed#0.131.0
// ```
// Another variant, if a project has a custom package name or hyphen in the name:
// ```
// path+file:///absolute/path/to/project/custom-package#my-custom-package@0.1.0
// ```
//
// Extracts the package name from the output according to the spec:
// https://doc.rust-lang.org/cargo/reference/pkgid-spec.html#specification-grammar
fn package_name_from_pkgid(pkgid: &str) -> Option<&str> {
    fn split_off_suffix(input: &str, suffix_start: char) -> &str {
        match input.rsplit_once(suffix_start) {
            Some((without_suffix, _)) => without_suffix,
            None => input,
        }
    }

    let (version_prefix, version_suffix) = pkgid.trim().rsplit_once('#')?;
    let package_name = match version_suffix.rsplit_once('@') {
        Some((custom_package_name, _version)) => custom_package_name,
        None => {
            let host_and_path = split_off_suffix(version_prefix, '?');
            let (_, package_name) = host_and_path.rsplit_once('/')?;
            package_name
        }
    };
    Some(package_name)
}

fn test_fragment(variables: &TaskVariables, path: &Path, stem: &str) -> String {
    let fragment = if stem == "lib" {
        // This isn't quite right---it runs the tests for the entire library, rather than
        // just for the top-level `mod tests`. But we don't really have the means here to
        // filter out just that module.
        Some("--lib".to_owned())
    } else if stem == "mod" {
        maybe!({ Some(path.parent()?.file_name()?.to_string_lossy().into_owned()) })
    } else if stem == "main" {
        if let (Some(bin_name), Some(bin_kind)) = (
            variables.get(&RUST_BIN_NAME_TASK_VARIABLE),
            variables.get(&RUST_BIN_KIND_TASK_VARIABLE),
        ) {
            Some(format!("--{bin_kind}={bin_name}"))
        } else {
            None
        }
    } else {
        Some(stem.to_owned())
    };
    fragment.unwrap_or_else(|| "--".to_owned())
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use super::*;
    
    use gpui::{BorrowAppContext, TestAppContext};
    
    use pretty_assertions::assert_eq;
    use settings::SettingsStore;
    
    

    #[gpui::test]
    async fn test_rust_autoindent(cx: &mut TestAppContext) {
        // cx.executor().set_block_on_ticks(usize::MAX..=usize::MAX);
        cx.update(|cx| {
            let test_settings = SettingsStore::test(cx);
            cx.set_global(test_settings);
            cx.update_global::<SettingsStore, _>(|store, cx| {
                store.update_user_settings(cx, |s| {
                    s.project.all_languages.defaults.tab_size = NonZeroU32::new(2);
                });
            });
        });

        let language = crate::language("rust", tree_sitter_rust::LANGUAGE.into());

        cx.new(|cx| {
            let mut buffer = Buffer::local("", cx).with_language(language, cx);

            // indent between braces
            buffer.set_text("fn a() {}", cx);
            let ix = buffer.len() - 1;
            buffer.edit([(ix..ix, "\n\n")], Some(AutoindentMode::EachLine), cx);
            assert_eq!(buffer.text(), "fn a() {\n  \n}");

            // indent between braces, even after empty lines
            buffer.set_text("fn a() {\n\n\n}", cx);
            let ix = buffer.len() - 2;
            buffer.edit([(ix..ix, "\n")], Some(AutoindentMode::EachLine), cx);
            assert_eq!(buffer.text(), "fn a() {\n\n\n  \n}");

            // indent a line that continues a field expression
            buffer.set_text("fn a() {\n  \n}", cx);
            let ix = buffer.len() - 2;
            buffer.edit([(ix..ix, "b\n.c")], Some(AutoindentMode::EachLine), cx);
            assert_eq!(buffer.text(), "fn a() {\n  b\n    .c\n}");

            // indent further lines that continue the field expression, even after empty lines
            let ix = buffer.len() - 2;
            buffer.edit([(ix..ix, "\n\n.d")], Some(AutoindentMode::EachLine), cx);
            assert_eq!(buffer.text(), "fn a() {\n  b\n    .c\n    \n    .d\n}");

            // dedent the line after the field expression
            let ix = buffer.len() - 2;
            buffer.edit([(ix..ix, ";\ne")], Some(AutoindentMode::EachLine), cx);
            assert_eq!(
                buffer.text(),
                "fn a() {\n  b\n    .c\n    \n    .d;\n  e\n}"
            );

            // indent inside a struct within a call
            buffer.set_text("const a: B = c(D {});", cx);
            let ix = buffer.len() - 3;
            buffer.edit([(ix..ix, "\n\n")], Some(AutoindentMode::EachLine), cx);
            assert_eq!(buffer.text(), "const a: B = c(D {\n  \n});");

            // indent further inside a nested call
            let ix = buffer.len() - 4;
            buffer.edit([(ix..ix, "e: f(\n\n)")], Some(AutoindentMode::EachLine), cx);
            assert_eq!(buffer.text(), "const a: B = c(D {\n  e: f(\n    \n  )\n});");

            // keep that indent after an empty line
            let ix = buffer.len() - 8;
            buffer.edit([(ix..ix, "\n")], Some(AutoindentMode::EachLine), cx);
            assert_eq!(
                buffer.text(),
                "const a: B = c(D {\n  e: f(\n    \n    \n  )\n});"
            );

            buffer
        });
    }

    #[test]
    fn test_package_name_from_pkgid() {
        for (input, expected) in [
            (
                "path+file:///absolute/path/to/project/zed/crates/zed#0.131.0",
                "zed",
            ),
            (
                "path+file:///absolute/path/to/project/custom-package#my-custom-package@0.1.0",
                "my-custom-package",
            ),
        ] {
            assert_eq!(package_name_from_pkgid(input), Some(expected));
        }
    }

    #[test]
    fn target_info_from_abs_path_failed() {
        let project_root = tempfile::tempdir().unwrap();
        let cargo_toml_path = project_root.path().join("Cargo.toml");
        let src_dir = project_root.path().join("src");
        let main_rs_path = src_dir.join("main.rs");

        std::fs::create_dir_all(&src_dir).unwrap();
        std::fs::write(&cargo_toml_path, "invalid_toml = {[[{").unwrap();
        std::fs::write(&main_rs_path, "// rust").unwrap();

        let e = smol::block_on(target_info_from_abs_path(&main_rs_path, None)).unwrap_err();
        assert!(e.to_string().contains("Cargo metadata failed"));
    }

    #[test]
    fn test_rust_test_fragment() {
        #[track_caller]
        fn check(
            variables: impl IntoIterator<Item = (VariableName, &'static str)>,
            path: &str,
            expected: &str,
        ) {
            let path = Path::new(path);
            let found = test_fragment(
                &TaskVariables::from_iter(variables.into_iter().map(|(k, v)| (k, v.to_owned()))),
                path,
                path.file_stem().unwrap().to_str().unwrap(),
            );
            assert_eq!(expected, found);
        }

        check([], "/project/src/lib.rs", "--lib");
        check([], "/project/src/foo/mod.rs", "foo");
        check(
            [
                (RUST_BIN_KIND_TASK_VARIABLE.clone(), "bin"),
                (RUST_BIN_NAME_TASK_VARIABLE, "x"),
            ],
            "/project/src/main.rs",
            "--bin=x",
        );
        check([], "/project/src/main.rs", "--");
    }

}
