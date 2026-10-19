---
title: Deno
description: "Configure Deno language support in Zed, including formatting and syntax highlighting."
---

# Deno

Deno support is available through the [Deno extension](https://github.com/zed-extensions/deno).

## Runnable support

To run deno tasks like tests from the ui, add this to `.zed/tasks.json`

```json [tasks]
[
  {
    "label": "deno test",
    "command": "deno test -A $ZED_FILE",
    "tags": ["js-test"]
  }
]
```

## See also:

- [TypeScript](./typescript.md)
- [JavaScript](./javascript.md)
