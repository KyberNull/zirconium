use anyhow::Context as _;
use async_trait::async_trait;
use collections::HashMap;
use futures::AsyncBufReadExt;
use futures::future::BoxFuture;
use gpui::{App, SharedString};
use language::{LanguageName, ManifestName, ManifestProvider, ManifestQuery};
use language::{Toolchain, ToolchainList, ToolchainLister, ToolchainMetadata};
use pet_core::Configuration;
use pet_core::os_environment::Environment;
use pet_core::python_environment::{PythonEnvironment, PythonEnvironmentKind};
use pet_virtualenv::is_virtualenv_dir;
use project::Fs;
use serde::{Deserialize, Serialize};
use settings::Settings;
use terminal::terminal_settings::TerminalSettings;

use std::cmp::{Ordering, Reverse};

use util::rel_path::RelPath;

use parking_lot::Mutex;
use std::{
    fmt::Write,
    path::{Path, PathBuf},
    sync::Arc,
};
use task::ShellKind;

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct PythonToolchainData {
    #[serde(flatten)]
    environment: PythonEnvironment,
    #[serde(skip_serializing_if = "Option::is_none")]
    activation_scripts: Option<HashMap<ShellKind, PathBuf>>,
}

pub(crate) struct PyprojectTomlManifestProvider;

impl ManifestProvider for PyprojectTomlManifestProvider {
    fn name(&self) -> ManifestName {
        SharedString::new_static("pyproject.toml").into()
    }

    fn search(
        &self,
        ManifestQuery {
            path,
            depth,
            delegate,
        }: ManifestQuery,
    ) -> Option<Arc<RelPath>> {
        const WORKSPACE_LOCKFILES: &[&str] =
            &["uv.lock", "poetry.lock", "pdm.lock", "Pipfile.lock"];

        let mut innermost_pyproject = None;
        let mut outermost_workspace_root = None;

        for path in path.ancestors().take(depth) {
            let pyproject_path = path.join(RelPath::from_unix_str("pyproject.toml").unwrap());
            if delegate.exists(&pyproject_path, Some(false)) {
                if innermost_pyproject.is_none() {
                    innermost_pyproject = Some(Arc::from(path));
                }

                let has_lockfile = WORKSPACE_LOCKFILES.iter().any(|lockfile| {
                    let lockfile_path = path.join(RelPath::from_unix_str(lockfile).unwrap());
                    delegate.exists(&lockfile_path, Some(false))
                });
                if has_lockfile {
                    outermost_workspace_root = Some(Arc::from(path));
                }
            }
        }

        outermost_workspace_root.or(innermost_pyproject)
    }
}

fn is_python_env_global(k: &PythonEnvironmentKind) -> bool {
    matches!(
        k,
        PythonEnvironmentKind::Homebrew
            | PythonEnvironmentKind::Pyenv
            | PythonEnvironmentKind::GlobalPaths
            | PythonEnvironmentKind::MacPythonOrg
            | PythonEnvironmentKind::MacCommandLineTools
            | PythonEnvironmentKind::LinuxGlobal
            | PythonEnvironmentKind::MacXCode
            | PythonEnvironmentKind::WindowsStore
            | PythonEnvironmentKind::WindowsRegistry
    )
}

fn python_env_kind_display(k: &PythonEnvironmentKind) -> &'static str {
    match k {
        PythonEnvironmentKind::Conda => "Conda",
        PythonEnvironmentKind::Pixi => "pixi",
        PythonEnvironmentKind::Homebrew => "Homebrew",
        PythonEnvironmentKind::Pyenv => "global (Pyenv)",
        PythonEnvironmentKind::GlobalPaths => "global",
        PythonEnvironmentKind::PyenvVirtualEnv => "Pyenv",
        PythonEnvironmentKind::Pipenv => "Pipenv",
        PythonEnvironmentKind::Poetry => "Poetry",
        PythonEnvironmentKind::Hatch => "Hatch",
        PythonEnvironmentKind::MacPythonOrg => "global (Python.org)",
        PythonEnvironmentKind::MacCommandLineTools => "global (Command Line Tools for Xcode)",
        PythonEnvironmentKind::LinuxGlobal => "global",
        PythonEnvironmentKind::MacXCode => "global (Xcode)",
        PythonEnvironmentKind::Venv => "venv",
        PythonEnvironmentKind::VirtualEnv => "virtualenv",
        PythonEnvironmentKind::VirtualEnvWrapper => "virtualenvwrapper",
        PythonEnvironmentKind::WinPython => "WinPython",
        PythonEnvironmentKind::WindowsStore => "global (Windows Store)",
        PythonEnvironmentKind::WindowsRegistry => "global (Windows Registry)",
        PythonEnvironmentKind::Uv => "uv",
        PythonEnvironmentKind::UvWorkspace => "uv (Workspace)",
    }
}

pub(crate) struct PythonToolchainProvider {
    fs: Arc<dyn Fs>,
}

