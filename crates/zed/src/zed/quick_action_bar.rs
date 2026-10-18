mod preview;
mod repl_menu;

use agent_settings::AgentSettings;
use editor::actions::{
    AddSelectionAbove, AddSelectionBelow, DuplicateLineDown, GoToHunk, GoToPreviousHunk,
    MoveLineDown, MoveLineUp, SelectAll, SelectLargerSyntaxNode, SelectNext,
    SelectSmallerSyntaxNode, ToggleGoToLine,
};
use editor::{Editor, EditorSettings};
use gpui::{
    Action, Anchor, ClickEvent, Context, ElementId, Entity, EventEmitter,
    FocusHandle, Focusable, InteractiveElement, ParentElement, Render, Styled, Subscription,
    WeakEntity, Window,
};
use project::{DisableAiSettings, project_settings::ProjectSettings};
use search::{BufferSearchBar, buffer_search};
use settings::{GitDiffBaseSetting, Settings, SettingsStore, update_settings_file};
use ui::{
    ButtonStyle, ContextMenu, ContextMenuEntry, DocumentationSide, IconButton, IconName, IconSize,
    PopoverMenu, PopoverMenuHandle, Tooltip, prelude::*,
};
use vim_mode_setting::{HelixModeSetting, VimModeSetting};
use workspace::item::ItemBufferKind;
use workspace::{
    ToolbarItemEvent, ToolbarItemLocation, ToolbarItemView, Workspace, item::ItemHandle,
};
use zed_actions::{agent::AddSelectionToThread, assistant::InlineAssist, outline::ToggleOutline};


pub struct QuickActionBar {
    _ai_settings_subscription: Subscription,
    active_item: Option<Box<dyn ItemHandle>>,
    buffer_search_bar: Entity<BufferSearchBar>,
    show: bool,
    toggle_selections_handle: PopoverMenuHandle<ContextMenu>,
    toggle_settings_handle: PopoverMenuHandle<ContextMenu>,
    workspace: WeakEntity<Workspace>,
}

impl QuickActionBar {
    pub fn new(
        buffer_search_bar: Entity<BufferSearchBar>,
        workspace: &Workspace,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut was_agent_enabled = AgentSettings::get_global(cx).enabled(cx);
        let mut was_agent_button = AgentSettings::get_global(cx).button;

        let ai_settings_subscription = cx.observe_global::<SettingsStore>(move |_, cx| {
            let agent_settings = AgentSettings::get_global(cx);
            let is_agent_enabled = agent_settings.enabled(cx);

            if was_agent_enabled != is_agent_enabled || was_agent_button != agent_settings.button {
                was_agent_enabled = is_agent_enabled;
                was_agent_button = agent_settings.button;
                cx.notify();
            }
        });

        let mut this = Self {
            _ai_settings_subscription: ai_settings_subscription,
            active_item: None,
            buffer_search_bar,
            show: true,
            toggle_selections_handle: Default::default(),
            toggle_settings_handle: Default::default(),
            workspace: workspace.weak_handle(),
        };
        this.apply_settings(cx);
        cx.observe_global::<SettingsStore>(|this, cx| this.apply_settings(cx))
            .detach();
        this
    }

    fn active_editor(&self) -> Option<Entity<Editor>> {
        self.active_item
            .as_ref()
            .and_then(|item| item.downcast::<Editor>())
    }

    fn apply_settings(&mut self, cx: &mut Context<Self>) {
        let new_show = EditorSettings::get_global(cx).toolbar.quick_actions;
        if new_show != self.show {
            self.show = new_show;
            cx.emit(ToolbarItemEvent::ChangeLocation(
                self.get_toolbar_item_location(),
            ));
        }
    }

    fn get_toolbar_item_location(&self) -> ToolbarItemLocation {
        if self.show && self.active_editor().is_some() {
            ToolbarItemLocation::PrimaryRight
        } else {
            ToolbarItemLocation::Hidden
        }
    }
}

