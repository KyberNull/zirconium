use anyhow::Result;
use codestral::{self, CodestralEditPredictionDelegate};
use copilot::Status;
use edit_prediction::EditPredictionStore;
use edit_prediction_types::EditPredictionDelegateHandle;
use editor::{
    Editor, MultiBufferOffset, SelectionEffects, actions::ShowEditPrediction, scroll::Autoscroll,
};
use fs::Fs;
use gpui::{
    Action, Anchor, Animation, AnimationExt, App, AsyncWindowContext, Entity, FocusHandle,
    Focusable, IntoElement, ParentElement, Render, Subscription, TaskExt, WeakEntity, actions, div,
    pulsating_between,
};
use indoc::indoc;
use language::{
    EditPredictionsMode, File, Language,
    language_settings::{
        AllLanguageSettings, EditPredictionProvider, LanguageSettings, all_language_settings,
    },
};
use project::{DisableAiSettings, Project};
use regex::Regex;
use settings::{Settings, SettingsStore, update_settings_file};
use std::{
    rc::Rc,
    sync::{Arc, LazyLock},
    time::Duration,
};
use ui::{
    Clickable, ContextMenu, ContextMenuEntry, DocumentationSide, IconButton, IconButtonShape,
    Indicator, PopoverMenu, PopoverMenuHandle, Tooltip, prelude::*,
};
use util::ResultExt as _;

use workspace::{
    HideStatusItem, StatusItemView, Toast, Workspace, create_and_open_local_file, item::ItemHandle,
    notifications::NotificationId,
};
use zed_actions::{OpenBrowser, OpenSettingsAt};

actions!(
    edit_prediction,
    [
        /// Toggles the edit prediction menu.
        ToggleMenu
    ]
);

const COPILOT_SETTINGS_PATH: &str = "/settings/copilot";
const COPILOT_SETTINGS_URL: &str = concat!("https://github.com", "/settings/copilot");
const PRIVACY_DOCS: &str = "https://zed.dev/docs/ai/privacy-and-security";

struct CopilotErrorToast;

pub struct EditPredictionButton {
    editor_subscription: Option<(Subscription, usize)>,
    editor_enabled: Option<bool>,
    editor_show_predictions: bool,
    editor_focus_handle: Option<FocusHandle>,
    language: Option<Arc<Language>>,
    file: Option<Arc<dyn File>>,
    edit_prediction_provider: Option<Arc<dyn EditPredictionDelegateHandle>>,
    fs: Arc<dyn Fs>,
    popover_menu_handle: PopoverMenuHandle<ContextMenu>,
    project: WeakEntity<Project>,
}

impl Render for EditPredictionButton {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Return empty div if AI is disabled
        if DisableAiSettings::get_global(cx).disable_ai {
            return div().hidden();
        }

        let language_settings = all_language_settings(None, cx);