impl PythonToolchainProvider {
    pub fn new(fs: Arc<dyn Fs>) -> Self {
        Self { fs }
    }
}

static ENV_PRIORITY_LIST: &[PythonEnvironmentKind] = &[
    // Prioritize non-Conda environments.
    PythonEnvironmentKind::UvWorkspace,
    PythonEnvironmentKind::Uv,
    PythonEnvironmentKind::Poetry,
    PythonEnvironmentKind::Pipenv,
    PythonEnvironmentKind::VirtualEnvWrapper,
    PythonEnvironmentKind::Venv,
    PythonEnvironmentKind::VirtualEnv,
    PythonEnvironmentKind::PyenvVirtualEnv,
    PythonEnvironmentKind::Pixi,
    PythonEnvironmentKind::Conda,
    PythonEnvironmentKind::Pyenv,
    PythonEnvironmentKind::GlobalPaths,
    PythonEnvironmentKind::Homebrew,
];

fn env_priority(kind: Option<PythonEnvironmentKind>) -> usize {
    if let Some(kind) = kind {
        ENV_PRIORITY_LIST
            .iter()
            .position(|blessed_env| blessed_env == &kind)
            .unwrap_or(ENV_PRIORITY_LIST.len())
    } else {
        // Unknown toolchains are less useful than non-blessed ones.
        ENV_PRIORITY_LIST.len() + 1
    }
}

/// Return the name of environment declared in <worktree-root/.venv.
///
/// https://virtualfish.readthedocs.io/en/latest/plugins.html#auto-activation-auto-activation
async fn get_worktree_venv_declaration(worktree_root: &Path) -> Option<String> {
    let file = async_fs::File::open(worktree_root.join(".venv"))
        .await
        .ok()?;
    let mut venv_name = String::new();
    smol::io::BufReader::new(file)
        .read_line(&mut venv_name)
        .await
        .ok()?;
    Some(venv_name.trim().to_string())
}

fn get_venv_parent_dir(env: &PythonEnvironment) -> Option<PathBuf> {
    // If global, we aren't a virtual environment
    if let Some(kind) = env.kind
        && is_python_env_global(&kind)
    {
        return None;
    }

    // Check to be sure we are a virtual environment using pet's most generic
    // virtual environment type, VirtualEnv
    let venv = env
        .executable
        .as_ref()
        .and_then(|p| p.parent())
        .and_then(|p| p.parent())
        .filter(|p| is_virtualenv_dir(p))?;

    venv.parent().map(|parent| parent.to_path_buf())
}

// How far is this venv from the root of our current project?
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum SubprojectDistance {
    WithinSubproject(Reverse<usize>),
    WithinWorktree(Reverse<usize>),
    NotInWorktree,
}

fn wr_distance(
    wr: &PathBuf,
    subroot_relative_path: &RelPath,
    venv: Option<&PathBuf>,
) -> SubprojectDistance {
    if let Some(venv) = venv
        && let Ok(p) = venv.strip_prefix(wr)
    {
        if subroot_relative_path.components().next().is_some()
            && let Ok(distance) = p
                .strip_prefix(subroot_relative_path.as_std_path())
                .map(|p| p.components().count())
        {
            SubprojectDistance::WithinSubproject(Reverse(distance))
        } else {
            SubprojectDistance::WithinWorktree(Reverse(p.components().count()))
        }
    } else {
        SubprojectDistance::NotInWorktree
    }
}

fn micromamba_shell_name(kind: ShellKind) -> &'static str {
    match kind {
        ShellKind::Csh => "csh",
        ShellKind::Fish => "fish",
        ShellKind::Nushell => "nu",
        ShellKind::PowerShell | ShellKind::Pwsh => "powershell",
        ShellKind::Cmd => "cmd.exe",
        // default / catch-all:
        _ => "posix",
    }
}

