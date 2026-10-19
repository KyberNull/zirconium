---
title: Nim
description: "Configure Nim language support in Zed, including formatting and syntax highlighting."
---

# Nim

Nim language support in Zed is provided by the community-maintained [Nim extension](https://github.com/foxoman/zed-nim).
Report issues to: [https://github.com/foxoman/zed-nim/issues](https://github.com/foxoman/zed-nim/issues)

- Tree-sitter: [alaviss/tree-sitter-nim](https://github.com/alaviss/tree-sitter-nim)

## Formatting

To use [arnetheduck/nph](https://github.com/arnetheduck/nph) as a formatter, follow the [nph installation instructions](https://github.com/arnetheduck/nph?tab=readme-ov-file#installation).

Configure formatting in Settings ({#kb zed::OpenSettings}) under Languages > Nim, or add to your settings file:

```json [settings]
  "languages": {
    "Nim": {
      "formatter": {
        "external": {
          "command": "nph",
          "arguments": ["-"]
        }
      }
    }
  }
```
