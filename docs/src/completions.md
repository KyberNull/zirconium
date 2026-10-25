---
title: Code Completions - Zed
description: Zed's word and snippet completions and edit predictions. Configure autocomplete behavior, snippets, and documentation display.
---

# Completions

Zed supports two sources for completions:

1. "Code Completions" from the words in your open buffers and from [snippets](./snippets.md).
2. "Edit Predictions" provided by external providers like [GitHub Copilot](#github-copilot) or by a local model.

## Code Completions {#code-completions}

Zed provides completions of words found in the current file. You can disable these by adding the following to your Zed `settings.json` file:

```json [settings]
"show_completions_on_input": false
```

You can manually trigger completions with `ctrl-space` or by triggering the `editor::ShowCompletions` action from the command palette.

> Note: Using `ctrl-space` in Zed requires disabling the macOS global shortcut.
> Open **System Settings** > **Keyboard** > **Keyboard Shortcut**s >
> **Input Sources** and uncheck **Select the previous input source**.

For more information, see:

- [Configuring Supported Languages](./configuring-languages.md)
- [List of Zed Supported Languages](./languages.md)

## Edit Predictions {#edit-predictions}

Zed supports predicting multiple edits at a time through the configured edit prediction provider.
Edit predictions appear as you type, and most of the time, you can accept them by pressing `tab`.

See the [edit predictions documentation](./ai/edit-prediction.md) for more information on how to setup and configure Zed's edit predictions.