#[async_trait]
impl ToolchainLister for PythonToolchainProvider {
    async fn list(
        &self,
        worktree_root: PathBuf,
        subroot_relative_path: Arc<RelPath>,
        project_env: Option<HashMap<String, String>>,
    ) -> ToolchainList {
        let fs = &*self.fs;
        let env = project_env.unwrap_or_default();
        let environment = EnvironmentApi::from_env(&env);
        let locators = pet::locators::create_locators(
            Arc::new(pet_conda::Conda::from(&environment)),
            Arc::new(pet_poetry::Poetry::from(&environment)),
            &environment,
        );
        let mut config = Configuration::default();

        // `.ancestors()` will yield at least one path, so in case of empty `subroot_relative_path`, we'll just use
        // worktree root as the workspace directory.
        config.workspace_directories = Some(
            subroot_relative_path
                .ancestors()
                .map(|ancestor| {
                    // remove trailing separator as it alters the environment name hash used by Poetry.
                    let path = worktree_root.join(ancestor.as_std_path());
                    let path_str = path.to_string_lossy();
                    if path_str.ends_with(std::path::MAIN_SEPARATOR) && path_str.len() > 1 {
                        PathBuf::from(path_str.trim_end_matches(std::path::MAIN_SEPARATOR))
                    } else {
                        path
                    }
                })
                .collect(),
        );
        for locator in locators.iter() {
            locator.configure(&config);
        }

        let reporter = pet_reporter::collect::create_reporter();
        pet::find::find_and_report_envs(&reporter, config, &locators, &environment, None, None);

        let mut toolchains = reporter
            .environments
            .lock()
            .map_or(Vec::new(), |mut guard| std::mem::take(&mut guard));

        let wr = worktree_root;
        let wr_venv = get_worktree_venv_declaration(&wr).await;
        // Sort detected environments by:
        //     environment name matching activation file (<workdir>/.venv)
        //     environment project dir matching worktree_root
        //     general env priority
        //     environment path matching the CONDA_PREFIX env var
        //     executable path
        toolchains.sort_by(|lhs, rhs| {
            // Compare venv names against worktree .venv file
            let venv_ordering =
                wr_venv
                    .as_ref()
                    .map_or(Ordering::Equal, |venv| match (&lhs.name, &rhs.name) {
                        (Some(l), Some(r)) => (r == venv).cmp(&(l == venv)),
                        (Some(l), None) if l == venv => Ordering::Less,
                        (None, Some(r)) if r == venv => Ordering::Greater,
                        _ => Ordering::Equal,
                    });

            // Compare project paths against worktree root
            let proj_ordering =
                || {
                    let lhs_project = lhs.project.clone().or_else(|| get_venv_parent_dir(lhs));
                    let rhs_project = rhs.project.clone().or_else(|| get_venv_parent_dir(rhs));
                    wr_distance(&wr, &subroot_relative_path, lhs_project.as_ref()).cmp(
                        &wr_distance(&wr, &subroot_relative_path, rhs_project.as_ref()),
                    )
                };

            // Compare environment priorities
            let priority_ordering = || env_priority(lhs.kind).cmp(&env_priority(rhs.kind));

            // Compare conda prefixes
            let conda_ordering = || {
                if lhs.kind == Some(PythonEnvironmentKind::Conda) {
                    environment
                        .get_env_var("CONDA_PREFIX".to_string())
                        .map(|conda_prefix| {
                            let is_match = |exe: &Option<PathBuf>| {
                                exe.as_ref().is_some_and(|e| e.starts_with(&conda_prefix))
                            };
                            match (is_match(&lhs.executable), is_match(&rhs.executable)) {
                                (true, false) => Ordering::Less,
                                (false, true) => Ordering::Greater,
                                _ => Ordering::Equal,
                            }
                        })
                        .unwrap_or(Ordering::Equal)
                } else {
                    Ordering::Equal
                }
            };

            // Compare Python executables
            let exe_ordering = || lhs.executable.cmp(&rhs.executable);

            venv_ordering
                .then_with(proj_ordering)
                .then_with(priority_ordering)
                .then_with(conda_ordering)
                .then_with(exe_ordering)
        });

        let mut out_toolchains = Vec::new();
        for toolchain in toolchains {
            let Some(toolchain) = venv_to_toolchain(toolchain, fs).await else {
                continue;
            };
            out_toolchains.push(toolchain);
        }
        out_toolchains.dedup();
        ToolchainList {
            toolchains: out_toolchains,
            default: None,
            groups: Default::default(),
        }
    }
    fn meta(&self) -> ToolchainMetadata {
        ToolchainMetadata {
            term: SharedString::new_static("Virtual Environment"),
            new_toolchain_placeholder: SharedString::new_static(
                "A path to the python3 executable within a virtual environment, or path to virtual environment itself",
            ),
            manifest_name: ManifestName::from(SharedString::new_static("pyproject.toml")),
        }
    }

    async fn resolve(
        &self,
        path: PathBuf,
        env: Option<HashMap<String, String>>,
    ) -> anyhow::Result<Toolchain> {
        let fs = &*self.fs;
        let env = env.unwrap_or_default();
        let environment = EnvironmentApi::from_env(&env);
        let locators = pet::locators::create_locators(
            Arc::new(pet_conda::Conda::from(&environment)),
            Arc::new(pet_poetry::Poetry::from(&environment)),
            &environment,
        );
        let toolchain = pet::resolve::resolve_environment(&path, &locators, &environment)
            .context("Could not find a virtual environment in provided path")?;
        let venv = toolchain.resolved.unwrap_or(toolchain.discovered);
        venv_to_toolchain(venv, fs)
            .await
            .context("Could not convert a venv into a toolchain")
    }

    fn activation_script(
        &self,
        toolchain: &Toolchain,
        shell: ShellKind,
        cx: &App,
    ) -> BoxFuture<'static, Vec<String>> {
        let settings = TerminalSettings::get_global(cx);
        let conda_manager = settings
            .detect_venv
            .as_option()
            .map(|venv| venv.conda_manager)
            .unwrap_or(settings::CondaManager::Auto);

