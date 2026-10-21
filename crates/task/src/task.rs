//! Types describing a command to spawn in a terminal.

use anyhow::Context as _;
use collections::HashMap;
use gpui::SharedString;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub use util::shell::{Shell, ShellKind};
pub use util::shell_builder::ShellBuilder;
pub use zed_actions::RevealTarget;

/// Task identifier, unique within the application.
/// Based on it, task reruns and terminal tabs are managed.
#[derive(Default, Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize)]
pub struct TaskId(pub String);

/// Contains all information needed by Zed to spawn a new terminal tab for the given task.
#[derive(Default, Debug, Clone, PartialEq, Eq)]
pub struct SpawnInTerminal {
    /// Id of the task to use when determining task tab affinity.
    pub id: TaskId,
    /// Full unshortened form of `label` field.
    pub full_label: String,
    /// Human readable name of the terminal tab.
    pub label: String,
    /// Executable command to spawn.
    pub command: Option<String>,
    /// Arguments to the command, potentially unsubstituted,
    /// to let the shell that spawns the command to do the substitution, if needed.
    pub args: Vec<String>,
    /// A human-readable label, containing command and all of its arguments, joined and substituted.
    pub command_label: String,
    /// Current working directory to spawn the command into.
    pub cwd: Option<PathBuf>,
    /// Env overrides for the command, will be appended to the terminal's environment from the settings.
    pub env: HashMap<String, String>,
    /// Whether to use a new terminal tab or reuse the existing one to spawn the process.
    pub use_new_terminal: bool,
    /// Whether to allow multiple instances of the same task to be run, or rather wait for the existing ones to finish.
    pub allow_concurrent_runs: bool,
    /// What to do with the terminal pane and tab, after the command was started.
    pub reveal: RevealStrategy,
    /// Where to show tasks' terminal output.
    pub reveal_target: RevealTarget,
    /// What to do with the terminal pane and tab, after the command had finished.
    pub hide: HideStrategy,
    /// Which shell to use when spawning the task.
    pub shell: Shell,
    /// Whether to show the task summary line in the task output (success/failure).
    pub show_summary: bool,
    /// Whether to show the command line in the task output.
    pub show_command: bool,
    /// Which edited buffers to save before running the task.
    pub save: SaveStrategy,
}

impl SpawnInTerminal {
    pub fn to_proto(&self) -> proto::SpawnInTerminal {
        proto::SpawnInTerminal {
            label: self.label.clone(),
            command: self.command.clone(),
            args: self.args.clone(),
            env: self
                .env
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            cwd: self
                .cwd
                .clone()
                .map(|cwd| cwd.to_string_lossy().into_owned()),
        }
    }

    pub fn from_proto(proto: proto::SpawnInTerminal) -> Self {
        Self {
            label: proto.label.clone(),
            command: proto.command.clone(),
            args: proto.args.clone(),
            env: proto.env.into_iter().collect(),
            cwd: proto.cwd.map(PathBuf::from),
            ..Default::default()
        }
    }
}

/// What to do with the terminal pane and tab, after the command was started.
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RevealStrategy {
    /// Always show the task's pane, and focus the corresponding tab in it.
    #[default]
    Always,
    /// Always show the task's pane, add the task's tab in it, but don't focus it.
    NoFocus,
    /// Do not alter focus, but still add/reuse the task's tab in its pane.
    Never,
}

/// What to do with the terminal pane and tab, after the command has finished.
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HideStrategy {
    /// Do nothing when the command finishes.
    #[default]
    Never,
    /// Always hide the terminal tab, hide the pane also if it was the last tab in it.
    Always,
    /// Hide the terminal tab on task success only, otherwise behaves similar to `Always`.
    OnSuccess,
}

/// Which edited buffers to save before running a task.
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SaveStrategy {
    /// Save all edited buffers.
    All,
    /// Save the current buffer.
    Current,
    #[default]
    /// Don't save any buffers.
    None,
}

/// This is a new type representing a 'tag' on a 'runnable symbol', typically a test of main() function, found via treesitter.
#[derive(Clone, Debug)]
pub struct RunnableTag(pub SharedString);

pub fn shell_from_proto(proto: proto::Shell) -> anyhow::Result<Shell> {
    let shell_type = proto.shell_type.context("invalid shell type")?;
    let shell = match shell_type {
        proto::shell::ShellType::System(_) => Shell::System,
        proto::shell::ShellType::Program(program) => Shell::Program(program),
        proto::shell::ShellType::WithArguments(program) => Shell::WithArguments {
            program: program.program,
            args: program.args,
            title_override: None,
        },
    };
    Ok(shell)
}

pub fn shell_to_proto(shell: Shell) -> proto::Shell {
    let shell_type = match shell {
        Shell::System => proto::shell::ShellType::System(proto::System {}),
        Shell::Program(program) => proto::shell::ShellType::Program(program),
        Shell::WithArguments {
            program,
            args,
            title_override: _,
        } => proto::shell::ShellType::WithArguments(proto::shell::WithArguments { program, args }),
    };
    proto::Shell {
        shell_type: Some(shell_type),
    }
}
