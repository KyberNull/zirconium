use crate::{
    Anchor, AnchorRangeExt, DisplayPoint, DisplayRow, Editor, EditorSettings, EditorSnapshot,
    HighlightKey, Hover,
    display_map::{ToDisplayPoint, is_invisible},
    editor_settings::EditorSettingsScrollbarProxy,
    hover_links::RangeInEditor,
    movement::TextLayoutDetails,
    scroll::ScrollAmount,
};
use gpui::{
    AnyElement, App, AsyncWindowContext, Bounds, Context, Entity, Focusable as _, FontWeight,
    InteractiveElement, IntoElement, MouseButton, ParentElement, Pixels, ScrollHandle, Size,
    StatefulInteractiveElement, StyleRefinement, Styled, Subscription, Task, TaskExt,
    TextStyleRefinement, Window, canvas, div, px,
};
use itertools::Itertools;
use language::{Language, LanguageRegistry};
use markdown::{CopyButtonVisibility, Markdown, MarkdownElement, MarkdownStyle};
use multi_buffer::ToOffset;
use project::{HoverBlock, HoverBlockKind};
use settings::Settings;
use std::{
    borrow::Cow,
    cell::{Cell, RefCell},
};
use std::{ops::Range, sync::Arc, time::Duration};
use std::{path::PathBuf, rc::Rc};
use theme_settings::ThemeSettings;
use ui::{Scrollbars, WithScrollbar, prelude::*};
use url::Url;
use util::TryFutureExt;
use workspace::{OpenOptions, OpenVisible, Workspace};

pub const MIN_POPOVER_CHARACTER_WIDTH: f32 = 20.;
pub const MIN_POPOVER_LINE_HEIGHT: f32 = 4.;
pub const POPOVER_RIGHT_OFFSET: Pixels = px(8.0);
pub const HOVER_POPOVER_GAP: Pixels = px(10.);

/// Bindable action which uses the most recent selection head to trigger a hover
pub fn hover(editor: &mut Editor, _: &Hover, window: &mut Window, cx: &mut Context<Editor>) {
    let head = editor.selections.newest_anchor().head();
    show_hover(editor, head, true, window, cx);
}

/// The internal hover action dispatches between `show_hover` or `hide_hover`
/// depending on whether a point to hover over is provided.
pub fn hover_at(
    editor: &mut Editor,
    anchor: Option<Anchor>,
    mouse_position: Option<gpui::Point<Pixels>>,
    window: &mut Window,
    cx: &mut Context<Editor>,
) {
    if EditorSettings::get_global(cx).hover_popover_enabled {
        if show_keyboard_hover(editor, window, cx) {
            return;
        }

        if let Some(anchor) = anchor {
            editor.hover_state.hiding_delay_task = None;
            editor.hover_state.closest_mouse_distance = None;
            show_hover(editor, anchor, false, window, cx);
        } else if !editor.hover_state.visible() {
            editor.hover_state.info_task = None;
        } else {
            let settings = EditorSettings::get_global(cx);
            if !settings.hover_popover_sticky {
                hide_hover(editor, cx);
                return;
            }

            let mut getting_closer = false;
            if let Some(mouse_position) = mouse_position {
                getting_closer = editor.hover_state.is_mouse_getting_closer(mouse_position);
            }

            // If we are moving away and a timer is already running, just let it count down.
            if !getting_closer && editor.hover_state.hiding_delay_task.is_some() {
                return;
            }

            // If we are moving closer, or if no timer is running at all, start/restart the timer.
            let delay = Duration::from_millis(settings.hover_popover_hiding_delay.0);
            let task = cx.spawn(async move |this, cx| {
                cx.background_executor().timer(delay).await;
                this.update(cx, |editor, cx| {
                    hide_hover(editor, cx);
                })
                .ok();
            });
            editor.hover_state.hiding_delay_task = Some(task);
        }
    }
}

pub fn show_keyboard_hover(
    editor: &mut Editor,
    window: &mut Window,
    cx: &mut Context<Editor>,
) -> bool {
    if let Some(anchor) = editor.hover_state.info_popovers.iter().find_map(|p| {
        if *p.keyboard_grace.borrow() {
            p.anchor
        } else {
            None
        }
    }) {
        show_hover(editor, anchor, false, window, cx);
        return true;
    }

    false
}

