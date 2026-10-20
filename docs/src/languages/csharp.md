---
title: C#
description: "Configure C# language support in Zed, including formatting and syntax highlighting."
---

# C#

C# support is available through the [C# extension](https://github.com/zed-extensions/csharp).

Roslyn is enabled by default. To switch to csharp-ls or OmniSharp, add the following to your Zed settings file:

```json [settings]
{
}
```

Note: the language name used in settings is "CSharp", not "C#".

- Tree-sitter: [tree-sitter/tree-sitter-c-sharp](https://github.com/tree-sitter/tree-sitter-c-sharp)