        let toolchain_clone = toolchain.clone();
        Box::pin(async move {
            let Ok(toolchain) =
                serde_json::from_value::<PythonToolchainData>(toolchain_clone.as_json.clone())
            else {
                return vec![];
            };

            log::debug!("(Python) Composing activation script for toolchain {toolchain:?}");

            let mut activation_script = vec![];

            match toolchain.environment.kind {
                Some(PythonEnvironmentKind::Conda) => {
                    if toolchain.environment.manager.is_none() {
                        return vec![];
                    };

                    let manager = match conda_manager {
                        settings::CondaManager::Conda => "conda",
                        settings::CondaManager::Mamba => "mamba",
                        settings::CondaManager::Micromamba => "micromamba",
                        settings::CondaManager::Auto => toolchain
                            .environment
                            .manager
                            .as_ref()
                            .and_then(|m| m.executable.file_name())
                            .and_then(|name| name.to_str())
                            .filter(|name| matches!(*name, "conda" | "mamba" | "micromamba"))
                            .unwrap_or("conda"),
                    };

                    // Activate micromamba shell in the child shell
                    // [required for micromamba]
                    if manager == "micromamba" {
                        match shell {
                            ShellKind::PowerShell | ShellKind::Pwsh => {
                                activation_script.push(format!(r#"(& {manager} shell hook --shell powershell) | Out-String | Invoke-Expression"#));
                            }
                            _ => {
                                let shell_name = micromamba_shell_name(shell);
                                activation_script.push(format!(
                                    r#"eval "$({manager} shell hook --shell {shell_name})""#
                                ));
                            }
                        }
                    }

                    // Only inject `{manager} activate <name>` when we have a
                    // safely-quotable name. Never silently fall back to
                    // `activate base`: a user with miniforge installed but a
                    // local uv/venv project should not have their terminal
                    // hijacked just because we couldn't resolve a name.
                    if let Some(name) = &toolchain.environment.name {
                        if let Some(quoted_name) = shell.try_quote(name) {
                            activation_script.push(format!("{manager} activate {quoted_name}"));
                        } else {
                            log::warn!(
                                "Conda environment name {:?} could not be safely quoted; \
                                 skipping terminal activation",
                                name
                            );
                        }
                    } else {
                        log::warn!("Conda toolchain has no name; skipping terminal activation");
                    }
                }
                Some(
                    PythonEnvironmentKind::Venv
                    | PythonEnvironmentKind::VirtualEnv
                    | PythonEnvironmentKind::Uv
                    | PythonEnvironmentKind::UvWorkspace
                    | PythonEnvironmentKind::Poetry,
                ) => {
                    if let Some(activation_scripts) = &toolchain.activation_scripts {
                        if let Some(activate_script_path) = activation_scripts.get(&shell) {
                            let activate_keyword = shell.activate_keyword();
                            if let Some(quoted) =
                                shell.try_quote(&activate_script_path.to_string_lossy())
                            {
                                activation_script.push(format!("{activate_keyword} {quoted}"));
                            }
                        }
                    }
                }
                Some(PythonEnvironmentKind::Pyenv) => {
                    let Some(manager) = &toolchain.environment.manager else {
                        return vec![];
                    };
                    let version = toolchain.environment.version.as_deref().unwrap_or("system");
                    let pyenv = &manager.executable;
                    let pyenv = pyenv.display();
                    activation_script.extend(match shell {
                        ShellKind::Fish => Some(format!("\"{pyenv}\" shell - fish {version}")),
                        ShellKind::Posix => Some(format!("\"{pyenv}\" shell - sh {version}")),
                        ShellKind::Nushell => Some(format!("^\"{pyenv}\" shell - nu {version}")),
                        ShellKind::PowerShell | ShellKind::Pwsh => None,
                        ShellKind::Csh => None,
                        ShellKind::Tcsh => None,
                        ShellKind::Cmd => None,
                        ShellKind::Rc => None,
                        ShellKind::Xonsh => None,
                        ShellKind::Elvish => None,
                    })
                }
                _ => {}
            }
            activation_script
        })
    }
}

async fn venv_to_toolchain(venv: PythonEnvironment, fs: &dyn Fs) -> Option<Toolchain> {
    let mut name = String::from("Python");
    if let Some(ref version) = venv.version {
        _ = write!(name, " {version}");
    }

    let name_and_kind = match (&venv.name, &venv.kind) {
        (Some(name), Some(kind)) => Some(format!("({name}; {})", python_env_kind_display(kind))),
        (Some(name), None) => Some(format!("({name})")),
        (None, Some(kind)) => Some(format!("({})", python_env_kind_display(kind))),
        (None, None) => None,
    };

    if let Some(nk) = name_and_kind {
        _ = write!(name, " {nk}");
    }

    let mut activation_scripts = HashMap::default();
    match venv.kind {
        Some(
            PythonEnvironmentKind::Venv
            | PythonEnvironmentKind::VirtualEnv
            | PythonEnvironmentKind::Uv
            | PythonEnvironmentKind::UvWorkspace
            | PythonEnvironmentKind::Poetry,
        ) => resolve_venv_activation_scripts(&venv, fs, &mut activation_scripts).await,
        _ => {}
    }
    let data = PythonToolchainData {
        environment: venv,
        activation_scripts: Some(activation_scripts),
    };

    Some(Toolchain {
        name: name.into(),
        path: data
            .environment
            .executable
            .as_ref()?
            .to_str()?
            .to_owned()
            .into(),
        language_name: LanguageName::new_static("Python"),
        as_json: serde_json::to_value(data).ok()?,
    })
}

async fn resolve_venv_activation_scripts(
    venv: &PythonEnvironment,
    fs: &dyn Fs,
    activation_scripts: &mut HashMap<ShellKind, PathBuf>,
) {
    log::debug!("(Python) Resolving activation scripts for venv toolchain {venv:?}");
    if let Some(prefix) = &venv.prefix {
        for (shell_kind, script_name) in &[
            (ShellKind::Posix, "activate"),
            (ShellKind::Rc, "activate"),
            (ShellKind::Csh, "activate.csh"),
            (ShellKind::Tcsh, "activate.csh"),
            (ShellKind::Fish, "activate.fish"),
            (ShellKind::Nushell, "activate.nu"),
            (ShellKind::PowerShell, "activate.ps1"),
            (ShellKind::Pwsh, "activate.ps1"),
            (ShellKind::Cmd, "activate.bat"),
            (ShellKind::Xonsh, "activate.xsh"),
        ] {
            let path = prefix.join(BINARY_DIR).join(script_name);

            log::debug!("Trying path: {}", path.display());

            if fs.is_file(&path).await {
                activation_scripts.insert(*shell_kind, path);
            }
        }
    }
}

pub struct EnvironmentApi<'a> {
    global_search_locations: Arc<Mutex<Vec<PathBuf>>>,
    project_env: &'a HashMap<String, String>,
    pet_env: pet_core::os_environment::EnvironmentApi,
}

