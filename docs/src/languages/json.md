---
title: JSON
description: "Configure JSON language support in Zed, including formatting and syntax highlighting."
---

# JSON

JSON support is available natively in Zed.

- Tree-sitter: [tree-sitter/tree-sitter-json](https://github.com/tree-sitter/tree-sitter-json)

## JSONC

Zed also supports a super-set of JSON called JSONC, which allows single line comments (`//`) in JSON files.
While editing these files you can use `cmd-/` (macOS) or `ctrl-/` (Linux) to toggle comments on the current line or selection.

## JSONC Prettier Formatting

If you use files with the `*.jsonc` extension when using `Format Document` or have `format_on_save` enabled, Zed invokes Prettier as the formatter. Prettier has an [outstanding issue](https://github.com/prettier/prettier/issues/15956) where it will add trailing commas to files with a `jsonc` extension. JSONC files which have a `.json` extension are unaffected.

To workaround this behavior you can add the following to your `.prettierrc` configuration file:

```json
{
  "overrides": [
    {
      "files": ["*.jsonc"],
      "options": {
        "parser": "json",
        "trailingComma": "none"
      }
    }
  ]
}
```
