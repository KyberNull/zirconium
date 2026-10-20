---
title: Helm
description: "Configure Helm language support in Zed, including formatting and syntax highlighting."
---

# Helm

Support for Helm in Zed is provided by the community-maintained [Helm extension](https://github.com/cabrinha/helm.zed).

- Tree-sitter: [tree-sitter-go-template](https://github.com/ngalaiko/tree-sitter-go-template/tree/master)

## Setup

Enable Helm language for Helm files by editing your `.zed/settings.json` and adding:

```json [settings]
  "file_types": {
    "Helm": [
      "**/templates/**/*.tpl",
      "**/templates/**/*.yaml",
      "**/templates/**/*.yml",
      "**/helmfile.d/**/*.yaml",
      "**/helmfile.d/**/*.yml",
      "**/values*.yaml"
    ]
  }
```

This will also mark values.yaml files as the type helm, since helm-ls supports this.
