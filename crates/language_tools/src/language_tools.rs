mod highlights_tree_view;
mod key_context_view;
mod syntax_tree_view;

use gpui::App;

pub use highlights_tree_view::{HighlightsTreeToolbarItemView, HighlightsTreeView};
pub use syntax_tree_view::{SyntaxTreeToolbarItemView, SyntaxTreeView};

pub fn init(cx: &mut App) {
    highlights_tree_view::init(cx);
    syntax_tree_view::init(cx);
    key_context_view::init(cx);
}
