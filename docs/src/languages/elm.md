---
title: Elm
description: "Configure Elm language support in Zed, including formatting and syntax highlighting."
---

# Elm

Elm support is available through the [Elm extension](https://github.com/zed-extensions/elm).

- Tree-sitter: [elm-tooling/tree-sitter-elm](https://github.com/elm-tooling/tree-sitter-elm)

## Setup

Zed support for Elm requires installation of `elm`, `elm-format`, and `elm-review`.

1. [Install Elm](https://guide.elm-lang.org/install/elm.html) (or run `brew install elm` on macOS).
2. Install `elm-review` to support code linting:
   ```sh
   npm install elm-review --save-dev
   ```
3. Install `elm-format` to support automatic formatting
   ```sh
   npm install -g elm-format
   ```