impl<'a> EnvironmentApi<'a> {
    pub fn from_env(project_env: &'a HashMap<String, String>) -> Self {
        let paths = project_env
            .get("PATH")
            .map(|p| std::env::split_paths(p).collect())
            .unwrap_or_default();

        EnvironmentApi {
            global_search_locations: Arc::new(Mutex::new(paths)),
            project_env,
            pet_env: pet_core::os_environment::EnvironmentApi::new(),
        }
    }

    fn user_home(&self) -> Option<PathBuf> {
        self.project_env
            .get("HOME")
            .or_else(|| self.project_env.get("USERPROFILE"))
            .map(|home| pet_fs::path::norm_case(PathBuf::from(home)))
            .or_else(|| self.pet_env.get_user_home())
    }
}

impl pet_core::os_environment::Environment for EnvironmentApi<'_> {
    fn get_user_home(&self) -> Option<PathBuf> {
        self.user_home()
    }

    fn get_root(&self) -> Option<PathBuf> {
        None
    }

    fn get_env_var(&self, key: String) -> Option<String> {
        self.project_env
            .get(&key)
            .cloned()
            .or_else(|| self.pet_env.get_env_var(key))
    }

    fn get_know_global_search_locations(&self) -> Vec<PathBuf> {
        if self.global_search_locations.lock().is_empty() {
            let mut paths = std::env::split_paths(
                &self
                    .get_env_var("PATH".to_string())
                    .or_else(|| self.get_env_var("Path".to_string()))
                    .unwrap_or_default(),
            )
            .collect::<Vec<PathBuf>>();

            log::trace!("Env PATH: {:?}", paths);
            for p in self.pet_env.get_know_global_search_locations() {
                if !paths.contains(&p) {
                    paths.push(p);
                }
            }

            let mut paths = paths
                .into_iter()
                .filter(|p| p.exists())
                .collect::<Vec<PathBuf>>();

            self.global_search_locations.lock().append(&mut paths);
        }
        self.global_search_locations.lock().clone()
    }
}

const BINARY_DIR: &str = if cfg!(target_os = "windows") {
    "Scripts"
} else {
    "bin"
};

#[cfg(test)]
mod tests {
    use gpui::{AppContext as _, BorrowAppContext, Context, TestAppContext};
    use language::{AutoindentMode, Buffer};
    use settings::SettingsStore;
    use std::num::NonZeroU32;

