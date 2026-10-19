---
title: YAML
description: "Configure YAML language support in Zed, including formatting and syntax highlighting."
---

# YAML

YAML support is available natively in Zed.

- Tree-sitter: [zed-industries/tree-sitter-yaml](https://github.com/zed-industries/tree-sitter-yaml)

## Formatting

By default, Zed uses Prettier for formatting YAML files.

### Prettier Formatting

You can customize the formatting behavior of Prettier. For example to use single-quotes in yaml files add the following to your `.prettierrc` configuration file:

```json
{
  "overrides": [
    {
      "files": ["*.yaml", "*.yml"],
      "options": {
        "singleQuote": false
      }
    }
  ]
}
```