        match language_settings.edit_predictions.provider {
            EditPredictionProvider::Copilot => {
                let Some(copilot) = EditPredictionStore::try_global(cx)
                    .and_then(|store| store.read(cx).copilot_for_project(&self.project.upgrade()?))
                else {
                    return div().hidden();
                };
                let status = copilot.read(cx).status();

                let enabled = self.editor_enabled.unwrap_or(false);

                let icon = match status {
                    Status::Error(_) => IconName::CopilotError,
                    Status::Authorized => {
                        if enabled {
                            IconName::Copilot
                        } else {
                            IconName::CopilotDisabled
                        }
                    }
                    _ => IconName::CopilotInit,
                };

                if let Status::Error(e) = status {
                    return div().child(
                        IconButton::new("copilot-error", icon)
                            .icon_size(IconSize::Small)
                            .tab_index(0isize)
                            .aria_label("GitHub Copilot")
                            .on_click(cx.listener(move |_, _, window, cx| {
                                if let Some(workspace) = Workspace::for_window(window, cx) {
                                    workspace.update(cx, |workspace, cx| {
                                        let copilot = copilot.clone();
                                        workspace.show_toast(
                                            Toast::new(
                                                NotificationId::unique::<CopilotErrorToast>(),
                                                format!(
                                                    "Copilot Edit Predictions can't be started: {}",
                                                    e
                                                ),
                                            )
                                            .on_click(
                                                "Reinstall Copilot Edit Predictions",
                                                move |window, cx| {
                                                    copilot_ui::reinstall_and_sign_in(
                                                        copilot.clone(),
                                                        window,
                                                        cx,
                                                    )
                                                },
                                            ),
                                            cx,
                                        );
                                    });
                                }
                            }))
                            .tooltip(|_window, cx| {
                                Tooltip::for_action(
                                    "GitHub Copilot Edit Predictions",
                                    &ToggleMenu,
                                    cx,
                                )
                            }),
                    );
                }
                let this = cx.weak_entity();
                let project = self.project.clone();
                let file = self.file.clone();
                let language = self.language.clone();
                div().child(
                    PopoverMenu::new("copilot")
                        .on_open({
                            let file = file.clone();
                            let language = language;
                            let project = project.clone();
                            Rc::new(move |_window, cx| {
                                emit_edit_prediction_menu_opened(
                                    "copilot", &file, &language, &project, cx,
                                );
                            })
                        })
                        .menu(move |window, cx| {
                            let current_status = EditPredictionStore::try_global(cx)
                                .and_then(|store| {
                                    store.read(cx).copilot_for_project(&project.upgrade()?)
                                })?
                                .read(cx)
                                .status();
                            match current_status {
                                Status::Authorized => this.update(cx, |this, cx| {
                                    this.build_copilot_context_menu(window, cx)
                                }),
                                _ => this.update(cx, |this, cx| {
                                    this.build_copilot_start_menu(window, cx)
                                }),
                            }
                            .ok()
                        })
                        .anchor(Anchor::BottomRight)
                        .trigger_with_tooltip(
                            IconButton::new("copilot-icon", icon)
                                .tab_index(0isize)
                                .aria_label("GitHub Copilot"),
                            |_window, cx| {
                                Tooltip::for_action(
                                    "GitHub Copilot Edit Predictions",
                                    &ToggleMenu,
                                    cx,
                                )
                            },
                        )
                        .with_handle(self.popover_menu_handle.clone()),
                )
            }
            EditPredictionProvider::Codestral => {
                let enabled = self.editor_enabled.unwrap_or(true);
                let has_api_key = codestral::codestral_api_key(cx).is_some();
                let this = cx.weak_entity();
                let file = self.file.clone();
                let language = self.language.clone();
                let project = self.project.clone();

                let tooltip_meta = if has_api_key {
                    "Powered by Codestral"
                } else {
                    "Missing API key for Codestral"
                };

                div().child(
                    PopoverMenu::new("codestral")
                        .on_open({
                            let file = file.clone();
                            let language = language;
                            let project = project;
                            Rc::new(move |_window, cx| {
                                emit_edit_prediction_menu_opened(
                                    "codestral",
                                    &file,
                                    &language,
                                    &project,
                                    cx,
                                );
                            })
                        })
                        .menu(move |window, cx| {
                            this.update(cx, |this, cx| {
                                this.build_codestral_context_menu(window, cx)
                            })
                            .ok()
                        })
                        .anchor(Anchor::BottomRight)
                        .trigger_with_tooltip(
                            IconButton::new("codestral-icon", IconName::AiMistral)
                                .shape(IconButtonShape::Square)
                                .tab_index(0isize)
                                .aria_label("Edit Prediction")
                                .when(!has_api_key, |this| {
                                    this.indicator(Indicator::dot().color(Color::Error))
                                        .indicator_border_color(Some(
                                            cx.theme().colors().status_bar_background,
                                        ))
                                })
                                .when(has_api_key && !enabled, |this| {
                                    this.indicator(Indicator::dot().color(Color::Ignored))
                                        .indicator_border_color(Some(
                                            cx.theme().colors().status_bar_background,
                                        ))
                                }),
                            move |_window, cx| {
                                Tooltip::with_meta(
                                    "Edit Prediction",
                                    Some(&ToggleMenu),
                                    tooltip_meta,
                                    cx,
                                )
                            },
                        )
                        .with_handle(self.popover_menu_handle.clone()),
                )
            }
            EditPredictionProvider::OpenAiCompatibleApi => {
                let enabled = self.editor_enabled.unwrap_or(true);
                let this = cx.weak_entity();

                div().child(
                    PopoverMenu::new("openai-compatible-api")
                        .menu(move |window, cx| {
                            this.update(cx, |this, cx| {
                                this.build_edit_prediction_context_menu(
                                    EditPredictionProvider::OpenAiCompatibleApi,
                                    window,
                                    cx,
                                )
                            })
                            .ok()
                        })
                        .anchor(Anchor::BottomRight)
                        .trigger(
                            IconButton::new("openai-compatible-api-icon", IconName::AiOpenAiCompat)
                                .shape(IconButtonShape::Square)
                                .tab_index(0isize)
                                .aria_label("Edit Prediction")
                                .when(!enabled, |this| {
                                    this.indicator(Indicator::dot().color(Color::Ignored))
                                        .indicator_border_color(Some(
                                            cx.theme().colors().status_bar_background,
                                        ))
                                }),
                        )
                        .with_handle(self.popover_menu_handle.clone()),
                )
            }
            EditPredictionProvider::Ollama => {
                let enabled = self.editor_enabled.unwrap_or(true);
                let this = cx.weak_entity();

                div().child(
                    PopoverMenu::new("ollama")
                        .menu(move |window, cx| {
                            this.update(cx, |this, cx| {
                                this.build_edit_prediction_context_menu(
                                    EditPredictionProvider::Ollama,
                                    window,
                                    cx,
                                )
                            })
                            .ok()
                        })
                        .anchor(Anchor::BottomRight)
                        .trigger_with_tooltip(
                            IconButton::new("ollama-icon", IconName::AiOllama)
                                .shape(IconButtonShape::Square)
                                .tab_index(0isize)
                                .aria_label("Edit Prediction")
                                .when(!enabled, |this| {
                                    this.indicator(Indicator::dot().color(Color::Ignored))
                                        .indicator_border_color(Some(
                                            cx.theme().colors().status_bar_background,
                                        ))
                                }),
                            move |_window, cx| {
                                let settings = all_language_settings(None, cx);
                                let tooltip_meta = match settings.edit_predictions.ollama.as_ref() {
                                    Some(settings) if !settings.model.trim().is_empty() => {
                                        format!("Powered by Ollama ({})", settings.model)
                                    }
                                    _ => {
                                        "Ollama model not configured — configure a model before use"
                                            .to_string()
                                    }
                                };

                                Tooltip::with_meta(
                                    "Edit Prediction",
                                    Some(&ToggleMenu),
                                    tooltip_meta,
                                    cx,
                                )
                            },
                        )
                        .with_handle(self.popover_menu_handle.clone()),
                )
            }
            EditPredictionProvider::Mercury => {
                let enabled = self.editor_enabled.unwrap_or(true);
                let file = self.file.clone();
                let language = self.language.clone();
                let project = self.project.clone();
                let icons = self
                    .edit_prediction_provider
                    .as_ref()
                    .map(|p| p.icons(cx))
                    .unwrap_or_else(|| {
                        edit_prediction_types::EditPredictionIconSet::new(IconName::Inception)
                    });

                let ep_icon = if enabled { icons.base } else { icons.disabled };
                let mercury_has_error = edit_prediction::EditPredictionStore::try_global(cx)
                    .is_some_and(|ep_store| ep_store.read(cx).mercury_has_payment_required_error());
                let missing_token = edit_prediction::EditPredictionStore::try_global(cx)
                    .is_some_and(|ep_store| !ep_store.read(cx).has_mercury_api_token(cx));
                let tooltip_meta = if missing_token {
                    "Missing API key for Mercury"
                } else if mercury_has_error {
                    "Mercury free tier limit reached"
                } else {
                    "Powered by Mercury"
                };

                let show_editor_predictions = self.editor_show_predictions;

                let indicator_color = if missing_token || mercury_has_error {
                    Some(Color::Error)
                } else if enabled && !show_editor_predictions {
                    Some(Color::Muted)
                } else {
                    None
                };

                let provider_unavailable = missing_token || mercury_has_error;

                let icon_button = IconButton::new("edit-prediction-button", ep_icon)
                    .shape(IconButtonShape::Square)
                    .tab_index(0isize)
                    .aria_label("Edit Prediction")
                    .when_some(indicator_color, |this, color| {
                        this.indicator(Indicator::dot().color(color))
                            .indicator_border_color(Some(cx.theme().colors().status_bar_background))
                    })
                    .when(!self.popover_menu_handle.is_deployed(), |element| {
                        element.tooltip(move |_window, cx| {
                            let description = if !enabled {
                                "Disabled For This File"
                            } else if provider_unavailable || show_editor_predictions {
                                tooltip_meta
                            } else {
                                "Enable to Use"
                            };

                            Tooltip::with_meta(
                                "Edit Prediction",
                                Some(&ToggleMenu),
                                description,
                                cx,
                            )
                        })
                    });

                let this = cx.weak_entity();

                let mut popover_menu = PopoverMenu::new("edit-prediction")
                    .on_open({
                        let file = file.clone();
                        let language = language;
                        let project = project;
                        Rc::new(move |_window, cx| {
                            emit_edit_prediction_menu_opened(
                                "mercury", &file, &language, &project, cx,
                            );
                        })
                    })
                    .map(|popover_menu| {
                        let this = this.clone();
                        popover_menu.menu(move |window, cx| {
                            this.update(cx, |this, cx| {
                                this.build_edit_prediction_context_menu(
                                    EditPredictionProvider::Mercury,
                                    window,
                                    cx,
                                )
                            })
                            .ok()
                        })
                    })
                    .anchor(Anchor::BottomRight)
                    .with_handle(self.popover_menu_handle.clone());

                let is_refreshing = self
                    .edit_prediction_provider
                    .as_ref()
                    .is_some_and(|provider| provider.is_refreshing(cx));

                if is_refreshing {
                    popover_menu = popover_menu.trigger(
                        icon_button.with_animation(
                            "pulsating-label",
                            Animation::new(Duration::from_secs(2))
                                .repeat()
                                .with_easing(pulsating_between(0.2, 1.0)),
                            |icon_button, delta| icon_button.alpha(delta),
                        ),
                    );
                } else {
                    popover_menu = popover_menu.trigger(icon_button);
                }

                div().child(popover_menu.into_any_element())
            }

            EditPredictionProvider::None => div().hidden(),
        }
    }
}