    #[gpui::test]
    async fn test_conda_activation_script_injection(cx: &mut TestAppContext) {
        use language::{LanguageName, Toolchain, ToolchainLister};
        use settings::{CondaManager, VenvSettings};
        use task::ShellKind;

        use crate::python::PythonToolchainProvider;

        cx.executor().allow_parking();

        cx.update(|cx| {
            let test_settings = SettingsStore::test(cx);
            cx.set_global(test_settings);
            cx.update_global::<SettingsStore, _>(|store, cx| {
                store.update_user_settings(cx, |s| {
                    s.terminal
                        .get_or_insert_with(Default::default)
                        .project
                        .detect_venv = Some(VenvSettings::On {
                        activate_script: None,
                        venv_name: None,
                        directories: None,
                        conda_manager: Some(CondaManager::Conda),
                    });
                });
            });
        });

        let fs = project::FakeFs::new(cx.executor());
        let provider = PythonToolchainProvider::new(fs);
        let malicious_name = "foo; rm -rf /";

        let manager_executable = std::env::current_exe().unwrap();

        let data = serde_json::json!({
            "name": malicious_name,
            "kind": "Conda",
            "executable": "/tmp/conda/bin/python",
            "version": serde_json::Value::Null,
            "prefix": serde_json::Value::Null,
            "arch": serde_json::Value::Null,
            "displayName": serde_json::Value::Null,
            "project": serde_json::Value::Null,
            "symlinks": serde_json::Value::Null,
            "manager": {
                "executable": manager_executable,
                "version": serde_json::Value::Null,
                "tool": "Conda",
            },
        });

        let toolchain = Toolchain {
            name: "test".into(),
            path: "/tmp/conda".into(),
            language_name: LanguageName::new_static("Python"),
            as_json: data,
        };

        let script = cx
            .update(|cx| provider.activation_script(&toolchain, ShellKind::Posix, cx))
            .await;

        assert!(
            script
                .iter()
                .any(|s| s.contains("conda activate 'foo; rm -rf /'")),
            "Script should contain quoted malicious name, actual: {:?}",
            script
        );
    }

    #[gpui::test]
    async fn test_conda_activation_skips_when_name_missing(cx: &mut TestAppContext) {
        use language::{LanguageName, Toolchain, ToolchainLister};
        use settings::{CondaManager, VenvSettings};
        use task::ShellKind;

        use crate::python::PythonToolchainProvider;

        cx.executor().allow_parking();

        cx.update(|cx| {
            let test_settings = SettingsStore::test(cx);
            cx.set_global(test_settings);
            cx.update_global::<SettingsStore, _>(|store, cx| {
                store.update_user_settings(cx, |s| {
                    s.terminal
                        .get_or_insert_with(Default::default)
                        .project
                        .detect_venv = Some(VenvSettings::On {
                        activate_script: None,
                        venv_name: None,
                        directories: None,
                        conda_manager: Some(CondaManager::Conda),
                    });
                });
            });
        });

        let fs = project::FakeFs::new(cx.executor());
        let provider = PythonToolchainProvider::new(fs);
        let manager_executable = std::env::current_exe().unwrap();

        let data = serde_json::json!({
            "name": serde_json::Value::Null,
            "kind": "Conda",
            "executable": "/tmp/conda/bin/python",
            "version": serde_json::Value::Null,
            "prefix": serde_json::Value::Null,
            "arch": serde_json::Value::Null,
            "displayName": serde_json::Value::Null,
            "project": serde_json::Value::Null,
            "symlinks": serde_json::Value::Null,
            "manager": {
                "executable": manager_executable,
                "version": serde_json::Value::Null,
                "tool": "Conda",
            },
        });

        let toolchain = Toolchain {
            name: "test".into(),
            path: "/tmp/conda".into(),
            language_name: LanguageName::new_static("Python"),
            as_json: data,
        };

        let script = cx
            .update(|cx| provider.activation_script(&toolchain, ShellKind::Posix, cx))
            .await;

        assert!(
            script.is_empty(),
            "Nameless conda toolchains must not fall back to `conda activate base`, actual: {:?}",
            script
        );
    }

    #[gpui::test]
    async fn test_conda_activation_skips_unquotable_name(cx: &mut TestAppContext) {
        use language::{LanguageName, Toolchain, ToolchainLister};
        use settings::{CondaManager, VenvSettings};
        use task::ShellKind;

        use crate::python::PythonToolchainProvider;

        cx.executor().allow_parking();

        cx.update(|cx| {
            let test_settings = SettingsStore::test(cx);
            cx.set_global(test_settings);
            cx.update_global::<SettingsStore, _>(|store, cx| {
                store.update_user_settings(cx, |s| {
                    s.terminal
                        .get_or_insert_with(Default::default)
                        .project
                        .detect_venv = Some(VenvSettings::On {
                        activate_script: None,
                        venv_name: None,
                        directories: None,
                        conda_manager: Some(CondaManager::Conda),
                    });
                });
            });
        });

        let fs = project::FakeFs::new(cx.executor());
        let provider = PythonToolchainProvider::new(fs);
        // shlex::try_quote rejects strings containing a NUL byte, so this name
        // is guaranteed to fail the Posix quoting path.
        let unquotable_name = "foo\0bar";
        let manager_executable = std::env::current_exe().unwrap();

        let data = serde_json::json!({
            "name": unquotable_name,
            "kind": "Conda",
            "executable": "/tmp/conda/bin/python",
            "version": serde_json::Value::Null,
            "prefix": serde_json::Value::Null,
            "arch": serde_json::Value::Null,
            "displayName": serde_json::Value::Null,
            "project": serde_json::Value::Null,
            "symlinks": serde_json::Value::Null,
            "manager": {
                "executable": manager_executable,
                "version": serde_json::Value::Null,
                "tool": "Conda",
            },
        });

        let toolchain = Toolchain {
            name: "test".into(),
            path: "/tmp/conda".into(),
            language_name: LanguageName::new_static("Python"),
            as_json: data,
        };

        let script = cx
            .update(|cx| provider.activation_script(&toolchain, ShellKind::Posix, cx))
            .await;

        assert!(
            !script.iter().any(|s| s.contains("conda activate")),
            "Unquotable conda env names must not emit any `conda activate` line, actual: {:?}",
            script
        );
    }

