---
title: GDScript
description: "Configure GDScript language support in Zed, including formatting and syntax highlighting."
---

# GDScript

Godot [GDScript](https://gdscript.com/) language support in Zed is provided by the community-maintained [GDScript extension](https://github.com/GDQuest/zed-gdscript).
Report issues to: [https://github.com/GDQuest/zed-gdscript/issues](https://github.com/GDQuest/zed-gdscript/issues)

- Tree-sitter: [PrestonKnopp/tree-sitter-gdscript](https://github.com/PrestonKnopp/tree-sitter-gdscript) and [PrestonKnopp/tree-sitter-godot-resource](https://github.com/PrestonKnopp/tree-sitter-godot-resource)

## Pre-requisites

You will need:

- [Godot](https://godotengine.org/download/).
- netcat (`nc` or `ncat`) on your system PATH.

## Setup

1. Inside your Godot editor, open Editor Settings, look for `Text Editor -> External` and set the following options:
   - Exec Path: `/path/to/zed`
   - Exec Flags: `{project} {file}:{line}:{col}`
   - Use External Editor: "✅ On"
2. Open any \*.gd file through Godot and Zed will launch.
