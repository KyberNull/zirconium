---
title: Ruby
description: "Configure Ruby language support in Zed, including formatting and syntax highlighting."
---

# Ruby

Ruby support is available through the [Ruby extension](https://github.com/zed-extensions/ruby).

The Ruby extension also provides support for ERB files.

- Tree-sitters:
  - [tree-sitter-ruby](https://github.com/tree-sitter/tree-sitter-ruby)
  - [tree-sitter-embedded-template](https://github.com/tree-sitter/tree-sitter-embedded-template)

## Setting up Herb

`Herb` is enabled by default for the `HTML+ERB` language.

## Running tests

To run tests in your Ruby project, you can set up custom tasks in your local `.zed/tasks.json` configuration file. These tasks can be defined to work with different test frameworks like Minitest, RSpec, quickdraw, and tldr. Below are some examples of how to set up these tasks to run your tests from within your editor.

### Minitest with Rails

```json [tasks]
[
  {
    "label": "test $ZED_RELATIVE_FILE -n /$ZED_CUSTOM_RUBY_TEST_NAME/",
    "command": "bin/rails",
    "args": [
      "test",
      "$ZED_RELATIVE_FILE",
      "-n",
      "\"$ZED_CUSTOM_RUBY_TEST_NAME\""
    ],
    "cwd": "$ZED_WORKTREE_ROOT",
    "tags": ["ruby-test"]
  }
]
```

### Minitest

Plain minitest does not support running tests by line number, only by name, so we need to use `$ZED_CUSTOM_RUBY_TEST_NAME` instead:

```json [tasks]
[
  {
    "label": "-Itest $ZED_RELATIVE_FILE -n /$ZED_CUSTOM_RUBY_TEST_NAME/",
    "command": "bundle",
    "args": [
      "exec",
      "ruby",
      "-Itest",
      "$ZED_RELATIVE_FILE",
      "-n",
      "\"$ZED_CUSTOM_RUBY_TEST_NAME\""
    ],
    "cwd": "$ZED_WORKTREE_ROOT",
    "tags": ["ruby-test"]
  }
]
```

### RSpec

```json [tasks]
[
  {
    "label": "test $ZED_RELATIVE_FILE:$ZED_ROW",
    "command": "bundle",
    "args": ["exec", "rspec", "\"$ZED_RELATIVE_FILE:$ZED_ROW\""],
    "cwd": "$ZED_WORKTREE_ROOT",
    "tags": ["ruby-test"]
  }
]
```

Similar task syntax can be used for other test frameworks such as `quickdraw` or `tldr`.

## Formatters

### `erb-formatter`

To format ERB templates, you can use the `erb-formatter` formatter. This formatter uses the [`erb-formatter`](https://rubygems.org/gems/erb-formatter) gem to format ERB templates.

Configure formatting in Settings ({#kb zed::OpenSettings}) under Languages > HTML+ERB, or add to your settings file:

```json [settings]
{
  "languages": {
    "HTML+ERB": {
      "formatter": {
        "external": {
          "command": "erb-formatter",
          "arguments": ["--stdin-filename", "{buffer_path}"]
        }
      }
    }
  }
}
```
