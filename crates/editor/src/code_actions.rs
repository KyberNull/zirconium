use super::*;

impl Editor {
    /// Toggles an action selection menu for the latest selection.
    /// May show LSP code actions, code lens' command, runnables and potentially more entities applicable as actions.
    /// Previous menu toggled with this method will be closed.
    pub fn toggle_code_actions(
        &mut self,
        action: &ToggleCodeActions,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let quick_launch = action.quick_launch;
        let mut context_menu = self.context_menu.borrow_mut();
        if let Some(CodeContextMenu::CodeActions(code_actions)) = context_menu.as_ref() {
            if code_actions.deployed_from == action.deployed_from {
                // Toggle if we're selecting the same one
                *context_menu = None;
                cx.notify();
                return;
            } else {
                // Otherwise, clear it and start a new one
                *context_menu = None;
                cx.notify();
            }
        }
        drop(context_menu);
        let snapshot = self.snapshot(window, cx);
        let deployed_from = action.deployed_from.clone();
        let action = action.clone();
        self.completion_tasks.clear();
        self.discard_edit_prediction(EditPredictionDiscardReason::Ignored, cx);

        let multibuffer_point = match &action.deployed_from {
            Some(CodeActionSource::RunMenu(row)) => {
                DisplayPoint::new(*row, 0).to_point(&snapshot)
            }
            _ => self
                .selections
                .newest::<Point>(&snapshot.display_snapshot)
                .head(),
        };
        let Some((buffer, buffer_row)) = snapshot
            .buffer_snapshot()
            .buffer_line_for_row(MultiBufferRow(multibuffer_point.row))
            .and_then(|(buffer_snapshot, range)| {
                self.buffer()
                    .read(cx)
                    .buffer(buffer_snapshot.remote_id())
                    .map(|buffer| (buffer, range.start.row))
            })
        else {
            return;
        };
        let buffer_id = buffer.read(cx).remote_id();
        let tasks = self
            .runnables
            .runnables((buffer_id, buffer_row))
            .map(|t| Arc::new(t.to_owned()));

        let project = self.project.clone();
        let mut task_context_task = Task::ready(Ok(None));
        let workspace = self.workspace().map(|w| w.downgrade());
        if let Some(tasks) = &tasks
            && let Some(project) = project
        {
            task_context_task = Self::build_tasks_context(&project, &buffer, buffer_row, tasks, cx);
        }

        let runnable_task = cx.spawn_in(window, async move |_editor, cx| {
            let task_context = match workspace {
                Some(ws) => task_context_task
                    .await
                    .notify_workspace_async_err(ws, cx)
                    .flatten(),
                None => task_context_task.await.ok().flatten(),
            };

            let resolved_tasks =
                tasks
                    .zip(task_context.clone())
                    .map(|(tasks, task_context)| ResolvedTasks {
                        templates: tasks.resolve(&task_context).collect(),
                        position: snapshot.buffer_snapshot().anchor_before(Point::new(
                            multibuffer_point.row,
                            tasks.column,
                        )),
                    });
            anyhow::Ok(resolved_tasks)
        });

        let toggle_task = cx.spawn_in(window, async move |editor, cx| {
            let resolved_tasks = runnable_task.await?;

            editor.update_in(cx, |editor, window, cx| {
                let spawn_straight_away = quick_launch
                    && resolved_tasks
                        .as_ref()
                        .is_some_and(|tasks| tasks.templates.len() == 1);

                crate::hover_popover::hide_hover(editor, cx);
                let actions = CodeActionContents::new(resolved_tasks);

                // Don't show the menu if there are no actions available
                if actions.is_empty() {
                    cx.notify();
                    return Task::ready(Ok(()));
                }

                *editor.context_menu.borrow_mut() =
                    Some(CodeContextMenu::CodeActions(CodeActionsMenu {
                        buffer,
                        actions,
                        selected_item: Default::default(),
                        scroll_handle: UniformListScrollHandle::default(),
                        deployed_from,
                    }));
                cx.notify();
                if spawn_straight_away
                    && let Some(task) = editor.confirm_code_action(
                        &ConfirmCodeAction { item_ix: Some(0) },
                        window,
                        cx,
                    )
                {
                    return task;
                }

                Task::ready(Ok(()))
            })
        });
        self.runnables_for_selection_toggle = cx.background_spawn(async move {
            match toggle_task.await {
                Ok(code_action_spawn) => match code_action_spawn.await {
                    Ok(()) => {}
                    Err(e) => log::error!("failed to spawn a toggled code action: {e:#}"),
                },
                Err(e) => log::error!("failed to toggle code actions: {e:#}"),
            }
        })
    }

    pub fn confirm_code_action(
        &mut self,
        action: &ConfirmCodeAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Task<Result<()>>> {
        if self.read_only(cx) {
            return None;
        }

        let actions_menu =
            if let CodeContextMenu::CodeActions(menu) = self.hide_context_menu(window, cx)? {
                menu
            } else {
                return None;
            };

        let action_ix = action.item_ix.unwrap_or(actions_menu.selected_item);
        let action = actions_menu.actions.get(action_ix)?;
        let runnable_task_key =
            self.runnable_task_key_for_source(&actions_menu.deployed_from, window, cx);
        let workspace = self.workspace()?;

        match action {
            CodeActionsItem::Task(task_source_kind, resolved_task) => {
                if let Some((buffer_id, buffer_row)) = runnable_task_key {
                    self.set_runnable_task_status(
                        buffer_id,
                        buffer_row,
                        RunnableTaskStatus::Running,
                        cx,
                    );
                }
                let editor = cx.weak_entity();
                workspace.update(cx, |workspace, cx| {
                    if let Some((buffer_id, buffer_row)) = runnable_task_key {
                        workspace.schedule_resolved_task_with_completion(
                            task_source_kind,
                            resolved_task,
                            false,
                            move |result, cx| {
                                editor
                                    .update(cx, |editor, cx| {
                                        editor.set_runnable_task_status(
                                            buffer_id,
                                            buffer_row,
                                            RunnableTaskStatus::from(result),
                                            cx,
                                        );
                                    })
                                    .ok();
                            },
                            window,
                            cx,
                        );
                    } else {
                        workspace.schedule_resolved_task(
                            task_source_kind,
                            resolved_task,
                            false,
                            window,
                            cx,
                        );
                    }

                    Some(Task::ready(Ok(())))
                })
            }
        }
    }

    fn runnable_task_key_for_source(
        &self,
        source: &Option<CodeActionSource>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<(BufferId, BufferRow)> {
        let display_row = match source {
            Some(CodeActionSource::RunMenu(row)) => *row,
            _ => return None,
        };
        let snapshot = self.snapshot(window, cx);
        let multibuffer_row =
            MultiBufferRow(DisplayPoint::new(display_row, 0).to_point(&snapshot).row);
        let (buffer_snapshot, range) = snapshot
            .buffer_snapshot()
            .buffer_line_for_row(multibuffer_row)?;
        Some((buffer_snapshot.remote_id(), range.start.row))
    }

    pub fn context_menu(&self) -> &RefCell<Option<CodeContextMenu>> {
        &self.context_menu
    }

}