/// Hides the type information popup.
/// Triggered by the `Hover` action when the cursor is not over a symbol or when the
/// selections changed.
pub fn hide_hover(editor: &mut Editor, cx: &mut Context<Editor>) -> bool {
    let info_popovers = editor.hover_state.info_popovers.drain(..);
    let did_hide = info_popovers.count() > 0;

    editor.hover_state.info_task = None;
    editor.hover_state.hiding_delay_task = None;
    editor.hover_state.closest_mouse_distance = None;

    editor.clear_background_highlights(HighlightKey::HoverState, cx);

    if did_hide {
        cx.notify();
    }

    did_hide
}

/// Shows diagnostics and character info about the position the mouse is
/// currently hovering over.
/// Triggered by the `Hover` action when the cursor may be over a symbol.
fn show_hover(
    editor: &mut Editor,
    anchor: Anchor,
    ignore_timeout: bool,
    window: &mut Window,
    cx: &mut Context<Editor>,
) -> Option<()> {
    let snapshot = editor.snapshot(window, cx);

    let language_registry = editor
        .project()
        .map(|project| project.read(cx).languages().clone());
    editor.hover_state.hiding_delay_task = None;
    editor.hover_state.closest_mouse_distance = None;

    if !ignore_timeout {
        if same_info_hover(editor, &snapshot, anchor) {
            // Hover triggered from same location as last time. Don't show again.
            return None;
        } else {
            hide_hover(editor, cx);
        }
    }

    let hover_popover_delay = EditorSettings::get_global(cx).hover_popover_delay.0;
    let task = cx.spawn_in(window, async move |this, cx| {
        async move {
            if !ignore_timeout {
                cx.background_executor()
                    .timer(Duration::from_millis(hover_popover_delay))
                    .await;
            }
            let invisible_char = if let Some(invisible) = snapshot
                .buffer_snapshot()
                .chars_at(anchor)
                .next()
                .filter(|&c| is_invisible(c))
            {
                let after = snapshot.buffer_snapshot().anchor_after(
                    anchor.to_offset(&snapshot.buffer_snapshot()) + invisible.len_utf8(),
                );
                Some((invisible, anchor..after))
            } else if let Some(invisible) = snapshot
                .buffer_snapshot()
                .reversed_chars_at(anchor)
                .next()
                .filter(|&c| is_invisible(c))
            {
                let before = snapshot.buffer_snapshot().anchor_before(
                    anchor.to_offset(&snapshot.buffer_snapshot()) - invisible.len_utf8(),
                );

                Some((invisible, before..anchor))
            } else {
                None
            };

            let mut info_popovers =
                Vec::with_capacity(if invisible_char.is_some() { 1 } else { 0 });

            if let Some((invisible, range)) = invisible_char {
                let blocks = vec![HoverBlock {
                    text: format!("Unicode character U+{:02X}", invisible as u32),
                    kind: HoverBlockKind::PlainText,
                }];
                let parsed_content = parse_blocks(&blocks, language_registry.as_ref(), None, cx);
                let scroll_handle = ScrollHandle::new();
                let subscription = this
                    .update(cx, |_, cx| {
                        parsed_content.as_ref().map(|parsed_content| {
                            cx.observe(parsed_content, |_, _, cx| cx.notify())
                        })
                    })
                    .ok()
                    .flatten();
                info_popovers.push(InfoPopover {
                    symbol_range: RangeInEditor::Text(range),
                    parsed_content,
                    scroll_handle,
                    keyboard_grace: Rc::new(RefCell::new(ignore_timeout)),
                    anchor: Some(anchor),
                    last_bounds: Rc::new(Cell::new(None)),
                    _subscription: subscription,
                })
            }

            this.update_in(cx, |editor, window, cx| {
                editor.clear_background_highlights(HighlightKey::HoverState, cx);

                editor.hover_state.info_popovers = info_popovers;
                cx.notify();
                window.refresh();
            })?;

            anyhow::Ok(())
        }
        .log_err()
        .await
    });

    editor.hover_state.info_task = Some(task);
    None
}

fn same_info_hover(editor: &Editor, snapshot: &EditorSnapshot, anchor: Anchor) -> bool {
    editor
        .hover_state
        .info_popovers
        .iter()
        .any(|InfoPopover { symbol_range, .. }| {
            symbol_range
                .as_text_range()
                .map(|range| {
                    let hover_range = range.to_offset(&snapshot.buffer_snapshot());
                    let offset = anchor.to_offset(&snapshot.buffer_snapshot());
                    // LSP returns a hover result for the end index of ranges that should be hovered, so we need to
                    // use an inclusive range here to check if we should dismiss the popover
                    (hover_range.start..=hover_range.end).contains(&offset)
                })
                .unwrap_or(false)
        })
}

