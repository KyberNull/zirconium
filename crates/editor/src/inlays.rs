//! The logic, responsible for managing [`Inlay`]s in the editor.
//!
//! Inlays are "not real" text that gets mixed into the "real" buffer's text.
//! They are attached to a certain [`Anchor`], and display certain contents (usually, strings)
//! between real text around that anchor.
//!
//! Inlay examples in Zed:
//! * inline predictions, showing the Zeta/Copilot/etc. predictions
//! * ... anything else, potentially.
//!
//! Editor uses [`crate::DisplayMap`] and [`crate::display_map::InlayMap`] to manage what's rendered inside the editor.

use std::sync::OnceLock;

use gpui::{Context, Hsla, Rgba};
use multi_buffer::Anchor;
use project::{InlayHint, InlayId};
use text::Rope;

use crate::Editor;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlayHighlight {
    pub inlay: InlayId,
    pub inlay_position: Anchor,
    pub range: std::ops::Range<usize>,
}

#[derive(Debug, Clone)]
pub struct Inlay {
    pub id: InlayId,
    // TODO this could be an ExcerptAnchor
    pub position: Anchor,
    pub content: InlayContent,
}

#[derive(Debug, Clone)]
pub enum InlayContent {
    Text(text::Rope),
    Color(Hsla),
}

impl Inlay {
    pub fn hint(id: InlayId, position: Anchor, hint: &InlayHint) -> Self {
        let mut text = hint.text();
        let needs_right_padding = hint.padding_right && !text.ends_with(" ");
        let needs_left_padding = hint.padding_left && !text.starts_with(" ");
        if needs_right_padding {
            text.push(" ");
        }
        if needs_left_padding {
            text.push_front(" ");
        }
        Self {
            id,
            position,
            content: InlayContent::Text(text),
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn mock_hint(id: usize, position: Anchor, text: impl Into<Rope>) -> Self {
        Self {
            id: InlayId::Hint(id),
            position,
            content: InlayContent::Text(text.into()),
        }
    }

    pub fn color(id: usize, position: Anchor, color: Rgba) -> Self {
        Self {
            id: InlayId::Color(id),
            position,
            content: InlayContent::Color(color.into()),
        }
    }

    pub fn edit_prediction<T: Into<Rope>>(id: usize, position: Anchor, text: T) -> Self {
        Self {
            id: InlayId::EditPrediction(id),
            position,
            content: InlayContent::Text(text.into()),
        }
    }

    pub fn text(&self) -> &Rope {
        static COLOR_TEXT: OnceLock<Rope> = OnceLock::new();
        match &self.content {
            InlayContent::Text(text) => text,
            InlayContent::Color(_) => COLOR_TEXT.get_or_init(|| Rope::from("◼")),
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn get_color(&self) -> Option<Hsla> {
        match self.content {
            InlayContent::Color(color) => Some(color),
            _ => None,
        }
    }
}

impl Editor {
    /// Modify which hints are displayed in the editor.
    pub fn splice_inlays(
        &mut self,
        to_remove: &[InlayId],
        to_insert: Vec<Inlay>,
        cx: &mut Context<Self>,
    ) {
        self.display_map.update(cx, |display_map, cx| {
            display_map.splice_inlays(to_remove, to_insert, cx)
        });
        cx.notify();
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn all_inlays(&self, cx: &gpui::App) -> Vec<Inlay> {
        self.display_map
            .read(cx)
            .current_inlays()
            .cloned()
            .collect()
    }
}
