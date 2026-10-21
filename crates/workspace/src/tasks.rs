use std::process::ExitStatus;

use anyhow::Result;
use gpui::{Context, Task};
use task::SpawnInTerminal;
use ui::Window;

use crate::Workspace;

impl Workspace {
    pub fn spawn_in_terminal(
        self: &mut Workspace,
        spawn_in_terminal: SpawnInTerminal,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) -> Task<Option<Result<ExitStatus>>> {
        if let Some(terminal_provider) = self.terminal_provider.as_ref() {
            terminal_provider.spawn(spawn_in_terminal, window, cx)
        } else {
            Task::ready(None)
        }
    }
}