    #[gpui::test]
    async fn test_python_autoindent(cx: &mut TestAppContext) {
        cx.executor().set_block_on_ticks(usize::MAX..=usize::MAX);
        let language = crate::language("python", tree_sitter_python::LANGUAGE.into());
        cx.update(|cx| {
            let test_settings = SettingsStore::test(cx);
            cx.set_global(test_settings);
            cx.update_global::<SettingsStore, _>(|store, cx| {
                store.update_user_settings(cx, |s| {
                    s.project.all_languages.defaults.tab_size = NonZeroU32::new(2);
                });
            });
        });

        cx.new(|cx| {
            let mut buffer = Buffer::local("", cx).with_language(language, cx);
            let append = |buffer: &mut Buffer, text: &str, cx: &mut Context<Buffer>| {
                let ix = buffer.len();
                buffer.edit([(ix..ix, text)], Some(AutoindentMode::EachLine), cx);
            };

            // indent after "def():"
            append(&mut buffer, "def a():\n", cx);
            assert_eq!(buffer.text(), "def a():\n  ");

            // preserve indent after blank line
            append(&mut buffer, "\n  ", cx);
            assert_eq!(buffer.text(), "def a():\n  \n  ");

            // indent after "if"
            append(&mut buffer, "if a:\n  ", cx);
            assert_eq!(buffer.text(), "def a():\n  \n  if a:\n    ");

            // preserve indent after statement
            append(&mut buffer, "b()\n", cx);
            assert_eq!(buffer.text(), "def a():\n  \n  if a:\n    b()\n    ");

            // preserve indent after statement
            append(&mut buffer, "else", cx);
            assert_eq!(buffer.text(), "def a():\n  \n  if a:\n    b()\n    else");

            // dedent "else""
            append(&mut buffer, ":", cx);
            assert_eq!(buffer.text(), "def a():\n  \n  if a:\n    b()\n  else:");

            // indent lines after else
            append(&mut buffer, "\n", cx);
            assert_eq!(
                buffer.text(),
                "def a():\n  \n  if a:\n    b()\n  else:\n    "
            );

            // indent after an open paren. the closing paren is not indented
            // because there is another token before it on the same line.
            append(&mut buffer, "foo(\n1)", cx);
            assert_eq!(
                buffer.text(),
                "def a():\n  \n  if a:\n    b()\n  else:\n    foo(\n      1)"
            );

            // dedent the closing paren if it is shifted to the beginning of the line
            let argument_ix = buffer.text().find('1').unwrap();
            buffer.edit(
                [(argument_ix..argument_ix + 1, "")],
                Some(AutoindentMode::EachLine),
                cx,
            );
            assert_eq!(
                buffer.text(),
                "def a():\n  \n  if a:\n    b()\n  else:\n    foo(\n    )"
            );

            // preserve indent after the close paren
            append(&mut buffer, "\n", cx);
            assert_eq!(
                buffer.text(),
                "def a():\n  \n  if a:\n    b()\n  else:\n    foo(\n    )\n    "
            );

            // manually outdent the last line
            let end_whitespace_ix = buffer.len() - 4;
            buffer.edit(
                [(end_whitespace_ix..buffer.len(), "")],
                Some(AutoindentMode::EachLine),
                cx,
            );
            assert_eq!(
                buffer.text(),
                "def a():\n  \n  if a:\n    b()\n  else:\n    foo(\n    )\n"
            );

            // preserve the newly reduced indentation on the next newline
            append(&mut buffer, "\n", cx);
            assert_eq!(
                buffer.text(),
                "def a():\n  \n  if a:\n    b()\n  else:\n    foo(\n    )\n\n"
            );

            // reset to a for loop statement
            let statement = "for i in range(10):\n  print(i)\n";
            buffer.edit([(0..buffer.len(), statement)], None, cx);

            // insert single line comment after each line
            let eol_ixs = statement
                .char_indices()
                .filter_map(|(ix, c)| if c == '\n' { Some(ix) } else { None })
                .collect::<Vec<usize>>();
            let editions = eol_ixs
                .iter()
                .enumerate()
                .map(|(i, &eol_ix)| (eol_ix..eol_ix, format!(" # comment {}", i + 1)))
                .collect::<Vec<(std::ops::Range<usize>, String)>>();
            buffer.edit(editions, Some(AutoindentMode::EachLine), cx);
            assert_eq!(
                buffer.text(),
                "for i in range(10): # comment 1\n  print(i) # comment 2\n"
            );

            // reset to a simple if statement
            buffer.edit([(0..buffer.len(), "if a:\n  b(\n  )")], None, cx);

            // dedent "else" on the line after a closing paren
            append(&mut buffer, "\n  else:\n", cx);
            assert_eq!(buffer.text(), "if a:\n  b(\n  )\nelse:\n  ");

            buffer
        });
    }

