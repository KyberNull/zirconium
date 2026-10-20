---
title: HTML
description: "Configure HTML language support in Zed, including formatting and syntax highlighting."
---

# HTML

HTML support is available through the [HTML extension](https://github.com/zed-industries/zed/tree/main/extensions/html).

This extension is automatically installed, but if you do not want to use it, you can add the following to your settings:

```json [settings]
{
}
```

- Tree-sitter: [tree-sitter/tree-sitter-html](https://github.com/tree-sitter/tree-sitter-html)

## Formatting

By default Zed uses [Prettier](https://prettier.io/) for formatting HTML.

Configure formatting in Settings ({#kb zed::OpenSettings}) under Languages > HTML, or add to your settings file:

```json [settings]
  "languages": {
    "HTML": {
      "format_on_save": "off",
    }
  }
```

You can still trigger formatting manually with {#kb editor::Format} or by opening the [command palette](..//getting-started.md#command-palette) ({#kb command_palette::Toggle}) and selecting "Format Document".

## See also

- [CSS](./css.md)
- [JavaScript](./javascript.md)
- [TypeScript](./typescript.md)
