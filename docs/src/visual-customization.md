---
title: Visual Customization
description: "Various aspects of Zed's visual layout can be configured in the Settings Editor or settings file."
---

# Visual Customization

Various aspects of Zed's visual layout can be configured in the Settings Editor ({#kb zed::OpenSettings}) or in your settings file ({#kb zed::OpenSettingsFile}).

See [All Settings](./reference/all-settings.md) for additional information and other non-visual settings.

## Themes

You can install many [themes](./themes.md) and [icon themes](./icon-themes.md) in form of extensions by running {#action zed::Extensions} from the command palette.

You can preview/choose amongst your installed themes and icon themes with {#action theme_selector::Toggle} ({#kb theme_selector::Toggle}) and {#action icon_theme_selector::Toggle} which will modify the following settings:

```json [settings]
{
  "theme": "One Dark",
  "icon_theme": "Zed (Default)"
}
```

If you would like to use distinct themes for light mode/dark mode that can be set with:

```json [settings]
{
  "theme": {
    "dark": "One Dark",
    "light": "One Light",
    // Mode to use (dark, light) or "system" to follow the OS's light/dark mode (default)
    "mode": "system"
  },
  "icon_theme": {
    "dark": "Zed (Default)",
    "light": "Zed (Default)",
    // Mode to use (dark, light) or "system" to follow the OS's light/dark mode (default)
    "mode": "system"
  }
}
```

## Fonts

```json [settings]
  // UI Font. Use ".SystemUIFont" to use the default system font (SF Pro on macOS),
  // or ".ZedSans" for the bundled default (currently IBM Plex)
  "ui_font_family": ".SystemUIFont",
  "ui_font_weight": 400, // Font weight in standard CSS units from 100 to 900.
  "ui_font_size": 16,

  // Buffer Font - Used by editor buffers
  // use ".ZedMono" for the bundled default monospace (currently Lilex)
  "buffer_font_family": "Berkeley Mono", // Font name for editor buffers
  "buffer_font_size": 15,                 // Font size for editor buffers
  "buffer_font_weight": 400,              // Font weight in CSS units [100-900]
  // Line height "comfortable" (1.618), "standard" (1.3) or custom: `{ "custom": 2 }`
  "buffer_line_height": "comfortable",

  // Terminal Font Settings
  "terminal": {
    "font_family": "",
    "font_size": 15,
    // Terminal line height: comfortable (1.618), standard(1.3) or `{ "custom": 2 }`
    "line_height": "standard",
  },

  // Controls the font family and size for agent responses in the agent panel.
  // If not specified, they fall back to the UI font family and size.
  "agent_ui_font_family": "Inter",
  "agent_ui_font_size": 15,
  // Controls the font family and size for the agent panel's message editor
  // and user messages.
  // If not specified, the font family falls back to the buffer font family.
  "agent_buffer_font_family": "Berkeley Mono",
  // Controls the font size for the agent panel's message editor, user message,
  // and any other snippet of code.
  "agent_buffer_font_size": 12,

  // Controls the font size for the markdown preview.
  // If not specified, it falls back to the editor font size.
  "markdown_preview_font_size": null,
  // Controls the font family for the markdown preview.
  // If not specified, it falls back to the UI font family.
  "markdown_preview_font_family": null,
  // Controls the font family for code blocks in the markdown preview.
  // If not specified, it falls back to the editor font family.
  "markdown_preview_code_font_family": null
```

### Font ligatures

By default Zed enables font ligatures which will visually combine certain adjacent characters.

For example `=>` will be displayed as `→` and `!=` will be `≠`.
This is purely cosmetic and the individual characters remain unchanged.

To disable this behavior use:

```json [settings]
{
  "buffer_font_features": {
    "calt": false // Disable ligatures
  }
}
```

### Status Bar

```json [settings]
{
  // Whether to show full labels in line indicator or short ones
  //   - `short`: "2 s, 15 l, 32 c"
  //   - `long`: "2 selections, 15 lines, 32 characters"
  "line_indicator_format": "long"

  // Individual status bar icons can be hidden:
  // "project_panel": {"button": false },
  // "outline_panel": {"button": false },
  // "git_panel": {"button": false },

  // "agent": {"button": false },
  // "diagnostics": {"button": false },
  // "search": {"button": false },
}
```

### Titlebar

```json [settings]
  // Control which items are shown/hidden in the title bar
  "title_bar": {
    "show_branch_status_icon": false, // Show git status on branch icon
    "show_branch_name": true,       // Show/hide branch name
    "show_worktree_name": true,     // Show/hide worktree name
    "show_project_items": true,     // Show/hide project host and name
    "show_onboarding_banner": true, // Show/hide onboarding banners
    "show_user_picture": true,      // Show/hide user avatar
    "show_user_menu": true,         // Show/hide app user button
    "show_sign_in": true,           // Show/hide sign-in button
    "show_menus": false             // Show/hide menus
  },
```

## Workspace

```json [settings]
{
  // Force usage of Zed build in path prompts (file and directory pickers)
  // instead of OS native pickers (false).
  "use_system_path_prompts": true,
  // Force usage of Zed built in confirmation prompts ("Do you want to save?")
  // instead of OS native prompts (false). On linux this is ignored (always false).
  "use_system_prompts": true,

  // Active pane styling settings.
  "active_pane_modifiers": {
    // Inset border size of the active pane, in pixels.
    "border_size": 0.0,
    // Opacity of the inactive panes. 0 means transparent, 1 means opaque.
    "inactive_opacity": 1.0
  },

  // Layout mode of the bottom dock: contained, full, left_aligned, right_aligned
  "bottom_dock_layout": "contained",

  // Whether to resize all the panels in a dock when resizing the dock.
  // Can be a combination of "left", "right" and "bottom".
  "resize_all_panels_in_dock": ["left"]
}
```

<!--
TBD: Centered layout related settings
```json [settings]
    "centered_layout": {
    // The relative width of the left padding of the central pane from the
    // workspace when the centered layout is used.
    "left_padding": 0.2,
    // The relative width of the right padding of the central pane from the
    // workspace when the centered layout is used.
    "right_padding": 0.2
    },
```
-->

## Editor

```json [settings]
  // Whether the cursor blinks in the editor.
  "cursor_blink": true,

  // Cursor shape for the default editor: bar, block, underline, hollow
  "cursor_shape": null,

  // Highlight the current line in the editor: none, gutter, line, all
  "current_line_highlight": "all",

  // When does the mouse cursor hide: never, on_typing, on_typing_and_action
  "hide_mouse": "on_typing_and_action",

  // Whether to highlight all occurrences of the selected text in an editor.
  "selection_highlight": true,

  // Visually show tabs and spaces  (none, all, selection, boundary, trailing)
  "show_whitespaces": "selection",
  "whitespace_map": { // Which characters to show when `show_whitespaces` enabled
    "space": "•",
    "tab": "⟶"       // use "→", for a shorter arrow
  },

  "unnecessary_code_fade": 0.3, // How much to fade out unused code.

  // Hide the values of in variables from visual display in private files
  "redact_private_values": false,

  // Soft-wrap and rulers
  "soft_wrap": "none",          // none, editor_width, preferred_line_length, bounded
  "preferred_line_length": 80,  // Column to soft-wrap
  "show_wrap_guides": true,     // Show/hide wrap guides (vertical rulers)
  "wrap_guides": [],            // Where to position wrap_guides (character counts)

  // Gutter Settings
  "gutter": {
    "line_numbers": true,         // Show/hide line numbers in the gutter.
    "breakpoints": true,          // Show/hide show breakpoints in the gutter.
    "folds": true,                // Show/hide show fold buttons in the gutter.
    "min_line_number_digits": 4   // Reserve space for N digit line numbers
  },
  "relative_line_numbers": "enabled", // Show relative line numbers in gutter

  // Indent guides
  "indent_guides": {
    "enabled": true,
    "line_width": 1,                  // Width of guides in pixels [1-10]
    "active_line_width": 1,           // Width of active guide in pixels [1-10]
    "coloring": "fixed",              // disabled, fixed, indent_aware
    "background_coloring": "disabled" // disabled, indent_aware
  },

  "sticky_scroll": {
    "enabled": false // Whether to stick scopes to the top of the editor. Disabled by default.
  }
```

### Git Blame {#editor-blame}

```json [settings]
  "git": {
    "inline_blame": {
      "enabled": true,             // Show/hide inline blame
      "delay_ms": 0,               // Show after delay (ms)
      "location": "inline",        // inline, status_bar
      "min_column": 0,             // Minimum column to inline display blame
      "padding": 7,                // Padding between code and inline blame (em)
      "show_commit_summary": false // Show/hide commit summary
    },
    "hunk_style": "staged_hollow"  // staged_hollow, unstaged_hollow
  }
```

### Editor Toolbar

```json [settings]
  // Editor toolbar related settings
  "toolbar": {
    "breadcrumbs": true, // Whether to show breadcrumbs.
    "quick_actions": true, // Whether to show quick action buttons.
    "selections_menu": true, // Whether to show the Selections menu
    "agent_review": true, // Whether to show agent review buttons
    "code_actions": false // Whether to show code action buttons
  }
```

### Editor Scrollbar and Minimap {#editor-scrollbar}

```json [settings]
  // Scrollbar related settings
  "scrollbar": {
    // When to show the scrollbar in the editor (auto, system, always, never)
    "show": "auto",
    "cursors": true,          // Show cursor positions in the scrollbar.
    "git_diff": true,         // Show git diff indicators in the scrollbar.
    "search_results": true,   // Show buffer search results in the scrollbar.
    "selected_text": true,    // Show selected text occurrences in the scrollbar.
    "selected_symbol": true,  // Show selected symbol occurrences in the scrollbar.
    "diagnostics": "all",     // Show diagnostics (none, error, warning, information, all)
    "axes": {
      "horizontal": true,     // Show/hide the horizontal scrollbar
      "vertical": true        // Show/hide the vertical scrollbar
    }
  },

  // Minimap related settings
  "minimap": {
    "show": "never",                // When to show (auto, always, never)
    "display_in": "active_editor",  // Where to show (active_editor, all_editors)
    "thumb": "always",              // When to show thumb (always, hover)
    "thumb_border": "left_open",    // Thumb border (left_open, right_open, full, none)
    "max_width_columns": 80,        // Maximum width of minimap
    "current_line_highlight": null  // Highlight current line (null, line, gutter)
  },

  // Control Editor scroll beyond the last line: off, one_page, vertical_scroll_margin
  "scroll_beyond_last_line": "one_page",
  // Lines to keep above/below the cursor when scrolling with the keyboard
  "vertical_scroll_margin": 3,
  // The number of characters to keep on either side when scrolling with the mouse
  "horizontal_scroll_margin": 5,
  // Scroll sensitivity multiplier
  "scroll_sensitivity": 1.0,
  // Scroll sensitivity multiplier for fast scrolling (hold alt while scrolling)
  "fast_scroll_sensitivity": 4.0,
```

### Editor Tabs

```json [settings]
  // Maximum number of tabs per pane. Unset for unlimited.
  "max_tabs": null,

  // Customize the tab bar appearance
  "tab_bar": {
    "show": true,                     // Show/hide the tab bar
    "show_nav_history_buttons": true, // Show/hide history buttons on tab bar
    "show_tab_bar_buttons": true      // Show hide buttons (new, split, zoom)
  },
  "tabs": {
    "git_status": false,              // Color to show git status
    "close_position": "right",        // Close button position (left, right, hidden)
    "show_close_button": "hover",     // Close button shown (hover, always, hidden)
    "file_icons": false               // Icon showing file type
  }
```

### Status Bar

```json [settings]
  "status_bar": {
    // Show/hide a button that displays the active buffer's language.
    // Clicking the button brings up the language selector.
    // Defaults to true.
    "active_language_button": true,
    // Show/hide a button that displays the cursor's position.
    // Clicking the button brings up an input for jumping to a line and column.
    // Defaults to true.
    "cursor_position_button": true,
    // Show/hide a button that displays the buffer's character encoding.
    // If set to "non_utf8", the button is hidden only for UTF-8 without BOM.
    // Defaults to "non_utf8".
    "active_encoding_button": "non_utf8"
  },
```

### Multibuffer

```json [settings]
{
  // The default number of lines to expand excerpts in the multibuffer by.
  "expand_excerpt_lines": 5,
  // The default number of lines of context provided for excerpts in the multibuffer by.
  "excerpt_context_lines": 2
}
```

### Editor Completions, Snippets, Actions, Diagnostics {#editor-lsp}

```json [settings]
  "snippet_sort_order": "inline",        // Snippets completions: top, inline, bottom, none
  "show_completions_on_input": true,     // Show completions while typing
  "show_completion_documentation": true, // Show documentation in completions
  "auto_signature_help": false,          // Show method signatures inside parentheses

  // Whether to show the signature help after completion or a bracket pair inserted.
  // If `auto_signature_help` is enabled, this setting will be treated as enabled also.
  "show_signature_help_after_edits": false,

  // Which level to use to filter out diagnostics displayed in the editor:
  "diagnostics_max_severity": null,      // off, error, warning, info, hint, null (all)

  // When to show the scrollbar in the completion menu.
  "completion_menu_scrollbar": "never", // auto, system, always, never

  // How to display the LSP item kind (function, method, variable, etc.)
  // of each entry in the completions menu. "symbol" shows a syntax-colored
  // single-letter badge.
  "completion_menu_item_kind": "off", // off, symbol
  // Turn on colorization of brackets in editors (configurable per language)
  "colorize_brackets": true,
```

### Edit Predictions {#editor-ai}

```json [settings]
  "edit_predictions": {
    "mode": "eager"                  // Automatically show (eager) or hold-alt (subtle)
  },
  "show_edit_predictions": true     // Show/hide predictions in editor
```

### Other Panels

```json [settings]
  // Git Panel
  "git_panel": {
    "button": true,               // Show/hide status bar icon
    "dock": "left",               // Where to dock: left, right
    "default_width": 360,         // Default width of the git panel.
    "status_style": "icon",       // label_color, icon
    "sort_by": "path",            // path, name
    "group_by": "status",         // none, status, staging
    "scrollbar": {
      "show": null                // Show/hide: (auto, system, always, never)
    }
  },

  // Outline Panel
  "outline_panel": {
    "button": true,               // Show/hide status bar icon
    "default_width": 300,         // Default width of the git panel
    "dock": "left",               // Where to dock: left, right
    "file_icons": true,           // Show/hide file_icons
    "folder_icons": true,         // Show file_icons (true), chevrons (false) for dirs
    "git_status": true,           // Show git status
    "indent_size": 20,            // Indentation for nested items (pixels)
    "indent_guides": {
      "show": "always"            // Show indent guides (always, never)
    },
    "auto_reveal_entries": true,  // Show file in panel when activating its buffer
    "auto_fold_dirs": true,       // Fold dirs with single subdir
    "scrollbar": {                // Project panel scrollbar settings
      "show": null                // Show/hide: (auto, system, always, never)
    }
  }
```

## Collaboration Panels

```json [settings]
{
  // Collaboration Panel
  "collaboration_panel": {
    "button": true, // Show/hide status bar icon
    "dock": "left", // Where to dock: left, right
    "default_width": 240 // Default width of the collaboration panel.
  },
  "show_call_status_icon": true // Shown call status in the OS status bar.
}
```