impl EditPredictionButton {
    pub fn new(
        fs: Arc<dyn Fs>,
        popover_menu_handle: PopoverMenuHandle<ContextMenu>,
        project: Entity<Project>,
        cx: &mut Context<Self>,
    ) -> Self {
        let copilot = EditPredictionStore::try_global(cx).and_then(|store| {
            store.update(cx, |this, cx| this.start_copilot_for_project(&project, cx))
        });
        if let Some(copilot) = copilot {
            cx.observe(&copilot, |_, _, cx| cx.notify()).detach()
        }

        cx.observe_global::<SettingsStore>(move |_, cx| cx.notify())
            .detach();

        cx.observe_global::<EditPredictionStore>(move |_, cx| cx.notify())
            .detach();

        edit_prediction::ollama::ensure_authenticated(cx);
        let mercury_api_token_task = edit_prediction::mercury::load_mercury_api_token(cx);
        let open_ai_compatible_api_token_task =
            edit_prediction::open_ai_compatible::load_open_ai_compatible_api_token(cx);

        cx.spawn(async move |this, cx| {
            _ = futures::join!(mercury_api_token_task, open_ai_compatible_api_token_task);
            this.update(cx, |_, cx| {
                cx.notify();
            })
            .ok();
        })
        .detach();

        CodestralEditPredictionDelegate::ensure_api_key_loaded(cx);

        Self {
            editor_subscription: None,
            editor_enabled: None,
            editor_show_predictions: true,
            editor_focus_handle: None,
            language: None,
            file: None,
            edit_prediction_provider: None,
            popover_menu_handle,
            project: project.downgrade(),
            fs,
        }
    }

