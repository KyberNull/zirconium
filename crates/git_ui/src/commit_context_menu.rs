use crate::commit_view::CommitView;
use git::Oid;
use gpui::{Action, ClipboardItem, Entity, FocusHandle, SharedString, WeakEntity, Window, actions};
use project::git_store::Repository;
use ui::{ContextMenu, ContextMenuEntry, prelude::*};
use workspace::Workspace;

actions!(
    git_graph,
    [
        /// Copies the SHA of the selected commit to the clipboard.
        CopyCommitSha,
        /// Copies a tag from the selected commit to the clipboard.
        CopyCommitTag,
        /// Opens the commit view for the selected commit.
        OpenCommitView,
    ]
);

const COMMIT_TAG_LIST_WIDTH_IN_REMS: Rems = rems(10.);

pub(crate) struct CommitContextMenuData {
    pub(crate) sha: Oid,
    pub(crate) tag_names: Vec<SharedString>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommitContextMenuSource {
    GitGraph,
    GitPanel,
}

pub(crate) fn commit_context_menu(
    commit: CommitContextMenuData,
    source: CommitContextMenuSource,
    ref_name: Option<SharedString>,
    focus_handle: FocusHandle,
    repository: Option<WeakEntity<Repository>>,
    workspace: WeakEntity<Workspace>,
    window: &mut Window,
    cx: &mut App,
) -> Entity<ContextMenu> {
    let sha = commit.sha;
    let sha_short = sha.display_short();
    let header = match &ref_name {
        Some(ref_name) => format!("Ref {ref_name}"),
        None => format!("Commit {sha_short}"),
    };

    ContextMenu::build(window, cx, move |context_menu, _, _| {
        context_menu
            .context(focus_handle)
            .header(header)
            .entry("View Diff", Some(OpenCommitView.boxed_clone()), {
                let repository = repository.clone();
                let workspace = workspace.clone();
                move |window, cx| {
                    let Some(repository) = repository.clone() else {
                        return;
                    };
                    CommitView::open(
                        sha.to_string(),
                        repository,
                        workspace.clone(),
                        None,
                        None,
                        window,
                        cx,
                    );
                }
            })
            .entry(
                "Copy SHA",
                Some(CopyCommitSha.boxed_clone()),
                move |_window, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(sha.to_string()));
                },
            )
            .when_some(ref_name.clone(), |menu, ref_name| {
                menu.entry("Copy Ref Name", None, move |_window, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(ref_name.to_string()));
                })
            })
            .when(ref_name.is_none(), |menu| {
                menu.map(|menu| {
                    let tag_names = commit.tag_names.clone();
                    let copy_tag_label = "Copy Tag";

                    match tag_names.as_slice() {
                        [] => menu.item(
                            ContextMenuEntry::new(copy_tag_label)
                                .action(CopyCommitTag.boxed_clone())
                                .disabled(true),
                        ),
                        [tag_name] => {
                            let tag_name = tag_name.clone();
                            let label = format!("{copy_tag_label}: {tag_name}");
                            menu.entry(
                                label,
                                Some(CopyCommitTag.boxed_clone()),
                                move |_window, cx| {
                                    cx.write_to_clipboard(ClipboardItem::new_string(
                                        tag_name.to_string(),
                                    ));
                                },
                            )
                        }
                        _ => menu.submenu(copy_tag_label, move |menu, _window, _cx| {
                            let mut menu = menu.fixed_width(COMMIT_TAG_LIST_WIDTH_IN_REMS.into());

                            for tag_name in tag_names.clone() {
                                let tag_name_to_copy = tag_name.clone();
                                menu = menu.entry(tag_name, None, move |_window, cx| {
                                    cx.write_to_clipboard(ClipboardItem::new_string(
                                        tag_name_to_copy.to_string(),
                                    ));
                                });
                            }
                            menu
                        }),
                    }
                })
            })
            .when(source == CommitContextMenuSource::GitPanel, |menu| {
                menu.entry("Show in Git Graph", None, move |window, cx| {
                    window.dispatch_action(
                        Box::new(crate::git_graph::OpenAtCommit {
                            sha: sha.to_string(),
                        }),
                        cx,
                    );
                })
            })
    })
}