    mod pyproject_manifest_tests {
        use std::collections::HashSet;
        use std::sync::Arc;

        use language::{ManifestDelegate, ManifestProvider, ManifestQuery};
        use settings::WorktreeId;
        use util::rel_path::RelPath;

        use crate::python::PyprojectTomlManifestProvider;

        struct FakeManifestDelegate {
            existing_files: HashSet<&'static str>,
        }

        impl ManifestDelegate for FakeManifestDelegate {
            fn worktree_id(&self) -> WorktreeId {
                WorktreeId::from_usize(0)
            }

            fn exists(&self, path: &RelPath, _is_dir: Option<bool>) -> bool {
                self.existing_files.contains(path.as_unix_str())
            }
        }

        fn search(files: &[&'static str], query_path: &str) -> Option<Arc<RelPath>> {
            let delegate = Arc::new(FakeManifestDelegate {
                existing_files: files.iter().copied().collect(),
            });
            let provider = PyprojectTomlManifestProvider;
            provider.search(ManifestQuery {
                path: RelPath::from_unix_str(query_path).unwrap().into(),
                depth: 10,
                delegate,
            })
        }

        #[test]
        fn test_simple_project_no_lockfile() {
            let result = search(&["project/pyproject.toml"], "project/src/main.py");
            assert_eq!(result.as_deref(), RelPath::from_unix_str("project").ok());
        }

        #[test]
        fn test_uv_workspace_returns_root() {
            let result = search(
                &[
                    "pyproject.toml",
                    "uv.lock",
                    "packages/subproject/pyproject.toml",
                ],
                "packages/subproject/src/main.py",
            );
            assert_eq!(result.as_deref(), RelPath::from_unix_str("").ok());
        }

        #[test]
        fn test_poetry_workspace_returns_root() {
            let result = search(
                &["pyproject.toml", "poetry.lock", "libs/mylib/pyproject.toml"],
                "libs/mylib/src/main.py",
            );
            assert_eq!(result.as_deref(), RelPath::from_unix_str("").ok());
        }

        #[test]
        fn test_pdm_workspace_returns_root() {
            let result = search(
                &[
                    "pyproject.toml",
                    "pdm.lock",
                    "packages/mypackage/pyproject.toml",
                ],
                "packages/mypackage/src/main.py",
            );
            assert_eq!(result.as_deref(), RelPath::from_unix_str("").ok());
        }

        #[test]
        fn test_independent_subprojects_no_lockfile_at_root() {
            let result_a = search(
                &["project-a/pyproject.toml", "project-b/pyproject.toml"],
                "project-a/src/main.py",
            );
            assert_eq!(
                result_a.as_deref(),
                RelPath::from_unix_str("project-a").ok()
            );

            let result_b = search(
                &["project-a/pyproject.toml", "project-b/pyproject.toml"],
                "project-b/src/main.py",
            );
            assert_eq!(
                result_b.as_deref(),
                RelPath::from_unix_str("project-b").ok()
            );
        }

        #[test]
        fn test_no_pyproject_returns_none() {
            let result = search(&[], "src/main.py");
            assert_eq!(result, None);
        }

        #[test]
        fn test_subproject_with_own_lockfile_and_workspace_root() {
            // Both root and subproject have lockfiles; should return root (outermost)
            let result = search(
                &[
                    "pyproject.toml",
                    "uv.lock",
                    "packages/sub/pyproject.toml",
                    "packages/sub/uv.lock",
                ],
                "packages/sub/src/main.py",
            );
            assert_eq!(result.as_deref(), RelPath::from_unix_str("").ok());
        }

        #[test]
        fn test_depth_limits_search() {
            let delegate = Arc::new(FakeManifestDelegate {
                existing_files: ["pyproject.toml", "uv.lock", "deep/nested/pyproject.toml"]
                    .into_iter()
                    .collect(),
            });
            let provider = PyprojectTomlManifestProvider;
            // depth=3 from "deep/nested/src/main.py" searches:
            //   "deep/nested/src/main.py", "deep/nested/src", and "deep/nested"
            // It won't reach "deep" or root ""
            let result = provider.search(ManifestQuery {
                path: RelPath::from_unix_str("deep/nested/src/main.py")
                    .unwrap()
                    .into(),
                depth: 3,
                delegate,
            });
            assert_eq!(
                result.as_deref(),
                RelPath::from_unix_str("deep/nested").ok()
            );
        }
    }
}