fn parse_blocks(
    blocks: &[HoverBlock],
    language_registry: Option<&Arc<LanguageRegistry>>,
    language: Option<Arc<Language>>,
    cx: &mut AsyncWindowContext,
) -> Option<Entity<Markdown>> {
    let combined_text = blocks
        .iter()
        .map(|block| match &block.kind {
            project::HoverBlockKind::PlainText | project::HoverBlockKind::Markdown => {
                Cow::Borrowed(block.text.trim())
            }
            project::HoverBlockKind::Code { language } => {
                Cow::Owned(format!("```{}\n{}\n```", language, block.text.trim()))
            }
        })
        .join("\n\n");

    cx.new_window_entity(|_window, cx| {
        Markdown::new(
            combined_text.into(),
            language_registry.cloned(),
            language.map(|language| language.name()),
            cx,
        )
    })
    .ok()
}

pub fn hover_markdown_style(window: &Window, cx: &App) -> MarkdownStyle {
    let settings = ThemeSettings::get_global(cx);
    let ui_font_family = settings.ui_font.family.clone();
    let ui_font_features = settings.ui_font.features.clone();
    let ui_font_fallbacks = settings.ui_font.fallbacks.clone();
    let buffer_font_family = settings.buffer_font.family.clone();
    let buffer_font_features = settings.buffer_font.features.clone();
    let buffer_font_fallbacks = settings.buffer_font.fallbacks.clone();
    let buffer_font_weight = settings.buffer_font.weight;

    let mut base_text_style = window.text_style();
    base_text_style.refine(&TextStyleRefinement {
        font_family: Some(ui_font_family),
        font_features: Some(ui_font_features),
        font_fallbacks: ui_font_fallbacks,
        color: Some(cx.theme().colors().editor_foreground),
        ..Default::default()
    });
    MarkdownStyle {
        base_text_style,
        code_block: StyleRefinement::default()
            .my(rems(1.))
            .font_buffer(cx)
            .font_features(buffer_font_features.clone())
            .font_weight(buffer_font_weight),
        inline_code: TextStyleRefinement {
            background_color: Some(cx.theme().colors().background),
            font_family: Some(buffer_font_family),
            font_features: Some(buffer_font_features),
            font_fallbacks: buffer_font_fallbacks,
            font_weight: Some(buffer_font_weight),
            ..Default::default()
        },
        rule_color: cx.theme().colors().border,
        block_quote_border_color: Color::Muted.color(cx),
        block_quote: TextStyleRefinement {
            color: Some(Color::Muted.color(cx)),
            ..Default::default()
        },
        link: TextStyleRefinement {
            color: Some(cx.theme().colors().editor_foreground),
            underline: Some(gpui::UnderlineStyle {
                thickness: px(1.),
                color: Some(cx.theme().colors().editor_foreground),
                wavy: false,
            }),
            ..Default::default()
        },
        syntax: cx.theme().syntax().clone(),
        selection_background_color: cx.theme().colors().element_selection_background,
        heading: StyleRefinement::default()
            .font_weight(FontWeight::BOLD)
            .text_base()
            .mt(rems(1.))
            .mb_0(),
        table_columns_min_size: true,
        ..Default::default()
    }
}

fn parse_file_link(link: &str) -> Option<(PathBuf, Option<String>)> {
    let uri = Url::parse(link).ok().filter(|uri| uri.scheme() == "file")?;
    let fragment = uri.fragment().map(ToOwned::to_owned);
    let path = uri.to_file_path().unwrap_or_else(|_| {
        let encoded = uri.path();

        urlencoding::decode(encoded)
            .map(Cow::into_owned)
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(encoded))
    });

    Some((path, fragment))
}

