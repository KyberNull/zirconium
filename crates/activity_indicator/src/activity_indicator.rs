use auto_update::DismissMessage;
use extension_host::{ExtensionOperation, ExtensionStore};
use futures::StreamExt;
use gpui::{
    App, Context, Entity, InteractiveElement as _, ParentElement as _, Render, Task, Window,
};
use project::{
    Project, ProjectEnvironmentEvent,
    git_store::{GitStoreEvent, Repository},
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use ui::{Tooltip, prelude::*};
use util::truncate_and_trailoff;
use workspace::{StatusItemView, Workspace, item::ItemHandle};

const GIT_OPERATION_DELAY: Duration = Duration::from_millis(0);
pub const DEFERRED_SCAN_MESSAGE_TIMEOUT: Duration = Duration::from_secs(10);

pub struct ActivityIndicator {
    project: Entity<Project>,
    fs_jobs: Vec<fs::JobInfo>,
    deferred_scan_message: DeferredScanMessage,
}

#[derive(Default)]
enum DeferredScanMessage {
    #[default]
    Undetected,
    Pending,
    Shown {
        _dismiss_timer: Task<()>,
    },
    Dismissed,
}

enum ActivityIcon {
    LoadingSpinner,
    Icon(IconName),
}

struct Content {
    icon: ActivityIcon,
    message: String,
    on_click:
        Option<Arc<dyn Fn(&mut ActivityIndicator, &mut Window, &mut Context<ActivityIndicator>)>>,
    tooltip_message: Option<String>,
}

impl ActivityIndicator {
    pub fn new(
        workspace: &mut Workspace,
        _window: &mut Window,
        cx: &mut Context<Workspace>,
    ) -> Entity<ActivityIndicator> {
        let project = workspace.project().clone();
        let this = cx.new(|cx| {
            let fs = project.read(cx).fs().clone();
            let mut job_events = fs.subscribe_to_jobs();
            cx.spawn(async move |this, cx| {
                while let Some(job_event) = job_events.next().await {
                    this.update(cx, |this: &mut ActivityIndicator, cx| {
                        match job_event {
                            fs::JobEvent::Started { info } => {
                                this.fs_jobs.retain(|j| j.id != info.id);
                                this.fs_jobs.push(info);
                            }
                            fs::JobEvent::Completed { id } => {
                                this.fs_jobs.retain(|j| j.id != id);
                            }
                        }
                        cx.notify();
                    })?;
                }
                anyhow::Ok(())
            })
            .detach();

            cx.subscribe(
                &project.read(cx).environment().clone(),
                |_, _, event, cx| match event {
                    ProjectEnvironmentEvent::ErrorsUpdated => cx.notify(),
                },
            )
            .detach();

            cx.subscribe(
                &project.read(cx).git_store().clone(),
                |_, _, event: &GitStoreEvent, cx| {
                    if let project::git_store::GitStoreEvent::JobsUpdated = event {
                        cx.notify()
                    }
                },
            )
            .detach();

            cx.subscribe(
                &project,
                |this, _, event: &project::Event, cx| match event {
                    project::Event::WorktreeAdded(_)
                    | project::Event::WorktreeRemoved(_)
                    | project::Event::WorktreeUpdatedEntries(..) => {
                        this.update_deferred_scan_status(cx);
                    }
                    _ => {}
                },
            )
            .detach();

            let mut this = Self {
                project: project.clone(),
                fs_jobs: Vec::new(),
                deferred_scan_message: DeferredScanMessage::default(),
            };
            this.update_deferred_scan_status(cx);
            this
        });

        this
    }

    fn dismiss_message(&mut self, _: &DismissMessage, _: &mut Window, cx: &mut Context<Self>) {
        self.project.update(cx, |project, cx| {
            if project.last_formatting_failure(cx).is_some() {
                project.reset_last_formatting_failure(cx);
                true
            } else {
                false
            }
        });
    }

    fn pending_environment_error<'a>(&'a self, cx: &'a App) -> Option<&'a String> {
        self.project.read(cx).peek_environment_error(cx)
    }

    fn update_deferred_scan_status(&mut self, cx: &mut Context<Self>) {
        let has_deferred_scan_dirs = self.project.read(cx).visible_worktrees(cx).any(|worktree| {
            let worktree = worktree.read(cx);
            worktree.deferred_scan_dir_count() > 0
                && worktree.as_local().is_none_or(|local_worktree| {
                    local_worktree.settings().file_scan_depth.is_some()
                })
        });
        match &self.deferred_scan_message {
            DeferredScanMessage::Undetected if has_deferred_scan_dirs => {
                self.deferred_scan_message = DeferredScanMessage::Pending;
                cx.notify();
            }
            DeferredScanMessage::Pending | DeferredScanMessage::Shown { .. }
                if !has_deferred_scan_dirs =>
            {
                self.deferred_scan_message = DeferredScanMessage::Undetected;
                cx.notify();
            }
            DeferredScanMessage::Dismissed if !has_deferred_scan_dirs => {
                self.deferred_scan_message = DeferredScanMessage::Undetected;
            }
            _ => {}
        }
    }

    #[cfg(feature = "test-support")]
    pub fn message_to_render(&mut self, cx: &mut Context<Self>) -> Option<String> {
        self.content_to_render(cx).map(|content| content.message)
    }

    fn content_to_render(&mut self, cx: &mut Context<Self>) -> Option<Content> {
        if let Some(content) = self.primary_content(cx) {
            return Some(content);
        }
        self.deferred_scan_content(cx)
    }

    fn primary_content(&mut self, cx: &mut Context<Self>) -> Option<Content> {
        // Show if any direnv calls failed
        if let Some(message) = self.pending_environment_error(cx) {
            return Some(Content {
                icon: ActivityIcon::Icon(IconName::Warning),
                message: message.clone(),
                on_click: Some(Arc::new(move |this, window, cx| {
                    this.project.update(cx, |project, cx| {
                        project.pop_environment_error(cx);
                    });
                    window.dispatch_action(Box::new(workspace::OpenLog), cx);
                })),
                tooltip_message: None,
            });
        }
        let current_job = self
            .project
            .read(cx)
            .active_repository(cx)
            .map(|r| r.read(cx))
            .and_then(Repository::current_job);
        // Show any long-running git command
        if let Some(job_info) = current_job
            && Instant::now() - job_info.start >= GIT_OPERATION_DELAY
        {
            return Some(Content {
                icon: ActivityIcon::LoadingSpinner,
                message: job_info.message.into(),
                on_click: None,
                tooltip_message: None,
            });
        }

        // Show any long-running fs command
        for fs_job in &self.fs_jobs {
            if Instant::now().duration_since(fs_job.start) >= GIT_OPERATION_DELAY {
                return Some(Content {
                    icon: ActivityIcon::LoadingSpinner,
                    message: fs_job.message.clone().into(),
                    on_click: None,
                    tooltip_message: None,
                });
            }
        }

        // Show any formatting failure
        if let Some(failure) = self.project.read(cx).last_formatting_failure(cx) {
            return Some(Content {
                icon: ActivityIcon::Icon(IconName::Warning),
                message: format!("Formatting failed: {failure}. Click to see logs."),
                on_click: Some(Arc::new(|indicator, window, cx| {
                    indicator.project.update(cx, |project, cx| {
                        project.reset_last_formatting_failure(cx);
                    });
                    window.dispatch_action(Box::new(workspace::OpenLog), cx);
                })),
                tooltip_message: None,
            });
        }

        // Show any extension installation info.
        if let Some(extension_store) =
            ExtensionStore::try_global(cx).map(|extension_store| extension_store.read(cx))
            && let Some((extension_id, operation)) =
                extension_store.outstanding_operations().iter().next()
        {
            let (message, icon) = match operation {
                ExtensionOperation::Install => (
                    format!("Installing {extension_id} extension…"),
                    ActivityIcon::LoadingSpinner,
                ),
                ExtensionOperation::Upgrade => (
                    format!("Updating {extension_id} extension…"),
                    ActivityIcon::Icon(IconName::Download),
                ),
                ExtensionOperation::Remove => (
                    format!("Removing {extension_id} extension…"),
                    ActivityIcon::LoadingSpinner,
                ),
            };

            return Some(Content {
                icon,
                message,
                on_click: Some(Arc::new(|this, window, cx| {
                    this.dismiss_message(&Default::default(), window, cx)
                })),
                tooltip_message: None,
            });
        }

        None
    }

    fn deferred_scan_content(&mut self, cx: &mut Context<Self>) -> Option<Content> {
        if !matches!(
            self.deferred_scan_message,
            DeferredScanMessage::Pending | DeferredScanMessage::Shown { .. }
        ) {
            return None;
        }
        if matches!(self.deferred_scan_message, DeferredScanMessage::Pending) {
            self.deferred_scan_message = DeferredScanMessage::Shown {
                _dismiss_timer: cx.spawn(async move |this, cx| {
                    cx.background_executor()
                        .timer(DEFERRED_SCAN_MESSAGE_TIMEOUT)
                        .await;
                    this.update(cx, |this, cx| {
                        this.deferred_scan_message = DeferredScanMessage::Dismissed;
                        cx.notify();
                    })
                    .ok();
                }),
            };
        }
        Some(Content {
            icon: ActivityIcon::Icon(IconName::Info),
            message: "Partial file index".to_string(),
            tooltip_message: Some("Directories outside of git repositories and deeper than the `file_scan_depth` setting will be indexed on demand.".to_string()),
            on_click: Some(Arc::new(|this, _, cx| {
                this.deferred_scan_message = DeferredScanMessage::Dismissed;
                cx.notify();
            })),
        })
    }
}

const MAX_MESSAGE_LEN: usize = 50;

impl Render for ActivityIndicator {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let result = h_flex()
            .id("activity-indicator")
            .on_action(cx.listener(Self::dismiss_message));

        let Some(content) = self.content_to_render(cx) else {
            return result;
        };

        let truncate_content = content.message.len() > MAX_MESSAGE_LEN;

        result.child(
            Button::new("activity-indicator-trigger", {
                if truncate_content {
                    truncate_and_trailoff(&content.message, MAX_MESSAGE_LEN)
                } else {
                    content.message.clone()
                }
            })
            .label_size(LabelSize::Small)
            .tab_index(0isize)
            .map(|this| match content.icon {
                ActivityIcon::LoadingSpinner => this.loading(true),
                ActivityIcon::Icon(icon_name) => this.start_icon(
                    Icon::new(icon_name)
                        .size(IconSize::Small)
                        .color(Color::Muted),
                ),
            })
            .map(|button| {
                if truncate_content {
                    button.tooltip(Tooltip::text(content.message))
                } else {
                    button.when_some(content.tooltip_message, |this, tooltip_message| {
                        this.tooltip(Tooltip::text(tooltip_message))
                    })
                }
            })
            .when_some(content.on_click, |this, handler| {
                this.on_click(cx.listener(move |this, _, window, cx| {
                    handler(this, window, cx);
                }))
            }),
        )
    }
}

impl StatusItemView for ActivityIndicator {
    fn set_active_pane_item(
        &mut self,
        _: Option<&dyn ItemHandle>,
        _window: &mut Window,
        _: &mut Context<Self>,
    ) {
    }

    fn hide_setting(&self, _: &App) -> Option<workspace::HideStatusItem> {
        // Activity indicator auto-hides when there's no work to display.
        None
    }
}