impl Render for QuickActionBar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(editor) = self.active_editor() else {
            return div().id("empty quick action bar");
        };

        let editor_value = editor.read(cx);
        let selection_menu_enabled = editor_value.selection_menu_enabled(cx);
        let git_blame_inline_enabled = editor_value.git_blame_inline_enabled();
        let show_git_blame_gutter = editor_value.show_git_blame_gutter();
        let show_line_numbers = editor_value.line_numbers_enabled(cx);
        let has_edit_prediction_provider = editor_value.edit_prediction_provider().is_some();
        let show_edit_predictions = editor_value.edit_predictions_enabled();
        let edit_predictions_enabled_at_cursor =
            editor_value.edit_predictions_enabled_at_cursor(cx);
        let supports_minimap = editor_value.supports_minimap(cx);
        let minimap_enabled = supports_minimap && editor_value.minimap().is_some();
        let focus_handle = editor_value.focus_handle(cx);

        let search_button = (editor.buffer_kind(cx) == ItemBufferKind::Singleton).then(|| {
            QuickActionBarButton::new(
                "toggle buffer search",
                search::SEARCH_ICON,
                !self.buffer_search_bar.read(cx).is_dismissed(),
                Box::new(buffer_search::Deploy::find()),
                focus_handle.clone(),
                "Buffer Search",
                {
                    let buffer_search_bar = self.buffer_search_bar.clone();
                    move |_, window, cx| {
                        buffer_search_bar.update(cx, |search_bar, cx| {
                            search_bar.toggle(&buffer_search::Deploy::find(), window, cx)
                        });
                    }
                },
            )
        });

        let assistant_button = QuickActionBarButton::new(
            "toggle inline assistant",
            IconName::ZedAssistant,
            false,
            Box::new(InlineAssist::default()),
            focus_handle,
            "Inline Assist",
            move |_, window, cx| {
                window.dispatch_action(Box::new(InlineAssist::default()), cx);
            },
        );

        let editor_selections_dropdown = selection_menu_enabled.then(|| {
            let has_diff_hunks = editor
                .read(cx)
                .buffer()
                .read(cx)
                .snapshot(cx)
                .has_diff_hunks();
            let has_selection = editor.update(cx, |editor, cx| {
                editor.has_non_empty_selection(&editor.display_snapshot(cx))
            });

            let focus = editor.focus_handle(cx);

            let disable_ai = DisableAiSettings::get_global(cx).disable_ai;

            PopoverMenu::new("editor-selections-dropdown")
                .trigger_with_tooltip(
                    IconButton::new("toggle_editor_selections_icon", IconName::CursorIBeam)
                        .icon_size(IconSize::Small)
                        .style(ButtonStyle::Subtle)
                        .toggle_state(self.toggle_selections_handle.is_deployed()),
                    Tooltip::text("Selection Controls"),
                )
                .with_handle(self.toggle_selections_handle.clone())
                .anchor(Anchor::TopRight)
                .menu(move |window, cx| {
                    let focus = focus.clone();
                    let menu = ContextMenu::build(window, cx, move |menu, _, _| {
                        menu.context(focus.clone())
                            .action("Select All", Box::new(SelectAll))
                            .action(
                                "Select Next Occurrence",
                                Box::new(SelectNext {
                                    replace_newest: false,
                                }),
                            )
                            .action("Expand Selection", Box::new(SelectLargerSyntaxNode))
                            .action("Shrink Selection", Box::new(SelectSmallerSyntaxNode))
                            .action(
                                "Add Cursor Above",
                                Box::new(AddSelectionAbove {
                                    skip_soft_wrap: true,
                                }),
                            )
                            .action(
                                "Add Cursor Below",
                                Box::new(AddSelectionBelow {
                                    skip_soft_wrap: true,
                                }),
                            )
                            .when(!disable_ai, |this| {
                                this.separator().action_disabled_when(
                                    !has_selection,
                                    "Add to Agent Thread",
                                    Box::new(AddSelectionToThread),
                                )
                            })
                            .separator()
                            .action("Go to Symbol", Box::new(ToggleOutline))
                            .action("Go to Line/Column", Box::new(ToggleGoToLine))
                            .separator()
                            .action_disabled_when(!has_diff_hunks, "Next Hunk", Box::new(GoToHunk))
                            .action_disabled_when(
                                !has_diff_hunks,
                                "Previous Hunk",
                                Box::new(GoToPreviousHunk),
                            )
                            .separator()
                            .action("Move Line Up", Box::new(MoveLineUp))
                            .action("Move Line Down", Box::new(MoveLineDown))
                            .action("Duplicate Selection", Box::new(DuplicateLineDown))
                    });
                    Some(menu)
                })
        });

        let editor_focus_handle = editor.focus_handle(cx);
        let editor = editor.downgrade();
        let editor_settings_dropdown = {
            let vim_mode_enabled = VimModeSetting::get_global(cx).0;
            let helix_mode_enabled = HelixModeSetting::get_global(cx).0;
            let diff_against_default_branch =
                ProjectSettings::get_global(cx).git.diff_base == GitDiffBaseSetting::DefaultBranch;
            let fs = self
                .workspace
                .upgrade()
                .map(|workspace| workspace.read(cx).app_state().fs.clone());

            PopoverMenu::new("editor-settings")
                .trigger_with_tooltip(
                    IconButton::new("toggle_editor_settings_icon", IconName::Filter)
                        .icon_size(IconSize::Small)
                        .toggle_state(self.toggle_settings_handle.is_deployed()),
                    Tooltip::text("Editor Controls"),
                )
                .anchor(Anchor::TopRight)
                .with_handle(self.toggle_settings_handle.clone())
                .menu(move |window, cx| {
                    let menu = ContextMenu::build(window, cx, {
                        let focus_handle = editor_focus_handle.clone();
                        |mut menu, _, _| {
                            menu = menu.context(focus_handle);

                            if supports_minimap {
                                menu = menu.toggleable_entry("Minimap", minimap_enabled, IconPosition::Start, Some(editor::actions::ToggleMinimap.boxed_clone()), {
                                    let editor = editor.clone();
                                    move |window, cx| {
                                        editor
                                            .update(cx, |editor, cx| {
                                                editor.toggle_minimap(
                                                    &editor::actions::ToggleMinimap,
                                                    window,
                                                    cx,
                                                );
                                            })
                                            .ok();
                                    }
                                },)
                            }

                            if has_edit_prediction_provider {
                                let mut edit_prediction_entry = ContextMenuEntry::new("Edit Predictions")
                                    .toggleable(IconPosition::Start, edit_predictions_enabled_at_cursor && show_edit_predictions)
                                    .disabled(!edit_predictions_enabled_at_cursor)
                                    .action(
                                        editor::actions::ToggleEditPrediction.boxed_clone(),
                                    ).handler({
                                        let editor = editor.clone();
                                        move |window, cx| {
                                            editor
                                                .update(cx, |editor, cx| {
                                                    editor.toggle_edit_predictions(
                                                        &editor::actions::ToggleEditPrediction,
                                                        window,
                                                        cx,
                                                    );
                                                })
                                                .ok();
                                        }
                                    });
                                if !edit_predictions_enabled_at_cursor {
                                    edit_prediction_entry = edit_prediction_entry.documentation_aside(DocumentationSide::Left, |_| {
                                        Label::new("You can't toggle edit predictions for this file as it is within the excluded files list.").into_any_element()
                                    });
                                }

                                menu = menu.item(edit_prediction_entry);
                            }

                            menu = menu.separator();

                            menu = menu.toggleable_entry(
                                "Line Numbers",
                                show_line_numbers,
                                IconPosition::Start,
                                Some(editor::actions::ToggleLineNumbers.boxed_clone()),
                                {
                                    let editor = editor.clone();
                                    move |window, cx| {
                                        editor
                                            .update(cx, |editor, cx| {
                                                editor.toggle_line_numbers(
                                                    &editor::actions::ToggleLineNumbers,
                                                    window,
                                                    cx,
                                                );
                                            })
                                            .ok();
                                    }
                                },
                            );

                            menu = menu.toggleable_entry(
                                "Selection Menu",
                                selection_menu_enabled,
                                IconPosition::Start,
                                Some(editor::actions::ToggleSelectionMenu.boxed_clone()),
                                {
                                    let editor = editor.clone();
                                    move |window, cx| {
                                        editor
                                            .update(cx, |editor, cx| {
                                                editor.toggle_selection_menu(
                                                    &editor::actions::ToggleSelectionMenu,
                                                    window,
                                                    cx,
                                                )
                                            })
                                            .ok();
                                    }
                                },
                            );

                            menu = menu.separator();

                            menu = menu.toggleable_entry(
                                "Inline Git Blame",
                                git_blame_inline_enabled,
                                IconPosition::Start,
                                Some(editor::actions::ToggleGitBlameInline.boxed_clone()),
                                {
                                    let editor = editor.clone();
                                    move |window, cx| {
                                        editor
                                            .update(cx, |editor, cx| {
                                                editor.toggle_git_blame_inline(
                                                    &editor::actions::ToggleGitBlameInline,
                                                    window,
                                                    cx,
                                                )
                                            })
                                            .ok();
                                    }
                                },
                            );

                            menu = menu.toggleable_entry(
                                "Column Git Blame",
                                show_git_blame_gutter,
                                IconPosition::Start,
                                Some(git::Blame.boxed_clone()),
                                {
                                    let editor = editor.clone();
                                    move |window, cx| {
                                        editor
                                            .update(cx, |editor, cx| {
                                                editor.toggle_git_blame(
                                                    &git::Blame,
                                                    window,
                                                    cx,
                                                )
                                            })
                                            .ok();
                                    }
                                },
                            );

                            if let Some(fs) = fs.clone() {
                                menu = menu.toggleable_entry(
                                    "Diff Against Default Branch",
                                    diff_against_default_branch,
                                    IconPosition::Start,
                                    None,
                                    {
                                        move |_window, cx| {
                                            let diff_base = if diff_against_default_branch {
                                                GitDiffBaseSetting::Head
                                            } else {
                                                GitDiffBaseSetting::DefaultBranch
                                            };
                                            update_settings_file(fs.clone(), cx, move |settings, _| {
                                                settings.git.get_or_insert_default().diff_base =
                                                    Some(diff_base);
                                            });
                                        }
                                    },
                                );
                            }

                            menu = menu.separator();

                            menu = menu.toggleable_entry(
                                "Vim Mode",
                                vim_mode_enabled,
                                IconPosition::Start,
                                None,
                                {
                                    move |window, cx| {
                                        let new_value = !vim_mode_enabled;
                                        VimModeSetting::override_global(VimModeSetting(new_value), cx);
                                        HelixModeSetting::override_global(HelixModeSetting(false), cx);
                                        window.refresh();
                                    }
                                },
                            );
                            menu = menu.toggleable_entry(
                                "Helix Mode",
                                helix_mode_enabled,
                                IconPosition::Start,
                                None,
                                {
                                    move |window, cx| {
                                        let new_value = !helix_mode_enabled;
                                        HelixModeSetting::override_global(HelixModeSetting(new_value), cx);
                                        VimModeSetting::override_global(VimModeSetting(false), cx);
                                        window.refresh();
                                    }
                                }
                            );

                            menu
                        }
                    });
                    Some(menu)
                })
        };

        h_flex()
            .id("quick action bar")
            .gap(DynamicSpacing::Base01.rems(cx))
            .children(self.render_repl_menu(cx))
            .children(self.render_preview_button(cx))
            .children(search_button)
            .when(
                AgentSettings::get_global(cx).enabled(cx) && AgentSettings::get_global(cx).button,
                |bar| bar.child(assistant_button),
            )
            .children(editor_selections_dropdown)
            .child(editor_settings_dropdown)
    }
}

