---
title: Scala
description: "Configure Scala language support in Zed, including formatting and syntax highlighting."
---

# Scala

Scala language support in Zed is provided by the community-maintained [Scala extension](https://github.com/scalameta/metals-zed).
Report issues to: [https://github.com/scalameta/metals-zed/issues](https://github.com/scalameta/metals-zed/issues)

- Tree-sitter: [tree-sitter/tree-sitter-scala](https://github.com/tree-sitter/tree-sitter-scala)

## Setup

- Install Scala with `cs setup` (Coursier): https://www.scala-lang.org/download/
  - `brew install coursier/formulas/coursier && cs setup`
- REPL (Almond) Setup Instructions https://almond.sh/docs/quick-start-install
  - `brew install --cask temurin` (Eclipse foundation official OpenJDK binaries)
  - `brew install coursier/formulas/coursier && cs setup`
  - `coursier launch --use-bootstrap almond -- --install`
