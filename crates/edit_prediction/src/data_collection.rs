use std::fmt::Write as _;

pub use zeta_prompt::udiff::CURSOR_POSITION_MARKER;

pub fn format_cursor_excerpt(
    excerpt: &str,
    cursor_offset: usize,
    line_comment_prefix: &str,
) -> String {
    let cursor_line_start = excerpt[..cursor_offset]
        .rfind('\n')
        .map(|pos| pos + 1)
        .unwrap_or(0);
    let cursor_line_end = excerpt[cursor_line_start..]
        .find('\n')
        .map(|pos| cursor_line_start + pos + 1)
        .unwrap_or(excerpt.len());
    let cursor_line = &excerpt[cursor_line_start..cursor_line_end];
    let cursor_line_indent = &cursor_line[..cursor_line.len() - cursor_line.trim_start().len()];
    let cursor_column = cursor_offset - cursor_line_start;

    let mut marker_line = String::new();
    if cursor_column < line_comment_prefix.len() {
        for _ in 0..cursor_column {
            marker_line.push(' ');
        }
        marker_line.push_str(line_comment_prefix);
        write!(marker_line, " <{}", CURSOR_POSITION_MARKER).unwrap();
    } else {
        if cursor_column >= cursor_line_indent.len() + line_comment_prefix.len() {
            marker_line.push_str(cursor_line_indent);
        }
        marker_line.push_str(line_comment_prefix);
        while marker_line.len() < cursor_column {
            marker_line.push(' ');
        }
        write!(marker_line, "^{}", CURSOR_POSITION_MARKER).unwrap();
    }

    let mut result = String::with_capacity(excerpt.len() + marker_line.len() + 2);
    result.push_str(&excerpt[..cursor_line_end]);
    if !result.ends_with('\n') {
        result.push('\n');
    }
    result.push_str(&marker_line);
    if cursor_line_end < excerpt.len() {
        result.push('\n');
        result.push_str(&excerpt[cursor_line_end..]);
    }
    result
}
