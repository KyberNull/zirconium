use std::time::Duration;

use collections::HashMap;
use futures::TryFutureExt;
use gpui::{App, Entity, Task};
use gpui::{AsyncApp, FutureExt};
use language::Buffer;
use lsp::LanguageServerName;
use project::LanguageServerToQuery;
use project::LocationLink;
use project::Project;
use project::TaskSourceKind;
use project::lsp_store::lsp_ext_command::GetLspRunnables;
use task::ResolvedTask;
use task::TaskContext;
use text::BufferId;
use ui::SharedString;
use util::ResultExt as _;

async fn lsp_task_context(
    project: &Entity<Project>,
    buffer: &Entity<Buffer>,
    cx: &mut AsyncApp,
) -> Option<TaskContext> {
    let (worktree_store, environment) = project.read_with(cx, |project, _| {
        (project.worktree_store(), project.environment().clone())
    });

    let worktree_abs_path = cx.update(|cx| {
        let worktree_id = buffer.read(cx).file().map(|f| f.worktree_id(cx));

        worktree_id
            .and_then(|worktree_id| worktree_store.read(cx).worktree_for_id(worktree_id, cx))
            .and_then(|worktree| worktree.read(cx).root_dir())
    });

    let project_env = environment
        .update(cx, |environment, cx| {
            environment.buffer_environment(buffer, &worktree_store, cx)
        })
        .await;

    Some(TaskContext {
        cwd: worktree_abs_path.map(|p| p.to_path_buf()),
        project_env: project_env.unwrap_or_default(),
        ..TaskContext::default()
    })
}

pub fn lsp_tasks(
    project: Entity<Project>,
    task_sources: &HashMap<LanguageServerName, Vec<BufferId>>,
    for_position: Option<text::Anchor>,
    cx: &mut App,
) -> Task<Vec<(TaskSourceKind, Vec<(Option<LocationLink>, ResolvedTask)>)>> {
    let lsp_task_sources = task_sources
        .iter()
        .filter_map(|(name, buffer_ids)| {
            let buffers = buffer_ids
                .iter()
                .filter(|&&buffer_id| match for_position {
                    Some(for_position) => for_position.buffer_id == buffer_id,
                    None => true,
                })
                .filter_map(|&buffer_id| project.read(cx).buffer_for_id(buffer_id, cx))
                .collect::<Vec<_>>();

            let server_id = buffers.iter().find_map(|buffer| {
                project.read_with(cx, |project, cx| {
                    project.language_server_id_for_name(buffer.read(cx), name, cx)
                })
            });
            server_id.zip(Some(buffers))
        })
        .collect::<Vec<_>>();

    cx.spawn(async move |cx| {
        cx.spawn(async move |cx| {
            let mut lsp_tasks = HashMap::default();
            for (server_id, buffers) in lsp_task_sources {
                let mut new_lsp_tasks = Vec::new();
                for buffer in buffers {
                    let source_kind = match buffer.update(cx, |buffer, _| {
                        buffer.language().map(|language| language.name())
                    }) {
                        Some(language_name) => TaskSourceKind::Lsp {
                            server: server_id,
                            language_name: SharedString::from(language_name),
                        },
                        None => continue,
                    };
                    let id_base = source_kind.to_id_base();
                    let lsp_buffer_context = lsp_task_context(&project, &buffer, cx)
                        .await
                        .unwrap_or_default();

                    let runnables_task = project.update(cx, |project, cx| {
                        let buffer_id = buffer.read(cx).remote_id();
                        project.request_lsp(
                            buffer,
                            LanguageServerToQuery::Other(server_id),
                            GetLspRunnables {
                                buffer_id,
                                position: for_position,
                            },
                            cx,
                        )
                    });
                    if let Some(new_runnables) = runnables_task.await.log_err() {
                        new_lsp_tasks.extend(new_runnables.runnables.into_iter().filter_map(
                            |(location, runnable)| {
                                let resolved_task =
                                    runnable.resolve_task(&id_base, &lsp_buffer_context)?;
                                Some((location, resolved_task))
                            },
                        ));
                    }
                    if !new_lsp_tasks.is_empty() {
                        lsp_tasks
                            .entry(source_kind)
                            .or_insert_with(Vec::new)
                            .append(&mut new_lsp_tasks);
                    }
                }
            }
            lsp_tasks.into_iter().collect()
        })
        .with_timeout(Duration::from_millis(200), &cx.background_executor())
        .unwrap_or_else(|_| {
            log::debug!("Timed out waiting for LSP tasks");
            Vec::new()
        })
        .await
    })
}