pub fn open_markdown_url(
    workspace: Option<Entity<Workspace>>,
    link: SharedString,
    window: &mut Window,
    cx: &mut App,
) {
    if let Some((path, fragment)) = parse_file_link(&link)
        && let Some(workspace) = workspace
    {
        workspace.update(cx, |workspace, cx| {
            let task = workspace.open_abs_path(
                path,
                OpenOptions {
                    visible: Some(OpenVisible::None),
                    ..Default::default()
                },
                window,
                cx,
            );

            cx.spawn_in(window, async move |_, cx| {
                let item = task.await?;
                // Ruby LSP uses URLs with #L1,1-4,4
                // we'll just take the first number and assume it's a line number
                let Some(fragment) = fragment else {
                    return anyhow::Ok(());
                };
                let mut accum = 0u32;
                for c in fragment.chars() {
                    if c >= '0' && c <= '9' && accum < u32::MAX / 2 {
                        accum *= 10;
                        accum += c as u32 - '0' as u32;
                    } else if accum > 0 {
                        break;
                    }
                }
                if accum == 0 {
                    return Ok(());
                }
                let Some(editor) = cx.update(|_, cx| item.act_as::<Editor>(cx))? else {
                    return Ok(());
                };
                editor.update_in(cx, |editor, window, cx| {
                    editor.change_selections(Default::default(), window, cx, |selections| {
                        selections.select_ranges([
                            text::Point::new(accum - 1, 0)..text::Point::new(accum - 1, 0)
                        ]);
                    });
                })
            })
            .detach_and_log_err(cx);
        });
        return;
    }

    if let Some(workspace) = workspace {
        workspace.update(cx, |workspace, cx| {
            workspace.open_url_or_file(&link, None, window, cx);
        });
    } else {
        cx.open_url(&link);
    }
}

#[derive(Default)]
pub struct HoverState {
    pub info_popovers: Vec<InfoPopover>,
    pub info_task: Option<Task<Option<()>>>,
    pub closest_mouse_distance: Option<Pixels>,
    pub hiding_delay_task: Option<Task<()>>,
}

impl HoverState {
    pub fn visible(&self) -> bool {
        !self.info_popovers.is_empty()
    }

    pub fn is_mouse_getting_closer(&mut self, mouse_position: gpui::Point<Pixels>) -> bool {
        if !self.visible() {
            return false;
        }

        let mut popover_bounds = Vec::new();
        for info_popover in &self.info_popovers {
            if let Some(bounds) = info_popover.last_bounds.get() {
                popover_bounds.push(bounds);
            }
        }

        if popover_bounds.is_empty() {
            return false;
        }

        let distance = popover_bounds
            .iter()
            .map(|bounds| self.distance_from_point_to_bounds(mouse_position, *bounds))
            .min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap_or(px(f32::MAX));

        if let Some(closest_distance) = self.closest_mouse_distance {
            if distance > closest_distance + px(4.0) {
                return false;
            }
        }

        self.closest_mouse_distance =
            Some(distance.min(self.closest_mouse_distance.unwrap_or(distance)));
        true
    }

    fn distance_from_point_to_bounds(
        &self,
        point: gpui::Point<Pixels>,
        bounds: Bounds<Pixels>,
    ) -> Pixels {
        let center_x = bounds.origin.x + bounds.size.width / 2.;
        let center_y = bounds.origin.y + bounds.size.height / 2.;
        let dx: f32 = ((point.x - center_x).abs() - bounds.size.width / 2.)
            .max(px(0.0))
            .into();
        let dy: f32 = ((point.y - center_y).abs() - bounds.size.height / 2.)
            .max(px(0.0))
            .into();
        px((dx.powi(2) + dy.powi(2)).sqrt())
    }

    pub(crate) fn render(
        &mut self,
        snapshot: &EditorSnapshot,
        visible_rows: Range<DisplayRow>,
        max_size: Size<Pixels>,
        text_layout_details: &TextLayoutDetails,
        window: &mut Window,
        cx: &mut Context<Editor>,
    ) -> Option<(DisplayPoint, Vec<AnyElement>)> {
        if !self.visible() {
            return None;
        }
        // Position the popovers at the start of the hover range
        let anchor = self.info_popovers.iter().find_map(|info_popover| {
            match &info_popover.symbol_range {
                RangeInEditor::Text(range) => Some(&range.start),
            }
        })?;
        let mut point = anchor.to_display_point(&snapshot.display_snapshot);
        // Clamp the point within the visible rows in case the popup source spans multiple lines
        if visible_rows.end <= point.row() {
            point = crate::movement::up_by_rows(
                &snapshot.display_snapshot,
                point,
                1 + (point.row() - visible_rows.end).0,
                text::SelectionGoal::None,
                true,
                text_layout_details,
            )
            .0;
        } else if point.row() < visible_rows.start {
            point = crate::movement::down_by_rows(
                &snapshot.display_snapshot,
                point,
                (visible_rows.start - point.row()).0,
                text::SelectionGoal::None,
                true,
                text_layout_details,
            )
            .0;
        }

        if !visible_rows.contains(&point.row()) {
            log::error!("Hover popover point out of bounds after moving");
            return None;
        }

        let mut elements = Vec::new();

        for info_popover in &mut self.info_popovers {
            elements.push(info_popover.render(max_size, window, cx));
        }

        Some((point, elements))
    }

