---
title: Svelte
description: "Configure Svelte language support in Zed, including formatting and syntax highlighting."
---

# Svelte

Svelte support is available through the [Svelte extension](https://github.com/zed-extensions/svelte).

- Tree-sitter: [tree-sitter-grammars/tree-sitter-svelte](https://github.com/tree-sitter-grammars/tree-sitter-svelte)

## Extra theme styling configuration

You can modify how certain styles, such as directives and modifiers, appear in attributes:

```json
"syntax": {
  // Styling for directives (e.g., `class:foo` or `on:click`) (the `on` or `class` part of the attribute).
  "attribute.function": {
    "color": "#ff0000"
  },
  // Styling for modifiers at the end of attributes, e.g. `on:<click|preventDefault|stopPropagation>`
  "attribute.special": {
    "color": "#00ff00"
  }
}
```