    fn add_provider_switching_section(
        &self,
        mut menu: ContextMenu,
        current_provider: EditPredictionProvider,
        cx: &mut App,
    ) -> ContextMenu {
        let available_providers = get_available_providers(cx);

        let providers: Vec<_> = available_providers
            .into_iter()
            .filter(|p| *p != EditPredictionProvider::None)
            .collect();

        if !providers.is_empty() {
            menu = menu.separator().header("Providers");

            for provider in providers {
                let Some(name) = provider.display_name() else {
                    continue;
                };
                let is_current = provider == current_provider;
                let fs = self.fs.clone();

                menu = menu.item(
                    ContextMenuEntry::new(name)
                        .toggleable(IconPosition::Start, is_current)
                        .handler(move |_, cx| {
                            set_completion_provider(fs.clone(), cx, provider);
                        }),
                )
            }
        }

        menu
    }

    fn add_configure_providers_item(&self, menu: ContextMenu) -> ContextMenu {
        menu.separator().item(
            ContextMenuEntry::new("Configure Providers")
                .icon(IconName::Settings)
                .icon_position(IconPosition::Start)
                .icon_color(Color::Muted)
                .handler(move |window, cx| {
                    telemetry::event!(
                        "Edit Prediction Menu Action",
                        action = "configure_providers",
                    );
                    window.dispatch_action(
                        OpenSettingsAt {
                            path: "edit_predictions.providers".to_string(),
                            target: None,
                        }
                        .boxed_clone(),
                        cx,
                    );
                }),
        )
    }

