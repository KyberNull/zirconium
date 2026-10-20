---
title: Lua
description: "Configure Lua language support in Zed, including formatting and syntax highlighting."
---

# Lua

Lua support is available through the [Lua extension](https://github.com/zed-extensions/lua).

- Tree-sitter: [tree-sitter-grammars/tree-sitter-lua](https://github.com/tree-sitter-grammars/tree-sitter-lua)

## luarc.json

To configure LuaLS you can create a `.luarc.json` file in the root of your project.

```json
{
  "$schema": "https://raw.githubusercontent.com/LuaLS/vscode-lua/master/setting/schema.json",
  "runtime.version": "Lua 5.4",
  "format.enable": true,
  "workspace.library": ["../somedir/library"]
}
```

See [LuaLS Settings Documentation](https://luals.github.io/wiki/settings/) for all available configuration options, or when editing this file in Zed available settings options will autocomplete, (e.g `runtime.version` will show `"Lua 5.1"`, `"Lua 5.2"`, `"Lua 5.3"`, `"Lua 5.4"` and `"LuaJIT"` as allowed values). Note when importing settings options from VS Code, remove the `Lua.` prefix. (e.g. `runtime.version` instead of `Lua.runtime.version`).

### LÖVE (Love2D) {#love2d}

To use [LÖVE (Love2D)](https://love2d.org/) in Zed, checkout [LuaCATS/love2d](https://github.com/LuaCATS/love2d) into a folder called `love2d-luacats` into the parent folder of your project:

```sh
cd .. && git clone https://github.com/LuaCATS/love2d love2d-luacats
```

Then in your `.luarc.json`:

```
{
  "$schema": "https://raw.githubusercontent.com/LuaLS/vscode-lua/master/setting/schema.json",
  "runtime.version": "Lua 5.4",
  "workspace.library": ["../love2d-luacats"],
  "runtime.special": {
    "love.filesystem.load": "loadfile"
  }
}
```

## Formatting

### StyLua Formatting

Alternatively to use [StyLua](https://github.com/JohnnyMorganz/StyLua) for auto-formatting:

1. Install [StyLua](https://github.com/JohnnyMorganz/StyLua): `brew install stylua` or `cargo install stylua --features lua52,lua53,lua54,luau,luajit` (feel free to remove any Lua versions you don't need).
2. Configure formatting in Settings ({#kb zed::OpenSettings}) under Languages > Lua, or add to your settings file:

```json [settings]
{
  "languages": {
    "Lua": {
      "format_on_save": "on",
      "formatter": {
        "external": {
          "command": "stylua",
          "arguments": [
            "--syntax=Lua54",
            "--respect-ignores",
            "--stdin-filepath",
            "{buffer_path}",
            "-"
          ]
        }
      }
    }
  }
}
```

You can specify various options to StyLua either on the command line above (like `--syntax=Lua54`) or in a `stylua.toml` in your project:

```toml
syntax = "Lua54"
column_width = 100
line_endings = "Unix"
indent_type = "Spaces"
indent_width = 4
quote_style = "AutoPreferDouble"
call_parentheses = "Always"
collapse_simple_statement = "All"

[sort_requires]
enabled = true
```

For a complete list of available options, see: [StyLua Options](https://github.com/JohnnyMorganz/StyLua?tab=readme-ov-file#options).
