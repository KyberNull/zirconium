---
title: Tree-sitter Language Config - Zed
description: Configure language support in Zed with Tree-sitter for syntax highlighting and structure, and Prettier or external formatters for formatting.
---

# Configuring Supported Languages

Zed's language support is built on **Tree-sitter**, which handles syntax highlighting and structure-based features like the outline panel.

This page covers language-specific settings, file associations, formatting, and syntax highlighting.

For a list of supported languages, see [Supported Languages](./languages.md). To add support for new languages, see [Language Extensions](./extensions/languages.md).

## Language-specific Settings

Zed allows you to override global settings for individual languages. These custom configurations are defined in your `settings.json` file under the `languages` key.

Here's an example of language-specific settings:

```json [settings]
"languages": {
  "Python": {
    "tab_size": 4,
    "formatter": "language_server",
    "format_on_save": "on"
  },
  "JavaScript": {
    "tab_size": 2,
    "formatter": {
      "external": {
        "command": "prettier",
        "arguments": ["--stdin-filepath", "{buffer_path}"]
      }
    }
  }
}
```

You can customize a wide range of settings for each language, including:

- [`tab_size`](./reference/all-settings.md#tab-size): The number of spaces for each indentation level
- [`formatter`](./reference/all-settings.md#formatter): The tool used for code formatting
- [`format_on_save`](./reference/all-settings.md#format-on-save): Whether to automatically format code when saving
- [`hard_tabs`](./reference/all-settings.md#hard-tabs): Use tabs instead of spaces for indentation
- [`preferred_line_length`](./reference/all-settings.md#preferred-line-length): The recommended maximum line length
- [`soft_wrap`](./reference/all-settings.md#soft-wrap): How to wrap long lines of code
- [`show_completions_on_input`](./reference/all-settings.md#show-completions-on-input): Whether or not to show completions as you type
- [`show_completion_documentation`](./reference/all-settings.md#show-completion-documentation): Whether to display inline and alongside documentation for items in the completions menu
- [`colorize_brackets`](./reference/all-settings.md#colorize-brackets): Whether to use tree-sitter bracket queries to detect and colorize the brackets in the editor (also known as "rainbow brackets")

These settings allow you to maintain specific coding styles across different languages and projects.

## File Associations

Zed automatically detects file types based on their extensions, but you can customize these associations to fit your workflow.

To set up custom file associations, use the [`file_types`](./reference/all-settings.md#file-types) setting in your `settings.json`:

```json [settings]
"file_types": {
  "C++": ["c"],
  "TOML": ["MyLockFile"],
  "Dockerfile": ["Dockerfile*"]
}
```

This configuration tells Zed to:

- Treat `.c` files as C++ instead of C
- Recognize files named "MyLockFile" as TOML
- Apply Dockerfile syntax to any file starting with "Dockerfile"

You can use glob patterns for more flexible matching, allowing you to handle complex naming conventions in your projects.

## Formatting

Zed provides support for code formatting to maintain consistent code style.

### Configuring Formatters

Zed supports both built-in and external formatters. See [`formatter`](./reference/all-settings.md#formatter) docs for more. You can configure formatters globally or per-language in your `settings.json`:

```json [settings]
"languages": {
  "JavaScript": {
    "formatter": {
      "external": {
        "command": "prettier",
        "arguments": ["--stdin-filepath", "{buffer_path}"]
      }
    },
    "format_on_save": "on"
  },
  "TypeScript": {
    "formatter": "prettier",
    "format_on_save": "on"
  }
}
```

This example uses an external Prettier command for JavaScript and Zed's Prettier integration for TypeScript, both set to format on save.

To disable formatting for a specific language:

```json [settings]
"languages": {
  "Markdown": {
    "format_on_save": "off"
  }
}
```

### Formatting Selections

Zed supports formatting only the selected text via {#action editor::FormatSelections} ({#kb editor::FormatSelections}). How
this works depends on the configured formatter:

- The action is only shown when the active formatter can actually format ranges for at least one
  selected buffer.
- **Prettier**: Uses Prettier's built-in range formatting to format the encompassing range of all selections. Any
  resulting edits that fall outside the selected ranges are discarded, so only the selected code is modified.
- **External commands**: External command formatters do not support range formatting and are skipped when formatting
  selections.

### Troubleshooting

If you encounter issues with formatting:

1. Check Zed's log file for error messages (Use the command palette: {#action zed::OpenLog})
2. Ensure external tools (formatters) are correctly installed and in your PATH
3. Verify configurations in both Zed settings and language-specific config files (e.g., `.prettierrc`)

## Syntax Highlighting and Themes

Zed offers customization options for syntax highlighting and themes, allowing you to tailor the visual appearance of your code.

### Customizing Syntax Highlighting

Zed uses Tree-sitter grammars for syntax highlighting. Override the default highlighting using the `theme_overrides` setting.

This example makes comments italic and changes the color of strings:

```json [settings]
"theme_overrides": {
  "One Dark": {
    "syntax": {
      "comment": {
        "font_style": "italic"
      },
      "string": {
        "color": "#00AA00"
      }
    }
  }
}
```

### Selecting and Customizing Themes

Change your theme:

1. Use the theme selector ({#kb theme_selector::Toggle})
2. Or set it in your `settings.json`:

```json [settings]
"theme": {
  "mode": "dark",
  "dark": "One Dark",
  "light": "GitHub Light"
}
```

Create custom themes by creating a JSON file in `~/.config/zed/themes/`. Zed will automatically detect and make available any themes in this directory.

### Using Theme Extensions

Zed supports theme extensions. Browse and install theme extensions from the Extensions panel ({#kb zed::Extensions}).

To create your own theme extension, refer to the [Developing Theme Extensions](./extensions/themes.md) guide.