impl EventEmitter<ToolbarItemEvent> for QuickActionBar {}

#[derive(IntoElement)]
struct QuickActionBarButton {
    id: ElementId,
    icon: IconName,
    toggled: bool,
    action: Box<dyn Action>,
    focus_handle: FocusHandle,
    tooltip: SharedString,
    on_click: Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>,
}

impl QuickActionBarButton {
    fn new(
        id: impl Into<ElementId>,
        icon: IconName,
        toggled: bool,
        action: Box<dyn Action>,
        focus_handle: FocusHandle,
        tooltip: impl Into<SharedString>,
        on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            id: id.into(),
            icon,
            toggled,
            action,
            focus_handle,
            tooltip: tooltip.into(),
            on_click: Box::new(on_click),
        }
    }
}

impl RenderOnce for QuickActionBarButton {
    fn render(self, _window: &mut Window, _: &mut App) -> impl IntoElement {
        let tooltip = self.tooltip.clone();
        let action = self.action.boxed_clone();

        IconButton::new(self.id.clone(), self.icon)
            .icon_size(IconSize::Small)
            .style(ButtonStyle::Subtle)
            .toggle_state(self.toggled)
            .tooltip(move |_window, cx| {
                Tooltip::for_action_in(tooltip.clone(), &*action, &self.focus_handle, cx)
            })
            .on_click(move |event, window, cx| (self.on_click)(event, window, cx))
    }
}

impl ToolbarItemView for QuickActionBar {
    fn set_active_pane_item(
        &mut self,
        active_pane_item: Option<&dyn ItemHandle>,
        _: &mut Window,
        _cx: &mut Context<Self>,
    ) -> ToolbarItemLocation {
        self.active_item = active_pane_item.map(ItemHandle::boxed_clone);
        self.get_toolbar_item_location()
    }
}
