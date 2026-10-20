---
title: JavaScript
description: "Configure JavaScript language support in Zed, including formatting and syntax highlighting."
---

# JavaScript

JavaScript support is available natively in Zed.

- Tree-sitter: [tree-sitter/tree-sitter-javascript](https://github.com/tree-sitter/tree-sitter-javascript)

## Code formatting

Formatting on save is disabled by default for JavaScript.

To turn `format-on-save` on, you can enable the setting globally or for `JavaScript` or `TypeScript` specifically.

```json [settings]
{
  "languages": {
    "JavaScript": {
      "format_on_save": "on"
    },
    "TypeScript": {
      "format_on_save": "on"
    }
  }
}
```

Many JavaScript projects use command-line code-formatting tools, such as [Prettier](https://prettier.io/).
You can use one of these tools by specifying an _external_ code formatter for JavaScript in your settings.
See [the configuration docs](../reference/all-settings.md) for more information.

For example, if you have Prettier installed and on your `PATH`, you can use it to format JavaScript files.

Configure formatting in Settings ({#kb zed::OpenSettings}) under Languages > JavaScript, or add to your settings file:

```json [settings]
{
  "languages": {
    "JavaScript": {
      "formatter": {
        "external": {
          "command": "prettier",
          "arguments": ["--stdin-filepath", "{buffer_path}"]
        }
      }
    }
  }
}
```

## JSDoc

Zed supports JSDoc syntax in JavaScript and TypeScript comments that match the JSDoc syntax.
Zed uses [tree-sitter/tree-sitter-jsdoc](https://github.com/tree-sitter/tree-sitter-jsdoc) for parsing and highlighting JSDoc.

## See also

- [Yarn documentation](./yarn.md) for a walkthrough of configuring your project to use Yarn.
- [TypeScript documentation](./typescript.md)