    pub fn build_copilot_start_menu(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<ContextMenu> {
        let fs = self.fs.clone();
        let project = self.project.clone();
        ContextMenu::build(window, cx, |menu, _, cx| {
            let menu = menu
                .entry(
                    "Sign In to Copilot Edit Predictions",
                    None,
                    move |window, cx| {
                        telemetry::event!(
                            "Edit Prediction Menu Action",
                            action = "sign_in",
                            provider = "copilot",
                        );
                        if let Some(copilot) =
                            EditPredictionStore::try_global(cx).and_then(|store| {
                                store.update(cx, |this, cx| {
                                    this.start_copilot_for_project(&project.upgrade()?, cx)
                                })
                            })
                        {
                            copilot_ui::initiate_sign_in(copilot, window, cx);
                        }
                    },
                )
                .entry("Disable Copilot Edit Predictions", None, {
                    let fs = fs.clone();
                    move |_window, cx| {
                        telemetry::event!(
                            "Edit Prediction Menu Action",
                            action = "disable_provider",
                            provider = "copilot",
                        );
                        hide_copilot(fs.clone(), cx)
                    }
                });

            let menu =
                self.add_provider_switching_section(menu, EditPredictionProvider::Copilot, cx);
            let menu = self.add_configure_providers_item(menu);
            menu
        })
    }

    pub fn build_language_settings_menu(&self, mut menu: ContextMenu, cx: &mut App) -> ContextMenu {
        let fs = self.fs.clone();

        menu = menu.header("Show Edit Predictions For");

        let language_state = self.language.as_ref().map(|language| {
            (
                language.clone(),
                LanguageSettings::resolve(None, Some(&language.name()), cx).show_edit_predictions,
            )
        });

        if let Some(editor_focus_handle) = self.editor_focus_handle.clone() {
            let entry = ContextMenuEntry::new("This Buffer")
                .toggleable(IconPosition::Start, self.editor_show_predictions)
                .action(Box::new(editor::actions::ToggleEditPrediction))
                .handler(move |window, cx| {
                    editor_focus_handle.dispatch_action(
                        &editor::actions::ToggleEditPrediction,
                        window,
                        cx,
                    );
                });

            match language_state.clone() {
                Some((language, false)) => {
                    menu = menu.item(entry.disabled(true).documentation_aside(
                        DocumentationSide::Left,
                        move |_cx| {
                            Label::new(format!(
                                "Edit predictions are disabled for {}",
                                language.name()
                            ))
                            .into_any_element()
                        },
                    ));
                }
                Some(_) | None => menu = menu.item(entry),
            }
        }

        if let Some((language, language_enabled)) = language_state {
            let fs = fs.clone();
            let language_name = language.name();

            menu = menu.toggleable_entry(
                language_name.clone(),
                language_enabled,
                IconPosition::Start,
                None,
                move |_, cx| {
                    telemetry::event!(
                        "Edit Prediction Setting Changed",
                        setting = "language",
                        language = language_name.to_string(),
                        enabled = !language_enabled,
                    );
                    toggle_show_edit_predictions_for_language(language.clone(), fs.clone(), cx)
                },
            );
        }

        let settings = AllLanguageSettings::get_global(cx);

        let globally_enabled = settings.show_edit_predictions(None, cx);
        let entry = ContextMenuEntry::new("All Files")
            .toggleable(IconPosition::Start, globally_enabled)
            .action(workspace::ToggleEditPrediction.boxed_clone())
            .handler(|window, cx| {
                window.dispatch_action(workspace::ToggleEditPrediction.boxed_clone(), cx)
            });
        menu = menu.item(entry);

        let current_mode = settings.edit_predictions_mode();
        let subtle_mode = matches!(current_mode, EditPredictionsMode::Subtle);
        let eager_mode = matches!(current_mode, EditPredictionsMode::Eager);

        menu = menu
                .separator()
                .header("Display Modes")
                .item(
                    ContextMenuEntry::new("Eager")
                        .toggleable(IconPosition::Start, eager_mode)
                        .documentation_aside(DocumentationSide::Left, move |_| {
                            Label::new("Display predictions inline when there are no language server completions available.").into_any_element()
                        })
                        .handler({
                            let fs = fs.clone();
                            move |_, cx| {
                                telemetry::event!(
                                    "Edit Prediction Setting Changed",
                                    setting = "mode",
                                    value = "eager",
                                );
                                toggle_edit_prediction_mode(fs.clone(), EditPredictionsMode::Eager, cx)
                            }
                        }),
                )
                .item(
                    ContextMenuEntry::new("Subtle")
                        .toggleable(IconPosition::Start, subtle_mode)
                        .documentation_aside(DocumentationSide::Left, move |_| {
                            Label::new("Display predictions inline only when holding a modifier key (alt by default).").into_any_element()
                        })
                        .handler({
                            let fs = fs.clone();
                            move |_, cx| {
                                telemetry::event!(
                                    "Edit Prediction Setting Changed",
                                    setting = "mode",
                                    value = "subtle",
                                );
                                toggle_edit_prediction_mode(fs.clone(), EditPredictionsMode::Subtle, cx)
                            }
                        }),
                );

        menu = menu.separator().header("Privacy");

        menu = menu.item(
            ContextMenuEntry::new("Configure Excluded Files")
                .icon(IconName::Lock)
                .icon_color(Color::Muted)
                .documentation_aside(DocumentationSide::Left, |_| {
                    Label::new(indoc!{"
                        Open your settings to add sensitive paths for which Zed will never predict edits."}).into_any_element()
                })
                .handler(move |window, cx| {
                    telemetry::event!(
                        "Edit Prediction Menu Action",
                        action = "configure_excluded_files",
                    );
                    if let Some(workspace) = Workspace::for_window(window, cx) {
                        let workspace = workspace.downgrade();
                        window
                            .spawn(cx, async |cx| {
                                open_disabled_globs_setting_in_editor(
                                    workspace,
                                    cx,
                                ).await
                            })
                            .detach_and_log_err(cx);
                    }
                }),
        ).item(
            ContextMenuEntry::new("View Docs")
                .icon(IconName::FileGeneric)
                .icon_color(Color::Muted)
                .handler(move |_, cx| {
                    telemetry::event!(
                        "Edit Prediction Menu Action",
                        action = "view_docs",
                    );
                    cx.open_url(PRIVACY_DOCS);
                })
        );

        if !self.editor_enabled.unwrap_or(true) {
            let icons = self
                .edit_prediction_provider
                .as_ref()
                .map(|p| p.icons(cx))
                .unwrap_or_else(|| {
                    edit_prediction_types::EditPredictionIconSet::new(IconName::ZedPredict)
                });
            menu = menu.item(
                ContextMenuEntry::new("This file is excluded.")
                    .disabled(true)
                    .icon(icons.disabled)
                    .icon_size(IconSize::Small),
            );
        }

        if let Some(editor_focus_handle) = self.editor_focus_handle.clone() {
            menu = menu
                .separator()
                .header("Actions")
                .entry(
                    "Predict Edit at Cursor",
                    Some(Box::new(ShowEditPrediction)),
                    {
                        let editor_focus_handle = editor_focus_handle.clone();
                        move |window, cx| {
                            telemetry::event!(
                                "Edit Prediction Menu Action",
                                action = "predict_at_cursor",
                            );
                            editor_focus_handle.dispatch_action(&ShowEditPrediction, window, cx);
                        }
                    },
                )
                .context(editor_focus_handle);
        }

        menu
    }

    fn build_copilot_context_menu(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<ContextMenu> {
        let all_language_settings = all_language_settings(None, cx);
        let next_edit_suggestions = all_language_settings
            .edit_predictions
            .copilot
            .enable_next_edit_suggestions
            .unwrap_or(true);
        let copilot_config = copilot_chat::CopilotChatConfiguration {
            enterprise_uri: all_language_settings
                .edit_predictions
                .copilot
                .enterprise_uri
                .clone(),
        };
        let settings_url = copilot_settings_url(copilot_config.enterprise_uri.as_deref());

        ContextMenu::build(window, cx, |menu, _, cx| {
            let menu = self.build_language_settings_menu(menu, cx);
            let menu =
                self.add_provider_switching_section(menu, EditPredictionProvider::Copilot, cx);

            let menu = self.add_configure_providers_item(menu);
            let menu = menu
                .separator()
                .item(
                    ContextMenuEntry::new("Copilot: Next Edit Suggestions")
                        .toggleable(IconPosition::Start, next_edit_suggestions)
                        .handler({
                            let fs = self.fs.clone();
                            move |_, cx| {
                                update_settings_file(fs.clone(), cx, move |settings, _| {
                                    settings
                                        .project
                                        .all_languages
                                        .edit_predictions
                                        .get_or_insert_default()
                                        .copilot
                                        .get_or_insert_default()
                                        .enable_next_edit_suggestions =
                                        Some(!next_edit_suggestions);
                                });
                            }
                        }),
                )
                .separator()
                .link(
                    "Go to Copilot Settings",
                    OpenBrowser { url: settings_url }.boxed_clone(),
                )
                .entry("Sign Out", None, |window, cx| {
                    if let Some(auth) = copilot::GlobalCopilotAuth::try_global(cx) {
                        copilot_ui::initiate_sign_out(auth.0.clone(), window, cx);
                    }
                });
            menu
        })
    }

    fn build_codestral_context_menu(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<ContextMenu> {
        ContextMenu::build(window, cx, |menu, _, cx| {
            let menu = self.build_language_settings_menu(menu, cx);
            let menu =
                self.add_provider_switching_section(menu, EditPredictionProvider::Codestral, cx);

            let menu = self.add_configure_providers_item(menu);
            menu
        })
    }

    fn build_edit_prediction_context_menu(
        &self,
        provider: EditPredictionProvider,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<ContextMenu> {
        ContextMenu::build(window, cx, |mut menu, _, cx| {
            let mercury_payment_required = matches!(provider, EditPredictionProvider::Mercury)
                && edit_prediction::EditPredictionStore::try_global(cx)
                    .is_some_and(|ep_store| ep_store.read(cx).mercury_has_payment_required_error());

            if mercury_payment_required {
                menu = menu
                    .header("Mercury")
                    .item(ContextMenuEntry::new("Free tier limit reached").disabled(true))
                    .item(
                        ContextMenuEntry::new(
                            "Upgrade to a paid plan to continue using the service",
                        )
                        .disabled(true),
                    )
                    .separator();
            }

            menu = self.build_language_settings_menu(menu, cx);
            menu = self.add_provider_switching_section(menu, provider, cx);

            self.add_configure_providers_item(menu)
        })
    }

    pub fn update_enabled(&mut self, editor: Entity<Editor>, cx: &mut Context<Self>) {
        let editor = editor.read(cx);
        let snapshot = editor.buffer().read(cx).snapshot(cx);
        let suggestion_anchor = editor.selections.newest_anchor().start;
        let language = snapshot.language_at(suggestion_anchor);
        let file = snapshot.file_at(suggestion_anchor).cloned();
        self.editor_enabled = {
            let file = file.as_ref();
            Some(
                file.map(|file| {
                    all_language_settings(Some(file), cx)
                        .edit_predictions_enabled_for_file(file, cx)
                })
                .unwrap_or(true),
            )
        };
        self.editor_show_predictions = editor.edit_predictions_enabled();
        self.edit_prediction_provider = editor.edit_prediction_provider();
        self.language = language.cloned();
        self.file = file;
        self.editor_focus_handle = Some(editor.focus_handle(cx));

        cx.notify();
    }
}

impl StatusItemView for EditPredictionButton {
    fn set_active_pane_item(
        &mut self,
        item: Option<&dyn ItemHandle>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(editor) = item.and_then(|item| item.act_as::<Editor>(cx)) {
            self.editor_subscription = Some((
                cx.observe(&editor, Self::update_enabled),
                editor.entity_id().as_u64() as usize,
            ));
            self.update_enabled(editor, cx);
        } else {
            self.language = None;
            self.editor_subscription = None;
            self.editor_enabled = None;
        }
        cx.notify();
    }

    fn hide_setting(&self, _: &App) -> Option<HideStatusItem> {
        // This button is already gated on having a non-disabled edit
        // prediction provider, which the user manages through provider/AI
        // settings.
        None
    }
}

async fn open_disabled_globs_setting_in_editor(
    workspace: WeakEntity<Workspace>,
    cx: &mut AsyncWindowContext,
) -> Result<()> {
    let settings_editor = workspace
        .update_in(cx, |_, window, cx| {
            create_and_open_local_file(paths::settings_file(), window, cx, || {
                settings::initial_user_settings_content().as_ref().into()
            })
        })?
        .await?
        .downcast::<Editor>()
        .unwrap();

    settings_editor
        .downgrade()
        .update_in(cx, |item, window, cx| {
            let text = item.buffer().read(cx).snapshot(cx).text();

            let settings = cx.global::<SettingsStore>();

            // Ensure that we always have "edit_predictions { "disabled_globs": [] }"
            let Some(edits) = settings
                .edits_for_update(&text, |file| {
                    file.project
                        .all_languages
                        .edit_predictions
                        .get_or_insert_with(Default::default)
                        .disabled_globs
                        .get_or_insert_with(Vec::new);
                })
                .log_err()
            else {
                return;
            };

            if !edits.is_empty() {
                item.edit(
                    edits
                        .into_iter()
                        .map(|(r, s)| (MultiBufferOffset(r.start)..MultiBufferOffset(r.end), s)),
                    cx,
                );
            }

            let text = item.buffer().read(cx).snapshot(cx).text();

            static DISABLED_GLOBS_REGEX: LazyLock<Regex> = LazyLock::new(|| {
                Regex::new(r#""disabled_globs":\s*\[\s*(?P<content>(?:.|\n)*?)\s*\]"#).unwrap()
            });
            // Only capture [...]
            let range = DISABLED_GLOBS_REGEX.captures(&text).and_then(|captures| {
                captures
                    .name("content")
                    .map(|inner_match| inner_match.start()..inner_match.end())
            });
            if let Some(range) = range {
                let range = MultiBufferOffset(range.start)..MultiBufferOffset(range.end);
                item.change_selections(
                    SelectionEffects::scroll(Autoscroll::newest()),
                    window,
                    cx,
                    |selections| {
                        selections.select_ranges(vec![range]);
                    },
                );
            }
        })?;

    anyhow::Ok(())
}

pub fn set_completion_provider(fs: Arc<dyn Fs>, cx: &mut App, provider: EditPredictionProvider) {
    update_settings_file(fs, cx, move |settings, _| {
        settings
            .project
            .all_languages
            .edit_predictions
            .get_or_insert_default()
            .provider = Some(provider);
    });
}

pub fn get_available_providers(cx: &mut App) -> Vec<EditPredictionProvider> {
    let mut providers = Vec::new();

    let app_state = workspace::AppState::global(cx);
    if copilot::GlobalCopilotAuth::try_get_or_init(app_state, cx)
        .is_some_and(|copilot| copilot.0.read(cx).is_authenticated())
    {
        providers.push(EditPredictionProvider::Copilot);
    };

    if codestral::codestral_api_key(cx).is_some() {
        providers.push(EditPredictionProvider::Codestral);
    }

    if edit_prediction::ollama::is_available(cx) {
        providers.push(EditPredictionProvider::Ollama);
    }

    if all_language_settings(None, cx)
        .edit_predictions
        .open_ai_compatible_api
        .is_some()
    {
        providers.push(EditPredictionProvider::OpenAiCompatibleApi);
    }

    if edit_prediction::mercury::mercury_api_token(cx)
        .read(cx)
        .has_key()
    {
        providers.push(EditPredictionProvider::Mercury);
    }

    providers
}

fn toggle_show_edit_predictions_for_language(
    language: Arc<Language>,
    fs: Arc<dyn Fs>,
    cx: &mut App,
) {
    let show_edit_predictions =
        all_language_settings(None, cx).show_edit_predictions(Some(&language), cx);
    update_settings_file(fs, cx, move |settings, _| {
        settings
            .project
            .all_languages
            .languages
            .0
            .entry(language.name().0.to_string())
            .or_default()
            .show_edit_predictions = Some(!show_edit_predictions);
    });
}

fn hide_copilot(fs: Arc<dyn Fs>, cx: &mut App) {
    update_settings_file(fs, cx, move |settings, _| {
        settings
            .project
            .all_languages
            .edit_predictions
            .get_or_insert(Default::default())
            .provider = Some(EditPredictionProvider::None);
    });
}

fn toggle_edit_prediction_mode(fs: Arc<dyn Fs>, mode: EditPredictionsMode, cx: &mut App) {
    let settings = AllLanguageSettings::get_global(cx);
    let current_mode = settings.edit_predictions_mode();

    if current_mode != mode {
        update_settings_file(fs, cx, move |settings, _cx| {
            if let Some(edit_predictions) = settings.project.all_languages.edit_predictions.as_mut()
            {
                edit_predictions.mode = Some(mode);
            } else {
                settings.project.all_languages.edit_predictions =
                    Some(settings::EditPredictionSettingsContent {
                        mode: Some(mode),
                        ..Default::default()
                    });
            }
        });
    }
}

fn emit_edit_prediction_menu_opened(
    provider: &str,
    file: &Option<Arc<dyn File>>,
    language: &Option<Arc<Language>>,
    project: &WeakEntity<Project>,
    cx: &App,
) {
    let language_name = language.as_ref().map(|l| l.name());
    let edit_predictions_enabled_for_language =
        LanguageSettings::resolve(None, language_name.as_ref(), cx).show_edit_predictions;
    let file_extension = file
        .as_ref()
        .and_then(|f| {
            std::path::Path::new(f.file_name(cx))
                .extension()
                .and_then(|e| e.to_str())
        })
        .map(|s| s.to_string());
    let is_via_ssh = project
        .upgrade()
        .map(|p| p.read(cx).is_via_remote_server())
        .unwrap_or(false);
    telemetry::event!(
        "Toolbar Menu Opened",
        name = "Edit Predictions",
        provider,
        file_extension,
        edit_predictions_enabled_for_language,
        is_via_ssh,
    );
}

fn copilot_settings_url(enterprise_uri: Option<&str>) -> Arc<str> {
    match enterprise_uri {
        Some(uri) => format!("{}{}", uri.trim_end_matches('/'), COPILOT_SETTINGS_PATH).into(),
        None => COPILOT_SETTINGS_URL.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;

    #[gpui::test]
    async fn test_copilot_settings_url_with_enterprise_uri(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let settings_store = SettingsStore::test(cx);
            cx.set_global(settings_store);
        });

        cx.update_global(|settings_store: &mut SettingsStore, cx| {
            settings_store
                .set_user_settings(
                    r#"{"edit_predictions":{"copilot":{"enterprise_uri":"https://my-company.ghe.com"}}}"#,
                    cx,
                )
                .unwrap();
        });

        let url = cx.update(|cx| {
            let all_language_settings = all_language_settings(None, cx);
            copilot_settings_url(
                all_language_settings
                    .edit_predictions
                    .copilot
                    .enterprise_uri
                    .as_deref(),
            )
        });

        assert_eq!(url.as_ref(), "https://my-company.ghe.com/settings/copilot");
    }

    #[gpui::test]
    async fn test_copilot_settings_url_with_enterprise_uri_trailing_slash(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let settings_store = SettingsStore::test(cx);
            cx.set_global(settings_store);
        });

        cx.update_global(|settings_store: &mut SettingsStore, cx| {
            settings_store
                .set_user_settings(
                    r#"{"edit_predictions":{"copilot":{"enterprise_uri":"https://my-company.ghe.com/"}}}"#,
                    cx,
                )
                .unwrap();
        });

        let url = cx.update(|cx| {
            let all_language_settings = all_language_settings(None, cx);
            copilot_settings_url(
                all_language_settings
                    .edit_predictions
                    .copilot
                    .enterprise_uri
                    .as_deref(),
            )
        });

        assert_eq!(url.as_ref(), "https://my-company.ghe.com/settings/copilot");
    }

    #[gpui::test]
    async fn test_copilot_settings_url_without_enterprise_uri(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let settings_store = SettingsStore::test(cx);
            cx.set_global(settings_store);
        });

        let url = cx.update(|cx| {
            let all_language_settings = all_language_settings(None, cx);
            copilot_settings_url(
                all_language_settings
                    .edit_predictions
                    .copilot
                    .enterprise_uri
                    .as_deref(),
            )
        });

        assert_eq!(url.as_ref(), "https://github.com/settings/copilot");
    }
}