    pub fn focused(&self, window: &mut Window, cx: &mut Context<Editor>) -> bool {
        let mut hover_popover_is_focused = false;
        for info_popover in &self.info_popovers {
            if let Some(markdown_view) = &info_popover.parsed_content
                && markdown_view.focus_handle(cx).is_focused(window)
            {
                hover_popover_is_focused = true;
            }
        }
        hover_popover_is_focused
    }
}

pub struct InfoPopover {
    pub symbol_range: RangeInEditor,
    pub parsed_content: Option<Entity<Markdown>>,
    pub scroll_handle: ScrollHandle,
    pub keyboard_grace: Rc<RefCell<bool>>,
    pub anchor: Option<Anchor>,
    pub last_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    _subscription: Option<Subscription>,
}

impl InfoPopover {
    pub(crate) fn render(
        &mut self,
        max_size: Size<Pixels>,
        window: &mut Window,
        cx: &mut Context<Editor>,
    ) -> AnyElement {
        let keyboard_grace = Rc::clone(&self.keyboard_grace);
        let this = cx.entity().downgrade();
        let this2 = this.clone();
        let bounds_cell = self.last_bounds.clone();
        div()
            .id("info_popover")
            .occlude()
            .elevation_2(cx)
            .child(
                canvas(
                    {
                        move |bounds, _window, _cx| {
                            bounds_cell.set(Some(bounds));
                        }
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
            // Prevent a mouse down/move on the popover from being propagated to the editor,
            // because that would dismiss the popover.
            .on_mouse_move({
                move |_, _, cx: &mut App| {
                    this.update(cx, |editor, _| {
                        editor.hover_state.closest_mouse_distance = Some(px(0.0));
                        editor.hover_state.hiding_delay_task = None;
                    })
                    .ok();
                    cx.stop_propagation()
                }
            })
            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                let mut keyboard_grace = keyboard_grace.borrow_mut();
                *keyboard_grace = false;
                cx.stop_propagation();
            })
            .when_some(self.parsed_content.clone(), |this, markdown| {
                this.child(
                    div()
                        .id("info-md-container")
                        .overflow_y_scroll()
                        .max_w(max_size.width)
                        .max_h(max_size.height)
                        .track_scroll(&self.scroll_handle)
                        .child(
                            MarkdownElement::new(markdown, hover_markdown_style(window, cx))
                                .scroll_handle(self.scroll_handle.clone())
                                .code_block_renderer(markdown::CodeBlockRenderer::Default {
                                    copy_button_visibility: CopyButtonVisibility::Hidden,
                                    wrap_button_visibility: markdown::WrapButtonVisibility::Hidden,
                                    border: false,
                                })
                                .on_url_click(move |link, window, cx| {
                                    open_markdown_url(
                                        this2
                                            .read_with(cx, |editor, _| editor.workspace())
                                            .ok()
                                            .flatten(),
                                        link,
                                        window,
                                        cx,
                                    )
                                })
                                .p_2(),
                        ),
                )
                .custom_scrollbars(
                    Scrollbars::for_settings::<EditorSettingsScrollbarProxy>()
                        .tracked_scroll_handle(&self.scroll_handle),
                    window,
                    cx,
                )
            })
            .into_any_element()
    }

    pub fn scroll(&self, amount: ScrollAmount, window: &mut Window, cx: &mut Context<Editor>) {
        let mut current = self.scroll_handle.offset();
        current.y -= amount.pixels(
            window.line_height(),
            self.scroll_handle.bounds().size.height - px(16.),
        ) / 2.0;
        cx.notify();
        self.scroll_handle.set_offset(current);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        editor_tests::init_test, test::editor_language_test_context::EditorLanguageTestContext,
    };

    use indoc::indoc;
    use settings::{DelayMs, SettingsStore};

    use text::Bias;

    #[gpui::test]
    fn test_hover_markdown_soft_breaks_reflow_per_commonmark(cx: &mut gpui::TestAppContext) {
        init_test(cx, |_| {});

        let cx = cx.add_empty_window();
        // JSDoc / Rust doc comment / Go doc style: single newlines inside a
        // paragraph. Per CommonMark §6.7 these are soft breaks and must render
        // as spaces, producing one reflowing paragraph. See issue #60777.
        let text = concat!("This is a test\n", "for tooltip\n", "reflow");
        let markdown = cx.new(|cx| Markdown::new(text.into(), None, None, cx));
        cx.run_until_parked();

        let rendered = MarkdownElement::rendered_text(markdown, cx, hover_markdown_style);

        // No hard line breaks should appear: the two soft breaks collapse to spaces.
        assert_eq!(
            rendered.matches('\n').count(),
            0,
            "expected no hard line breaks, got {rendered:?}"
        );
        // The whole paragraph reflows onto a single line.
        assert_eq!(rendered, "This is a test for tooltip reflow");
    }

    #[gpui::test]
    fn test_hover_markdown_explicit_hard_breaks_preserved(cx: &mut gpui::TestAppContext) {
        init_test(cx, |_| {});

        let cx = cx.add_empty_window();
        // The spec-correct way for an LSP server to request line preservation
        // is to emit CommonMark hard breaks. PR #54165 also maps `<br>` tags to
        // hard breaks. Both must continue to render as forced line breaks under
        // the default hover style.
        let text = "first line  \nsecond line\\\nthird line";
        let markdown = cx.new(|cx| Markdown::new(text.into(), None, None, cx));
        cx.run_until_parked();

        let rendered = MarkdownElement::rendered_text(markdown, cx, hover_markdown_style);

        // Two explicit hard breaks (two trailing spaces, and a trailing `\`)
        // must render as two newline characters.
        assert_eq!(
            rendered.matches('\n').count(),
            2,
            "expected two hard line breaks, got {rendered:?}"
        );
        let lines: Vec<&str> = rendered.split('\n').collect();
        assert_eq!(lines, ["first line", "second line", "third line"]);
    }

    #[gpui::test]
    // https://github.com/zed-industries/zed/issues/15498
    #[gpui::test]
    async fn test_hover_popover_enabled_false_ignores_sticky(cx: &mut gpui::TestAppContext) {
        init_test(cx, |_| {});

        cx.update(|cx| {
            cx.update_global::<SettingsStore, _>(|settings, cx| {
                settings.update_user_settings(cx, |settings| {
                    settings.editor.hover_popover_enabled = Some(false);
                    settings.editor.hover_popover_sticky = Some(true);
                    settings.editor.hover_popover_hiding_delay = Some(DelayMs(500));
                });
            });
        });

        let mut cx = EditorLanguageTestContext::new_rust(cx).await;

        cx.set_state(indoc! {"
            fn ˇtest() { println!(); }
        "});

        let hover_point = cx.display_point(indoc! {"
            fn test() { printˇln!(); }
        "});

        // Trigger hover_at — should be gated by hover_popover_enabled=false
        cx.update_editor(|editor, window, cx| {
            let snapshot = editor.snapshot(window, cx);
            let anchor = snapshot
                .buffer_snapshot()
                .anchor_before(hover_point.to_offset(&snapshot, Bias::Left));
            hover_at(editor, Some(anchor), None, window, cx)
        });

        // No need to advance clock or wait for LSP — the gate should prevent any work
        cx.editor(|editor, _, _| {
            assert!(
                !editor.hover_state.visible(),
                "Popover should not appear when hover_popover_enabled is false"
            );
            assert!(
                editor.hover_state.info_task.is_none(),
                "No hover info task should be scheduled when hover is disabled"
            );
        });
    }

    #[test]
    fn test_parse_file_links() {
        assert_eq!(
            parse_file_link("file:///path/to/file"),
            Some((PathBuf::from("/path/to/file"), None))
        );
        assert_eq!(
            parse_file_link("file:///path/to/file%20with%20spaces"),
            Some((PathBuf::from("/path/to/file with spaces"), None))
        );
        assert_eq!(
            parse_file_link("file:///path/to/file#123"),
            Some((PathBuf::from("/path/to/file"), Some("123".to_string())))
        );
        assert_eq!(parse_file_link("http://example.com/"), None,);
    }
}
