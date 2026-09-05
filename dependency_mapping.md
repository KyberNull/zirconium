# Purged Crates Dependency Reference List

This document provides a complete, structured inventory of all locations across the repository where crates specified for removal in `PURGE_MANIFEST.md` are referenced as dependencies, workspace members, code imports, configurations, keybinds, and documentation.

---

## A. IDE Debugging & Code Execution Tooling
### `dap`
- **Category**: A. IDE Debugging & Code Execution Tooling
- **Primary Crate Location**: `crates/dap`
- **Path Status**: `Present`

**Dependencies & References Found:**

#### Manifest Declarations (`Cargo.toml`)
- `Cargo.toml:47:    "crates/dap",`
- `Cargo.toml:320:dap = { path = "crates/dap" }`
- `Cargo.toml:593:dap-types = { git = "https://github.com/zed-industries/dap-types", rev = "1b461b310481d01e02b2603c16d7144b926339f8" }`

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:25:| `crates/dap` | Debug Adapter Protocol client | Not needed for Markdown document editing. |`
- `plan.md:288:- `lsp`, `lsp_locations`, `dap`, `dap_adapters`, `debugger_*`, `terminal`, `terminal_view`, `repl`, `task`, `tasks_ui`, `runnable``

---

### `dap_adapters`
- **Category**: A. IDE Debugging & Code Execution Tooling
- **Primary Crate Location**: `crates/dap_adapters`
- **Path Status**: `Present`

**Dependencies & References Found:**

#### Manifest Declarations (`Cargo.toml`)
- `Cargo.toml:48:    "crates/dap_adapters",`
- `Cargo.toml:321:dap_adapters = { path = "crates/dap_adapters" }`

#### Documentation References (`.md`)
- `plan.md:288:- `lsp`, `lsp_locations`, `dap`, `dap_adapters`, `debugger_*`, `terminal`, `terminal_view`, `repl`, `task`, `tasks_ui`, `runnable``
- `PURGE_MANIFEST.md:26:| `crates/dap_adapters` | Pre-configured DAP configurations (gdb, lldb, debugpy) | Pure IDE debugger infrastructure. |`

---

### `debug_adapter_extension`
- **Category**: A. IDE Debugging & Code Execution Tooling
- **Primary Crate Location**: `crates/debug_adapter_extension`
- **Path Status**: `Present`

**Dependencies & References Found:**

#### Manifest Declarations (`Cargo.toml`)
- `Cargo.toml:50:    "crates/debug_adapter_extension",`
- `Cargo.toml:323:debug_adapter_extension = { path = "crates/debug_adapter_extension" }`

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:27:| `crates/debug_adapter_extension` | Extension integration for custom debug adapters | Not applicable to note vault management. |`

---

### `debugger_tools`
- **Category**: A. IDE Debugging & Code Execution Tooling
- **Primary Crate Location**: `crates/debugger_tools`
- **Path Status**: `Present`

**Dependencies & References Found:**

#### Manifest Declarations (`Cargo.toml`)
- `Cargo.toml:51:    "crates/debugger_tools",`
- `Cargo.toml:324:debugger_tools = { path = "crates/debugger_tools" }`

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:28:| `crates/debugger_tools` | Tools panel for debugger state | IDE UI panel unneeded in Obsidian. |`

---

### `debugger_ui`
- **Category**: A. IDE Debugging & Code Execution Tooling
- **Primary Crate Location**: `crates/debugger_ui`
- **Path Status**: `Present`

**Dependencies & References Found:**

#### Manifest Declarations (`Cargo.toml`)
- `Cargo.toml:52:    "crates/debugger_ui",`
- `Cargo.toml:325:debugger_ui = { path = "crates/debugger_ui" }`

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:29:| `crates/debugger_ui` | Visual breakpoint, stack frame, and variable panels | Replaced by Note Backlinks and Graph views. |`

---

### `terminal`
- **Category**: A. IDE Debugging & Code Execution Tooling
- **Primary Crate Location**: `crates/terminal`
- **Path Status**: `Present`

**Dependencies & References Found:**

#### Manifest Declarations (`Cargo.toml`)
- `Cargo.toml:210:    "crates/terminal",`
- `Cargo.toml:475:terminal = { path = "crates/terminal" }`

#### Documentation References (`.md`)
- `plan.md:288:- `lsp`, `lsp_locations`, `dap`, `dap_adapters`, `debugger_*`, `terminal`, `terminal_view`, `repl`, `task`, `tasks_ui`, `runnable``
- `PURGE_MANIFEST.md:11:2. **Purge IDE & Development Bloat**: Eliminate Debug Adapters (DAP), Language Server Protocol (LSP), embedded terminal emulators, REPLs, remote SSH/WebRTC collaboration (`collab`, `livekit`), and 50+ programming language syntax grammars.`
- `PURGE_MANIFEST.md:30:| `crates/terminal` | Embedded terminal emulator (Alacritty / portable-pty) | Heavy native dependency (`alacritty_terminal`, pty forks) unrelated to markdown notes. |`
- `PURGE_MANIFEST.md:31:| `crates/terminal_view` | GPUI view for rendering shell terminals | IDE terminal panel; purge to reduce UI clutter and dependency bloat. |`

---

### `terminal_view`
- **Category**: A. IDE Debugging & Code Execution Tooling
- **Primary Crate Location**: `crates/terminal_view`
- **Path Status**: `Present`

**Dependencies & References Found:**

#### Manifest Declarations (`Cargo.toml`)
- `Cargo.toml:211:    "crates/terminal_view",`
- `Cargo.toml:476:terminal_view = { path = "crates/terminal_view" }`

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:31:| `crates/terminal_view` | GPUI view for rendering shell terminals | IDE terminal panel; purge to reduce UI clutter and dependency bloat. |`
- `plan.md:288:- `lsp`, `lsp_locations`, `dap`, `dap_adapters`, `debugger_*`, `terminal`, `terminal_view`, `repl`, `task`, `tasks_ui`, `runnable``

---

### `repl`
- **Category**: A. IDE Debugging & Code Execution Tooling
- **Primary Crate Location**: `crates/repl`
- **Path Status**: `Present`

**Dependencies & References Found:**

#### Manifest Declarations (`Cargo.toml`)
- `Cargo.toml:177:    "crates/repl",`
- `Cargo.toml:442:repl = { path = "crates/repl" }`

#### Documentation References (`.md`)
- `plan.md:288:- `lsp`, `lsp_locations`, `dap`, `dap_adapters`, `debugger_*`, `terminal`, `terminal_view`, `repl`, `task`, `tasks_ui`, `runnable``
- `PURGE_MANIFEST.md:32:| `crates/repl` | Read-Eval-Print Loop integration for Jupyter/Python | Code evaluation tool; unneeded in note vault. |`

---

### `shell_command_parser`
- **Category**: A. IDE Debugging & Code Execution Tooling
- **Primary Crate Location**: `crates/shell_command_parser`
- **Path Status**: `Present`

**Dependencies & References Found:**

#### Manifest Declarations (`Cargo.toml`)
- `Cargo.toml:192:    "crates/shell_command_parser",`
- `Cargo.toml:457:shell_command_parser = { path = "crates/shell_command_parser" }`

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:33:| `crates/shell_command_parser` | Parsing bash/zsh command strings for tasks | IDE shell command execution helper. |`

---

### `task`
- **Category**: A. IDE Debugging & Code Execution Tooling
- **Primary Crate Location**: `crates/task`
- **Path Status**: `Present`

**Dependencies & References Found:**

#### Manifest Declarations (`Cargo.toml`)
- `Cargo.toml:206:    "crates/task",`
- `Cargo.toml:471:task = { path = "crates/task" }`
- `Cargo.toml:544:async-task = "4.7"`
- `Cargo.toml:971:async-task = { git = "https://github.com/smol-rs/async-task.git", rev = "b4486cd71e4e94fbda54ce6302444de14f4d190e" }`

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:34:| `crates/task` | Task system for running builds/scripts (`cargo check`, `npm test`) | Unnecessary for markdown vault workflows. |`
- `plan.md:66:- **Cooperative Task Execution**: Zed uses GPUI's built-in background executor ([executor.rs](file://crates/gpui/src/executor.rs)) built on top of `smol` and `async-task`.`
- `plan.md:288:- `lsp`, `lsp_locations`, `dap`, `dap_adapters`, `debugger_*`, `terminal`, `terminal_view`, `repl`, `task`, `tasks_ui`, `runnable``
- `gpui_notes.md:77:3. **`Task<T>`**: A future handle returned by `cx.spawn(...)` or `cx.background_spawn(...)`. If dropped without calling `.detach()`, the task is automatically cancelled.`
- `gpui_notes.md:161:4. [ ] **Async Runtime**: Build foreground/background task dispatchers on top of standard thread pools and event queues.`

---

### `tasks_ui`
- **Category**: A. IDE Debugging & Code Execution Tooling
- **Primary Crate Location**: `crates/tasks_ui`
- **Path Status**: `Present`

**Dependencies & References Found:**

#### Manifest Declarations (`Cargo.toml`)
- `Cargo.toml:207:    "crates/tasks_ui",`
- `Cargo.toml:472:tasks_ui = { path = "crates/tasks_ui" }`

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:35:| `crates/tasks_ui` | Task runner selector modal and status bar | IDE build runner UI. |`
- `plan.md:288:- `lsp`, `lsp_locations`, `dap`, `dap_adapters`, `debugger_*`, `terminal`, `terminal_view`, `repl`, `task`, `tasks_ui`, `runnable``

---

### `runnable`
- **Category**: A. IDE Debugging & Code Execution Tooling
- **Primary Crate Location**: `crates/runnable`
- **Path Status**: `Pruned / Not Present`

**Dependencies & References Found:**

#### Documentation References (`.md`)
- `plan.md:288:- `lsp`, `lsp_locations`, `dap`, `dap_adapters`, `debugger_*`, `terminal`, `terminal_view`, `repl`, `task`, `tasks_ui`, `runnable``
- `PURGE_MANIFEST.md:36:| `crates/runnable` | Detecting runnable code main functions and tests | Code runner detection; irrelevant for `.md` files. |`

---

## B. Language Server Protocol (LSP) & Code Tooling
### `lsp`
- **Category**: B. Language Server Protocol (LSP) & Code Tooling
- **Primary Crate Location**: `crates/lsp`
- **Path Status**: `Present`

**Dependencies & References Found:**

#### Manifest Declarations (`Cargo.toml`)
- `Cargo.toml:132:    "crates/lsp",`
- `Cargo.toml:398:lsp = { path = "crates/lsp" }`
- `Cargo.toml:678:lsp-types = { git = "https://github.com/zed-industries/lsp-types", rev = "f4dfa89a21ca35cd929b70354b1583fabae325f8" }`

#### Documentation References (`.md`)
- `plan.md:288:- `lsp`, `lsp_locations`, `dap`, `dap_adapters`, `debugger_*`, `terminal`, `terminal_view`, `repl`, `task`, `tasks_ui`, `runnable``
- `CONTRIBUTING.md:175:- [`lsp`](/crates/lsp) handles communication with external LSP server.`
- `PURGE_MANIFEST.md:45:| `crates/lsp` | LSP client implementation (JSON-RPC protocol for diagnostics, completion) | Replaced by internal SQLite Wikilink & Tag indexer. |`

---

### `lsp_locations`
- **Category**: B. Language Server Protocol (LSP) & Code Tooling
- **Primary Crate Location**: `crates/lsp_locations`
- **Path Status**: `Present`

**Dependencies & References Found:**

#### Manifest Declarations (`Cargo.toml`)
- `Cargo.toml:133:    "crates/lsp_locations",`
- `Cargo.toml:399:lsp_locations = { path = "crates/lsp_locations" }`

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:46:| `crates/lsp_locations` | Mapping LSP symbol locations to buffer coordinates | Code symbol navigation unneeded for markdown files. |`
- `plan.md:288:- `lsp`, `lsp_locations`, `dap`, `dap_adapters`, `debugger_*`, `terminal`, `terminal_view`, `repl`, `task`, `tasks_ui`, `runnable``

---

### `prettier`
- **Category**: B. Language Server Protocol (LSP) & Code Tooling
- **Primary Crate Location**: `crates/prettier`
- **Path Status**: `Present`

**Dependencies & References Found:**

#### Manifest Declarations (`Cargo.toml`)
- `Cargo.toml:162:    "crates/prettier",`
- `Cargo.toml:429:prettier = { path = "crates/prettier" }`
- `Cargo.toml:1045:prettier = { codegen-units = 1 }`

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:47:| `crates/prettier` | Node.js Prettier formatting integration | Formats JS/TS/HTML code; markdown formatting will use native rules. |`

---

### `toolchain_selector`
- **Category**: B. Language Server Protocol (LSP) & Code Tooling
- **Primary Crate Location**: `crates/toolchain_selector`
- **Path Status**: `Present`

**Dependencies & References Found:**

#### Manifest Declarations (`Cargo.toml`)
- `Cargo.toml:220:    "crates/toolchain_selector",`
- `Cargo.toml:484:toolchain_selector = { path = "crates/toolchain_selector" }`

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:48:| `crates/toolchain_selector` | UI widget for choosing Python venvs / Rust toolchains | IDE toolchain menu unneeded in note workspace. |`

---

### `encoding_selector`
- **Category**: B. Language Server Protocol (LSP) & Code Tooling
- **Primary Crate Location**: `crates/encoding_selector`
- **Path Status**: `Present`

**Dependencies & References Found:**

#### Manifest Declarations (`Cargo.toml`)
- `Cargo.toml:65:    "crates/encoding_selector",`
- `Cargo.toml:336:encoding_selector = { path = "crates/encoding_selector" }`

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:49:| `crates/encoding_selector` | Selecting legacy file encodings (Shift-JIS, Latin-1) | Modern vaults use UTF-8 markdown exclusively. |`

---

### `line_ending_selector`
- **Category**: B. Language Server Protocol (LSP) & Code Tooling
- **Primary Crate Location**: `crates/line_ending_selector`
- **Path Status**: `Present`

**Dependencies & References Found:**

#### Manifest Declarations (`Cargo.toml`)
- `Cargo.toml:127:    "crates/line_ending_selector",`
- `Cargo.toml:393:line_ending_selector = { path = "crates/line_ending_selector" }`

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:50:| `crates/line_ending_selector` | Switching CRLF / LF line endings in status bar | IDE status bar widget; clutter reduction. |`

---

## C. Programming Languages (Purge 50+ Non-Markdown Grammars)
### `c`
- **Category**: C. Programming Languages
- **Primary Crate Location**: `crates/languages/src/c`
- **Path Status**: `Pruned / Not Present`

**Dependencies & References Found:**

#### Manifest Declarations (`Cargo.toml`)
- `Cargo.toml:846:tree-sitter-c = "0.24.1"`

#### Documentation References (`.md`)
- `plan.md:139:                .find(|c: char| !c.is_alphanumeric() && c != '_' && c != '-')`
- `plan.md:151:            .find(|c: char| c == '\n' || c == '#' || c == '[')`
- `PURGE_MANIFEST.md:57:* **Purge List**: `c`, `cpp`, `c_sharp`, `clojure`, `cmake`, `css`, `dart`, `dockerfile`, `elixir`, `elm`, `erb`, `erlang`, `fortran`, `gleam`, `glsl`, `go`, `groovy`, `haskell`, `helm`, `html`, `java`, `javascript`, `json5`, `julia`, `kotlin`, `lua`, `make`, `nix`, `objc`, `ocaml`, `pascal`, `perl`, `php`, `po`, `proto`, `purescript`, `python`, `r`, `racket`, `ruby`, `rust`, `scala`, `scheme`, `scss`, `solidity`, `sql`, `starlark`, `swift`, `svelte`, `systemd`, `tailwind`, `thrift`, `tsv`, `tsx`, `typescript`, `typst`, `v`, `verilog`, `vhdl`, `vue`, `wgsl`, `xml`, `zig`.`

---

### `cpp`
- **Category**: C. Programming Languages
- **Primary Crate Location**: `crates/languages/src/cpp`
- **Path Status**: `Pruned / Not Present`

**Dependencies & References Found:**

#### Manifest Declarations (`Cargo.toml`)
- `Cargo.toml:847:tree-sitter-cpp = { git = "https://github.com/tree-sitter/tree-sitter-cpp", rev = "5cb9b693cfd7bfacab1d9ff4acac1a4150700609" }`

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:57:* **Purge List**: `c`, `cpp`, `c_sharp`, `clojure`, `cmake`, `css`, `dart`, `dockerfile`, `elixir`, `elm`, `erb`, `erlang`, `fortran`, `gleam`, `glsl`, `go`, `groovy`, `haskell`, `helm`, `html`, `java`, `javascript`, `json5`, `julia`, `kotlin`, `lua`, `make`, `nix`, `objc`, `ocaml`, `pascal`, `perl`, `php`, `po`, `proto`, `purescript`, `python`, `r`, `racket`, `ruby`, `rust`, `scala`, `scheme`, `scss`, `solidity`, `sql`, `starlark`, `swift`, `svelte`, `systemd`, `tailwind`, `thrift`, `tsv`, `tsx`, `typescript`, `typst`, `v`, `verilog`, `vhdl`, `vue`, `wgsl`, `xml`, `zig`.`

---

### `c_sharp`
- **Category**: C. Programming Languages
- **Primary Crate Location**: `crates/languages/src/c_sharp`
- **Path Status**: `Pruned / Not Present`

**Dependencies & References Found:**

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:57:* **Purge List**: `c`, `cpp`, `c_sharp`, `clojure`, `cmake`, `css`, `dart`, `dockerfile`, `elixir`, `elm`, `erb`, `erlang`, `fortran`, `gleam`, `glsl`, `go`, `groovy`, `haskell`, `helm`, `html`, `java`, `javascript`, `json5`, `julia`, `kotlin`, `lua`, `make`, `nix`, `objc`, `ocaml`, `pascal`, `perl`, `php`, `po`, `proto`, `purescript`, `python`, `r`, `racket`, `ruby`, `rust`, `scala`, `scheme`, `scss`, `solidity`, `sql`, `starlark`, `swift`, `svelte`, `systemd`, `tailwind`, `thrift`, `tsv`, `tsx`, `typescript`, `typst`, `v`, `verilog`, `vhdl`, `vue`, `wgsl`, `xml`, `zig`.`

---

### `clojure`
- **Category**: C. Programming Languages
- **Primary Crate Location**: `crates/languages/src/clojure`
- **Path Status**: `Pruned / Not Present`

**Dependencies & References Found:**

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:57:* **Purge List**: `c`, `cpp`, `c_sharp`, `clojure`, `cmake`, `css`, `dart`, `dockerfile`, `elixir`, `elm`, `erb`, `erlang`, `fortran`, `gleam`, `glsl`, `go`, `groovy`, `haskell`, `helm`, `html`, `java`, `javascript`, `json5`, `julia`, `kotlin`, `lua`, `make`, `nix`, `objc`, `ocaml`, `pascal`, `perl`, `php`, `po`, `proto`, `purescript`, `python`, `r`, `racket`, `ruby`, `rust`, `scala`, `scheme`, `scss`, `solidity`, `sql`, `starlark`, `swift`, `svelte`, `systemd`, `tailwind`, `thrift`, `tsv`, `tsx`, `typescript`, `typst`, `v`, `verilog`, `vhdl`, `vue`, `wgsl`, `xml`, `zig`.`

---

### `cmake`
- **Category**: C. Programming Languages
- **Primary Crate Location**: `crates/languages/src/cmake`
- **Path Status**: `Pruned / Not Present`

**Dependencies & References Found:**

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:57:* **Purge List**: `c`, `cpp`, `c_sharp`, `clojure`, `cmake`, `css`, `dart`, `dockerfile`, `elixir`, `elm`, `erb`, `erlang`, `fortran`, `gleam`, `glsl`, `go`, `groovy`, `haskell`, `helm`, `html`, `java`, `javascript`, `json5`, `julia`, `kotlin`, `lua`, `make`, `nix`, `objc`, `ocaml`, `pascal`, `perl`, `php`, `po`, `proto`, `purescript`, `python`, `r`, `racket`, `ruby`, `rust`, `scala`, `scheme`, `scss`, `solidity`, `sql`, `starlark`, `swift`, `svelte`, `systemd`, `tailwind`, `thrift`, `tsv`, `tsx`, `typescript`, `typst`, `v`, `verilog`, `vhdl`, `vue`, `wgsl`, `xml`, `zig`.`

---

### `css`
- **Category**: C. Programming Languages
- **Primary Crate Location**: `crates/languages/src/css`
- **Path Status**: `Pruned / Not Present`

**Dependencies & References Found:**

#### Manifest Declarations (`Cargo.toml`)
- `Cargo.toml:848:tree-sitter-css = "0.23"`

#### Configuration / Keymaps / Asset Files
- `typos.toml:51:    "docs/theme/css/",`

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:57:* **Purge List**: `c`, `cpp`, `c_sharp`, `clojure`, `cmake`, `css`, `dart`, `dockerfile`, `elixir`, `elm`, `erb`, `erlang`, `fortran`, `gleam`, `glsl`, `go`, `groovy`, `haskell`, `helm`, `html`, `java`, `javascript`, `json5`, `julia`, `kotlin`, `lua`, `make`, `nix`, `objc`, `ocaml`, `pascal`, `perl`, `php`, `po`, `proto`, `purescript`, `python`, `r`, `racket`, `ruby`, `rust`, `scala`, `scheme`, `scss`, `solidity`, `sql`, `starlark`, `swift`, `svelte`, `systemd`, `tailwind`, `thrift`, `tsv`, `tsx`, `typescript`, `typst`, `v`, `verilog`, `vhdl`, `vue`, `wgsl`, `xml`, `zig`.`

---

### `dart`
- **Category**: C. Programming Languages
- **Primary Crate Location**: `crates/languages/src/dart`
- **Path Status**: `Pruned / Not Present`

**Dependencies & References Found:**

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:57:* **Purge List**: `c`, `cpp`, `c_sharp`, `clojure`, `cmake`, `css`, `dart`, `dockerfile`, `elixir`, `elm`, `erb`, `erlang`, `fortran`, `gleam`, `glsl`, `go`, `groovy`, `haskell`, `helm`, `html`, `java`, `javascript`, `json5`, `julia`, `kotlin`, `lua`, `make`, `nix`, `objc`, `ocaml`, `pascal`, `perl`, `php`, `po`, `proto`, `purescript`, `python`, `r`, `racket`, `ruby`, `rust`, `scala`, `scheme`, `scss`, `solidity`, `sql`, `starlark`, `swift`, `svelte`, `systemd`, `tailwind`, `thrift`, `tsv`, `tsx`, `typescript`, `typst`, `v`, `verilog`, `vhdl`, `vue`, `wgsl`, `xml`, `zig`.`

---

### `dockerfile`
- **Category**: C. Programming Languages
- **Primary Crate Location**: `crates/languages/src/dockerfile`
- **Path Status**: `Pruned / Not Present`

**Dependencies & References Found:**

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:57:* **Purge List**: `c`, `cpp`, `c_sharp`, `clojure`, `cmake`, `css`, `dart`, `dockerfile`, `elixir`, `elm`, `erb`, `erlang`, `fortran`, `gleam`, `glsl`, `go`, `groovy`, `haskell`, `helm`, `html`, `java`, `javascript`, `json5`, `julia`, `kotlin`, `lua`, `make`, `nix`, `objc`, `ocaml`, `pascal`, `perl`, `php`, `po`, `proto`, `purescript`, `python`, `r`, `racket`, `ruby`, `rust`, `scala`, `scheme`, `scss`, `solidity`, `sql`, `starlark`, `swift`, `svelte`, `systemd`, `tailwind`, `thrift`, `tsv`, `tsx`, `typescript`, `typst`, `v`, `verilog`, `vhdl`, `vue`, `wgsl`, `xml`, `zig`.`

---

### `elixir`
- **Category**: C. Programming Languages
- **Primary Crate Location**: `crates/languages/src/elixir`
- **Path Status**: `Pruned / Not Present`

**Dependencies & References Found:**

#### Manifest Declarations (`Cargo.toml`)
- `Cargo.toml:850:tree-sitter-elixir = "0.3"`

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:57:* **Purge List**: `c`, `cpp`, `c_sharp`, `clojure`, `cmake`, `css`, `dart`, `dockerfile`, `elixir`, `elm`, `erb`, `erlang`, `fortran`, `gleam`, `glsl`, `go`, `groovy`, `haskell`, `helm`, `html`, `java`, `javascript`, `json5`, `julia`, `kotlin`, `lua`, `make`, `nix`, `objc`, `ocaml`, `pascal`, `perl`, `php`, `po`, `proto`, `purescript`, `python`, `r`, `racket`, `ruby`, `rust`, `scala`, `scheme`, `scss`, `solidity`, `sql`, `starlark`, `swift`, `svelte`, `systemd`, `tailwind`, `thrift`, `tsv`, `tsx`, `typescript`, `typst`, `v`, `verilog`, `vhdl`, `vue`, `wgsl`, `xml`, `zig`.`

---

### `elm`
- **Category**: C. Programming Languages
- **Primary Crate Location**: `crates/languages/src/elm`
- **Path Status**: `Pruned / Not Present`

**Dependencies & References Found:**

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:57:* **Purge List**: `c`, `cpp`, `c_sharp`, `clojure`, `cmake`, `css`, `dart`, `dockerfile`, `elixir`, `elm`, `erb`, `erlang`, `fortran`, `gleam`, `glsl`, `go`, `groovy`, `haskell`, `helm`, `html`, `java`, `javascript`, `json5`, `julia`, `kotlin`, `lua`, `make`, `nix`, `objc`, `ocaml`, `pascal`, `perl`, `php`, `po`, `proto`, `purescript`, `python`, `r`, `racket`, `ruby`, `rust`, `scala`, `scheme`, `scss`, `solidity`, `sql`, `starlark`, `swift`, `svelte`, `systemd`, `tailwind`, `thrift`, `tsv`, `tsx`, `typescript`, `typst`, `v`, `verilog`, `vhdl`, `vue`, `wgsl`, `xml`, `zig`.`

---

### `erb`
- **Category**: C. Programming Languages
- **Primary Crate Location**: `crates/languages/src/erb`
- **Path Status**: `Pruned / Not Present`

**Dependencies & References Found:**

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:57:* **Purge List**: `c`, `cpp`, `c_sharp`, `clojure`, `cmake`, `css`, `dart`, `dockerfile`, `elixir`, `elm`, `erb`, `erlang`, `fortran`, `gleam`, `glsl`, `go`, `groovy`, `haskell`, `helm`, `html`, `java`, `javascript`, `json5`, `julia`, `kotlin`, `lua`, `make`, `nix`, `objc`, `ocaml`, `pascal`, `perl`, `php`, `po`, `proto`, `purescript`, `python`, `r`, `racket`, `ruby`, `rust`, `scala`, `scheme`, `scss`, `solidity`, `sql`, `starlark`, `swift`, `svelte`, `systemd`, `tailwind`, `thrift`, `tsv`, `tsx`, `typescript`, `typst`, `v`, `verilog`, `vhdl`, `vue`, `wgsl`, `xml`, `zig`.`

---

### `erlang`
- **Category**: C. Programming Languages
- **Primary Crate Location**: `crates/languages/src/erlang`
- **Path Status**: `Pruned / Not Present`

**Dependencies & References Found:**

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:57:* **Purge List**: `c`, `cpp`, `c_sharp`, `clojure`, `cmake`, `css`, `dart`, `dockerfile`, `elixir`, `elm`, `erb`, `erlang`, `fortran`, `gleam`, `glsl`, `go`, `groovy`, `haskell`, `helm`, `html`, `java`, `javascript`, `json5`, `julia`, `kotlin`, `lua`, `make`, `nix`, `objc`, `ocaml`, `pascal`, `perl`, `php`, `po`, `proto`, `purescript`, `python`, `r`, `racket`, `ruby`, `rust`, `scala`, `scheme`, `scss`, `solidity`, `sql`, `starlark`, `swift`, `svelte`, `systemd`, `tailwind`, `thrift`, `tsv`, `tsx`, `typescript`, `typst`, `v`, `verilog`, `vhdl`, `vue`, `wgsl`, `xml`, `zig`.`

---

### `fortran`
- **Category**: C. Programming Languages
- **Primary Crate Location**: `crates/languages/src/fortran`
- **Path Status**: `Pruned / Not Present`

**Dependencies & References Found:**

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:57:* **Purge List**: `c`, `cpp`, `c_sharp`, `clojure`, `cmake`, `css`, `dart`, `dockerfile`, `elixir`, `elm`, `erb`, `erlang`, `fortran`, `gleam`, `glsl`, `go`, `groovy`, `haskell`, `helm`, `html`, `java`, `javascript`, `json5`, `julia`, `kotlin`, `lua`, `make`, `nix`, `objc`, `ocaml`, `pascal`, `perl`, `php`, `po`, `proto`, `purescript`, `python`, `r`, `racket`, `ruby`, `rust`, `scala`, `scheme`, `scss`, `solidity`, `sql`, `starlark`, `swift`, `svelte`, `systemd`, `tailwind`, `thrift`, `tsv`, `tsx`, `typescript`, `typst`, `v`, `verilog`, `vhdl`, `vue`, `wgsl`, `xml`, `zig`.`

---

### `gleam`
- **Category**: C. Programming Languages
- **Primary Crate Location**: `crates/languages/src/gleam`
- **Path Status**: `Pruned / Not Present`

**Dependencies & References Found:**

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:57:* **Purge List**: `c`, `cpp`, `c_sharp`, `clojure`, `cmake`, `css`, `dart`, `dockerfile`, `elixir`, `elm`, `erb`, `erlang`, `fortran`, `gleam`, `glsl`, `go`, `groovy`, `haskell`, `helm`, `html`, `java`, `javascript`, `json5`, `julia`, `kotlin`, `lua`, `make`, `nix`, `objc`, `ocaml`, `pascal`, `perl`, `php`, `po`, `proto`, `purescript`, `python`, `r`, `racket`, `ruby`, `rust`, `scala`, `scheme`, `scss`, `solidity`, `sql`, `starlark`, `swift`, `svelte`, `systemd`, `tailwind`, `thrift`, `tsv`, `tsx`, `typescript`, `typst`, `v`, `verilog`, `vhdl`, `vue`, `wgsl`, `xml`, `zig`.`

---

### `glsl`
- **Category**: C. Programming Languages
- **Primary Crate Location**: `crates/languages/src/glsl`
- **Path Status**: `Pruned / Not Present`

**Dependencies & References Found:**

#### Manifest Declarations (`Cargo.toml`)
- `Cargo.toml:252:    "extensions/glsl",`

#### Configuration / Keymaps / Asset Files
- `typos.toml:40:    # glsl isn't recognized by this tool.`
- `typos.toml:41:    "extensions/glsl/languages/glsl/",`

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:57:* **Purge List**: `c`, `cpp`, `c_sharp`, `clojure`, `cmake`, `css`, `dart`, `dockerfile`, `elixir`, `elm`, `erb`, `erlang`, `fortran`, `gleam`, `glsl`, `go`, `groovy`, `haskell`, `helm`, `html`, `java`, `javascript`, `json5`, `julia`, `kotlin`, `lua`, `make`, `nix`, `objc`, `ocaml`, `pascal`, `perl`, `php`, `po`, `proto`, `purescript`, `python`, `r`, `racket`, `ruby`, `rust`, `scala`, `scheme`, `scss`, `solidity`, `sql`, `starlark`, `swift`, `svelte`, `systemd`, `tailwind`, `thrift`, `tsv`, `tsx`, `typescript`, `typst`, `v`, `verilog`, `vhdl`, `vue`, `wgsl`, `xml`, `zig`.`

---

### `go`
- **Category**: C. Programming Languages
- **Primary Crate Location**: `crates/languages/src/go`
- **Path Status**: `Pruned / Not Present`

**Dependencies & References Found:**

#### Manifest Declarations (`Cargo.toml`)
- `Cargo.toml:853:tree-sitter-go = "0.25"`
- `Cargo.toml:854:tree-sitter-go-mod = { git = "https://github.com/camdencheek/tree-sitter-go-mod", rev = "2e886870578eeba1927a2dc4bd2e2b3f598c5f9a", package = "tree-sitter-gomod" }`
- `Cargo.toml:855:tree-sitter-gowork = { git = "https://github.com/zed-industries/tree-sitter-go-work", rev = "acb0617bf7f4fda02c6217676cc64acb89536dc7" }`

#### Documentation References (`.md`)
- `README.md:47:Sponsorships go directly to Zed Industries and are used as general company revenue.`
- `PURGE_MANIFEST.md:57:* **Purge List**: `c`, `cpp`, `c_sharp`, `clojure`, `cmake`, `css`, `dart`, `dockerfile`, `elixir`, `elm`, `erb`, `erlang`, `fortran`, `gleam`, `glsl`, `go`, `groovy`, `haskell`, `helm`, `html`, `java`, `javascript`, `json5`, `julia`, `kotlin`, `lua`, `make`, `nix`, `objc`, `ocaml`, `pascal`, `perl`, `php`, `po`, `proto`, `purescript`, `python`, `r`, `racket`, `ruby`, `rust`, `scala`, `scheme`, `scss`, `solidity`, `sql`, `starlark`, `swift`, `svelte`, `systemd`, `tailwind`, `thrift`, `tsv`, `tsx`, `typescript`, `typst`, `v`, `verilog`, `vhdl`, `vue`, `wgsl`, `xml`, `zig`.`
- `CONTRIBUTING.md:168:Zed is made up of several smaller crates - let's go over those you're most likely to interact with:`

---

### `groovy`
- **Category**: C. Programming Languages
- **Primary Crate Location**: `crates/languages/src/groovy`
- **Path Status**: `Pruned / Not Present`

**Dependencies & References Found:**

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:57:* **Purge List**: `c`, `cpp`, `c_sharp`, `clojure`, `cmake`, `css`, `dart`, `dockerfile`, `elixir`, `elm`, `erb`, `erlang`, `fortran`, `gleam`, `glsl`, `go`, `groovy`, `haskell`, `helm`, `html`, `java`, `javascript`, `json5`, `julia`, `kotlin`, `lua`, `make`, `nix`, `objc`, `ocaml`, `pascal`, `perl`, `php`, `po`, `proto`, `purescript`, `python`, `r`, `racket`, `ruby`, `rust`, `scala`, `scheme`, `scss`, `solidity`, `sql`, `starlark`, `swift`, `svelte`, `systemd`, `tailwind`, `thrift`, `tsv`, `tsx`, `typescript`, `typst`, `v`, `verilog`, `vhdl`, `vue`, `wgsl`, `xml`, `zig`.`

---

### `haskell`
- **Category**: C. Programming Languages
- **Primary Crate Location**: `crates/languages/src/haskell`
- **Path Status**: `Pruned / Not Present`

**Dependencies & References Found:**

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:57:* **Purge List**: `c`, `cpp`, `c_sharp`, `clojure`, `cmake`, `css`, `dart`, `dockerfile`, `elixir`, `elm`, `erb`, `erlang`, `fortran`, `gleam`, `glsl`, `go`, `groovy`, `haskell`, `helm`, `html`, `java`, `javascript`, `json5`, `julia`, `kotlin`, `lua`, `make`, `nix`, `objc`, `ocaml`, `pascal`, `perl`, `php`, `po`, `proto`, `purescript`, `python`, `r`, `racket`, `ruby`, `rust`, `scala`, `scheme`, `scss`, `solidity`, `sql`, `starlark`, `swift`, `svelte`, `systemd`, `tailwind`, `thrift`, `tsv`, `tsx`, `typescript`, `typst`, `v`, `verilog`, `vhdl`, `vue`, `wgsl`, `xml`, `zig`.`

---

### `helm`
- **Category**: C. Programming Languages
- **Primary Crate Location**: `crates/languages/src/helm`
- **Path Status**: `Pruned / Not Present`

**Dependencies & References Found:**

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:57:* **Purge List**: `c`, `cpp`, `c_sharp`, `clojure`, `cmake`, `css`, `dart`, `dockerfile`, `elixir`, `elm`, `erb`, `erlang`, `fortran`, `gleam`, `glsl`, `go`, `groovy`, `haskell`, `helm`, `html`, `java`, `javascript`, `json5`, `julia`, `kotlin`, `lua`, `make`, `nix`, `objc`, `ocaml`, `pascal`, `perl`, `php`, `po`, `proto`, `purescript`, `python`, `r`, `racket`, `ruby`, `rust`, `scala`, `scheme`, `scss`, `solidity`, `sql`, `starlark`, `swift`, `svelte`, `systemd`, `tailwind`, `thrift`, `tsv`, `tsx`, `typescript`, `typst`, `v`, `verilog`, `vhdl`, `vue`, `wgsl`, `xml`, `zig`.`

---

### `html`
- **Category**: C. Programming Languages
- **Primary Crate Location**: `crates/languages/src/html`
- **Path Status**: `Pruned / Not Present`

**Dependencies & References Found:**

#### Manifest Declarations (`Cargo.toml`)
- `Cargo.toml:253:    "extensions/html",`
- `Cargo.toml:857:tree-sitter-html = "0.23"`

#### Documentation References (`.md`)
- `README.md:40:- Is `cargo-about` unable to find the license for a dependency? If so, add a clarification field at the end of `script/licenses/zed-licenses.toml`, as specified in the [cargo-about book](https://embarkstudios.github.io/cargo-about/cli/generate/config.html#crate-configuration).`
- `PURGE_MANIFEST.md:57:* **Purge List**: `c`, `cpp`, `c_sharp`, `clojure`, `cmake`, `css`, `dart`, `dockerfile`, `elixir`, `elm`, `erb`, `erlang`, `fortran`, `gleam`, `glsl`, `go`, `groovy`, `haskell`, `helm`, `html`, `java`, `javascript`, `json5`, `julia`, `kotlin`, `lua`, `make`, `nix`, `objc`, `ocaml`, `pascal`, `perl`, `php`, `po`, `proto`, `purescript`, `python`, `r`, `racket`, `ruby`, `rust`, `scala`, `scheme`, `scss`, `solidity`, `sql`, `starlark`, `swift`, `svelte`, `systemd`, `tailwind`, `thrift`, `tsv`, `tsx`, `typescript`, `typst`, `v`, `verilog`, `vhdl`, `vue`, `wgsl`, `xml`, `zig`.`

---

### `java`
- **Category**: C. Programming Languages
- **Primary Crate Location**: `crates/languages/src/java`
- **Path Status**: `Pruned / Not Present`

**Dependencies & References Found:**

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:57:* **Purge List**: `c`, `cpp`, `c_sharp`, `clojure`, `cmake`, `css`, `dart`, `dockerfile`, `elixir`, `elm`, `erb`, `erlang`, `fortran`, `gleam`, `glsl`, `go`, `groovy`, `haskell`, `helm`, `html`, `java`, `javascript`, `json5`, `julia`, `kotlin`, `lua`, `make`, `nix`, `objc`, `ocaml`, `pascal`, `perl`, `php`, `po`, `proto`, `purescript`, `python`, `r`, `racket`, `ruby`, `rust`, `scala`, `scheme`, `scss`, `solidity`, `sql`, `starlark`, `swift`, `svelte`, `systemd`, `tailwind`, `thrift`, `tsv`, `tsx`, `typescript`, `typst`, `v`, `verilog`, `vhdl`, `vue`, `wgsl`, `xml`, `zig`.`

---

### `javascript`
- **Category**: C. Programming Languages
- **Primary Crate Location**: `crates/languages/src/javascript`
- **Path Status**: `Pruned / Not Present`

**Dependencies & References Found:**

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:57:* **Purge List**: `c`, `cpp`, `c_sharp`, `clojure`, `cmake`, `css`, `dart`, `dockerfile`, `elixir`, `elm`, `erb`, `erlang`, `fortran`, `gleam`, `glsl`, `go`, `groovy`, `haskell`, `helm`, `html`, `java`, `javascript`, `json5`, `julia`, `kotlin`, `lua`, `make`, `nix`, `objc`, `ocaml`, `pascal`, `perl`, `php`, `po`, `proto`, `purescript`, `python`, `r`, `racket`, `ruby`, `rust`, `scala`, `scheme`, `scss`, `solidity`, `sql`, `starlark`, `swift`, `svelte`, `systemd`, `tailwind`, `thrift`, `tsv`, `tsx`, `typescript`, `typst`, `v`, `verilog`, `vhdl`, `vue`, `wgsl`, `xml`, `zig`.`

---

### `json5`
- **Category**: C. Programming Languages
- **Primary Crate Location**: `crates/languages/src/json5`
- **Path Status**: `Pruned / Not Present`

**Dependencies & References Found:**

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:57:* **Purge List**: `c`, `cpp`, `c_sharp`, `clojure`, `cmake`, `css`, `dart`, `dockerfile`, `elixir`, `elm`, `erb`, `erlang`, `fortran`, `gleam`, `glsl`, `go`, `groovy`, `haskell`, `helm`, `html`, `java`, `javascript`, `json5`, `julia`, `kotlin`, `lua`, `make`, `nix`, `objc`, `ocaml`, `pascal`, `perl`, `php`, `po`, `proto`, `purescript`, `python`, `r`, `racket`, `ruby`, `rust`, `scala`, `scheme`, `scss`, `solidity`, `sql`, `starlark`, `swift`, `svelte`, `systemd`, `tailwind`, `thrift`, `tsv`, `tsx`, `typescript`, `typst`, `v`, `verilog`, `vhdl`, `vue`, `wgsl`, `xml`, `zig`.`

---

### `julia`
- **Category**: C. Programming Languages
- **Primary Crate Location**: `crates/languages/src/julia`
- **Path Status**: `Pruned / Not Present`

**Dependencies & References Found:**

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:57:* **Purge List**: `c`, `cpp`, `c_sharp`, `clojure`, `cmake`, `css`, `dart`, `dockerfile`, `elixir`, `elm`, `erb`, `erlang`, `fortran`, `gleam`, `glsl`, `go`, `groovy`, `haskell`, `helm`, `html`, `java`, `javascript`, `json5`, `julia`, `kotlin`, `lua`, `make`, `nix`, `objc`, `ocaml`, `pascal`, `perl`, `php`, `po`, `proto`, `purescript`, `python`, `r`, `racket`, `ruby`, `rust`, `scala`, `scheme`, `scss`, `solidity`, `sql`, `starlark`, `swift`, `svelte`, `systemd`, `tailwind`, `thrift`, `tsv`, `tsx`, `typescript`, `typst`, `v`, `verilog`, `vhdl`, `vue`, `wgsl`, `xml`, `zig`.`

---

### `kotlin`
- **Category**: C. Programming Languages
- **Primary Crate Location**: `crates/languages/src/kotlin`
- **Path Status**: `Pruned / Not Present`

**Dependencies & References Found:**

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:57:* **Purge List**: `c`, `cpp`, `c_sharp`, `clojure`, `cmake`, `css`, `dart`, `dockerfile`, `elixir`, `elm`, `erb`, `erlang`, `fortran`, `gleam`, `glsl`, `go`, `groovy`, `haskell`, `helm`, `html`, `java`, `javascript`, `json5`, `julia`, `kotlin`, `lua`, `make`, `nix`, `objc`, `ocaml`, `pascal`, `perl`, `php`, `po`, `proto`, `purescript`, `python`, `r`, `racket`, `ruby`, `rust`, `scala`, `scheme`, `scss`, `solidity`, `sql`, `starlark`, `swift`, `svelte`, `systemd`, `tailwind`, `thrift`, `tsv`, `tsx`, `typescript`, `typst`, `v`, `verilog`, `vhdl`, `vue`, `wgsl`, `xml`, `zig`.`

---

### `lua`
- **Category**: C. Programming Languages
- **Primary Crate Location**: `crates/languages/src/lua`
- **Path Status**: `Pruned / Not Present`

**Dependencies & References Found:**

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:57:* **Purge List**: `c`, `cpp`, `c_sharp`, `clojure`, `cmake`, `css`, `dart`, `dockerfile`, `elixir`, `elm`, `erb`, `erlang`, `fortran`, `gleam`, `glsl`, `go`, `groovy`, `haskell`, `helm`, `html`, `java`, `javascript`, `json5`, `julia`, `kotlin`, `lua`, `make`, `nix`, `objc`, `ocaml`, `pascal`, `perl`, `php`, `po`, `proto`, `purescript`, `python`, `r`, `racket`, `ruby`, `rust`, `scala`, `scheme`, `scss`, `solidity`, `sql`, `starlark`, `swift`, `svelte`, `systemd`, `tailwind`, `thrift`, `tsv`, `tsx`, `typescript`, `typst`, `v`, `verilog`, `vhdl`, `vue`, `wgsl`, `xml`, `zig`.`

---

### `make`
- **Category**: C. Programming Languages
- **Primary Crate Location**: `crates/languages/src/make`
- **Path Status**: `Pruned / Not Present`

**Dependencies & References Found:**

#### Manifest Declarations (`Cargo.toml`)
- `Cargo.toml:1024:# Build single-source-file crates with cg=1 as it helps make `cargo build` of a whole workspace a bit faster`

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:57:* **Purge List**: `c`, `cpp`, `c_sharp`, `clojure`, `cmake`, `css`, `dart`, `dockerfile`, `elixir`, `elm`, `erb`, `erlang`, `fortran`, `gleam`, `glsl`, `go`, `groovy`, `haskell`, `helm`, `html`, `java`, `javascript`, `json5`, `julia`, `kotlin`, `lua`, `make`, `nix`, `objc`, `ocaml`, `pascal`, `perl`, `php`, `po`, `proto`, `purescript`, `python`, `r`, `racket`, `ruby`, `rust`, `scala`, `scheme`, `scss`, `solidity`, `sql`, `starlark`, `swift`, `svelte`, `systemd`, `tailwind`, `thrift`, `tsv`, `tsx`, `typescript`, `typst`, `v`, `verilog`, `vhdl`, `vue`, `wgsl`, `xml`, `zig`.`
- `CONTRIBUTING.md:3:Thank you for helping us make Zed better!`
- `CONTRIBUTING.md:20:- **Small** enhancements to existing features to **make them work for more people** (making things work on more platforms/modes/whatever).`
- `CONTRIBUTING.md:46:that we've agreed we want, please open a PR early so we can discuss how to make`
- `gpui_notes.md:57:Instead of standard Rust ownership hierarchies (which make cyclic UI references difficult), GPUI uses an **Arena / SlotMap entity store**:`

---

### `nix`
- **Category**: C. Programming Languages
- **Primary Crate Location**: `crates/languages/src/nix`
- **Path Status**: `Pruned / Not Present`

**Dependencies & References Found:**

#### Manifest Declarations (`Cargo.toml`)
- `Cargo.toml:687:nix = "0.29"`

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:57:* **Purge List**: `c`, `cpp`, `c_sharp`, `clojure`, `cmake`, `css`, `dart`, `dockerfile`, `elixir`, `elm`, `erb`, `erlang`, `fortran`, `gleam`, `glsl`, `go`, `groovy`, `haskell`, `helm`, `html`, `java`, `javascript`, `json5`, `julia`, `kotlin`, `lua`, `make`, `nix`, `objc`, `ocaml`, `pascal`, `perl`, `php`, `po`, `proto`, `purescript`, `python`, `r`, `racket`, `ruby`, `rust`, `scala`, `scheme`, `scss`, `solidity`, `sql`, `starlark`, `swift`, `svelte`, `systemd`, `tailwind`, `thrift`, `tsv`, `tsx`, `typescript`, `typst`, `v`, `verilog`, `vhdl`, `vue`, `wgsl`, `xml`, `zig`.`

---

### `objc`
- **Category**: C. Programming Languages
- **Primary Crate Location**: `crates/languages/src/objc`
- **Path Status**: `Pruned / Not Present`

**Dependencies & References Found:**

#### Manifest Declarations (`Cargo.toml`)
- `Cargo.toml:691:objc = "0.2"`

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:57:* **Purge List**: `c`, `cpp`, `c_sharp`, `clojure`, `cmake`, `css`, `dart`, `dockerfile`, `elixir`, `elm`, `erb`, `erlang`, `fortran`, `gleam`, `glsl`, `go`, `groovy`, `haskell`, `helm`, `html`, `java`, `javascript`, `json5`, `julia`, `kotlin`, `lua`, `make`, `nix`, `objc`, `ocaml`, `pascal`, `perl`, `php`, `po`, `proto`, `purescript`, `python`, `r`, `racket`, `ruby`, `rust`, `scala`, `scheme`, `scss`, `solidity`, `sql`, `starlark`, `swift`, `svelte`, `systemd`, `tailwind`, `thrift`, `tsv`, `tsx`, `typescript`, `typst`, `v`, `verilog`, `vhdl`, `vue`, `wgsl`, `xml`, `zig`.`

---

### `ocaml`
- **Category**: C. Programming Languages
- **Primary Crate Location**: `crates/languages/src/ocaml`
- **Path Status**: `Pruned / Not Present`

**Dependencies & References Found:**

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:57:* **Purge List**: `c`, `cpp`, `c_sharp`, `clojure`, `cmake`, `css`, `dart`, `dockerfile`, `elixir`, `elm`, `erb`, `erlang`, `fortran`, `gleam`, `glsl`, `go`, `groovy`, `haskell`, `helm`, `html`, `java`, `javascript`, `json5`, `julia`, `kotlin`, `lua`, `make`, `nix`, `objc`, `ocaml`, `pascal`, `perl`, `php`, `po`, `proto`, `purescript`, `python`, `r`, `racket`, `ruby`, `rust`, `scala`, `scheme`, `scss`, `solidity`, `sql`, `starlark`, `swift`, `svelte`, `systemd`, `tailwind`, `thrift`, `tsv`, `tsx`, `typescript`, `typst`, `v`, `verilog`, `vhdl`, `vue`, `wgsl`, `xml`, `zig`.`

---

### `pascal`
- **Category**: C. Programming Languages
- **Primary Crate Location**: `crates/languages/src/pascal`
- **Path Status**: `Pruned / Not Present`

**Dependencies & References Found:**

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:57:* **Purge List**: `c`, `cpp`, `c_sharp`, `clojure`, `cmake`, `css`, `dart`, `dockerfile`, `elixir`, `elm`, `erb`, `erlang`, `fortran`, `gleam`, `glsl`, `go`, `groovy`, `haskell`, `helm`, `html`, `java`, `javascript`, `json5`, `julia`, `kotlin`, `lua`, `make`, `nix`, `objc`, `ocaml`, `pascal`, `perl`, `php`, `po`, `proto`, `purescript`, `python`, `r`, `racket`, `ruby`, `rust`, `scala`, `scheme`, `scss`, `solidity`, `sql`, `starlark`, `swift`, `svelte`, `systemd`, `tailwind`, `thrift`, `tsv`, `tsx`, `typescript`, `typst`, `v`, `verilog`, `vhdl`, `vue`, `wgsl`, `xml`, `zig`.`

---

### `perl`
- **Category**: C. Programming Languages
- **Primary Crate Location**: `crates/languages/src/perl`
- **Path Status**: `Pruned / Not Present`

**Dependencies & References Found:**

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:57:* **Purge List**: `c`, `cpp`, `c_sharp`, `clojure`, `cmake`, `css`, `dart`, `dockerfile`, `elixir`, `elm`, `erb`, `erlang`, `fortran`, `gleam`, `glsl`, `go`, `groovy`, `haskell`, `helm`, `html`, `java`, `javascript`, `json5`, `julia`, `kotlin`, `lua`, `make`, `nix`, `objc`, `ocaml`, `pascal`, `perl`, `php`, `po`, `proto`, `purescript`, `python`, `r`, `racket`, `ruby`, `rust`, `scala`, `scheme`, `scss`, `solidity`, `sql`, `starlark`, `swift`, `svelte`, `systemd`, `tailwind`, `thrift`, `tsv`, `tsx`, `typescript`, `typst`, `v`, `verilog`, `vhdl`, `vue`, `wgsl`, `xml`, `zig`.`

---

### `php`
- **Category**: C. Programming Languages
- **Primary Crate Location**: `crates/languages/src/php`
- **Path Status**: `Pruned / Not Present`

**Dependencies & References Found:**

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:57:* **Purge List**: `c`, `cpp`, `c_sharp`, `clojure`, `cmake`, `css`, `dart`, `dockerfile`, `elixir`, `elm`, `erb`, `erlang`, `fortran`, `gleam`, `glsl`, `go`, `groovy`, `haskell`, `helm`, `html`, `java`, `javascript`, `json5`, `julia`, `kotlin`, `lua`, `make`, `nix`, `objc`, `ocaml`, `pascal`, `perl`, `php`, `po`, `proto`, `purescript`, `python`, `r`, `racket`, `ruby`, `rust`, `scala`, `scheme`, `scss`, `solidity`, `sql`, `starlark`, `swift`, `svelte`, `systemd`, `tailwind`, `thrift`, `tsv`, `tsx`, `typescript`, `typst`, `v`, `verilog`, `vhdl`, `vue`, `wgsl`, `xml`, `zig`.`

---

### `po`
- **Category**: C. Programming Languages
- **Primary Crate Location**: `crates/languages/src/po`
- **Path Status**: `Pruned / Not Present`

**Dependencies & References Found:**

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:57:* **Purge List**: `c`, `cpp`, `c_sharp`, `clojure`, `cmake`, `css`, `dart`, `dockerfile`, `elixir`, `elm`, `erb`, `erlang`, `fortran`, `gleam`, `glsl`, `go`, `groovy`, `haskell`, `helm`, `html`, `java`, `javascript`, `json5`, `julia`, `kotlin`, `lua`, `make`, `nix`, `objc`, `ocaml`, `pascal`, `perl`, `php`, `po`, `proto`, `purescript`, `python`, `r`, `racket`, `ruby`, `rust`, `scala`, `scheme`, `scss`, `solidity`, `sql`, `starlark`, `swift`, `svelte`, `systemd`, `tailwind`, `thrift`, `tsv`, `tsx`, `typescript`, `typst`, `v`, `verilog`, `vhdl`, `vue`, `wgsl`, `xml`, `zig`.`

---

### `proto`
- **Category**: C. Programming Languages
- **Primary Crate Location**: `crates/languages/src/proto`
- **Path Status**: `Pruned / Not Present`

**Dependencies & References Found:**

#### Manifest Declarations (`Cargo.toml`)
- `Cargo.toml:168:    "crates/proto",`
- `Cargo.toml:254:    "extensions/proto",`
- `Cargo.toml:434:proto = { path = "crates/proto" }`

#### Configuration / Keymaps / Asset Files
- `typos.toml:43:    "extensions/proto/extension.toml",`
- `typos.toml:44:    "extensions/proto/src/language_servers/protols.rs",`

#### Documentation References (`.md`)
- `plan.md:285:- `collab`, `collab_ui`, `rpc`, `proto`, `client`, `call`, `livekit_*`, `cloud_*`, `channel`, `remote`, `remote_connection`, `remote_server`, `dev_container`, `sandbox``
- `PURGE_MANIFEST.md:57:* **Purge List**: `c`, `cpp`, `c_sharp`, `clojure`, `cmake`, `css`, `dart`, `dockerfile`, `elixir`, `elm`, `erb`, `erlang`, `fortran`, `gleam`, `glsl`, `go`, `groovy`, `haskell`, `helm`, `html`, `java`, `javascript`, `json5`, `julia`, `kotlin`, `lua`, `make`, `nix`, `objc`, `ocaml`, `pascal`, `perl`, `php`, `po`, `proto`, `purescript`, `python`, `r`, `racket`, `ruby`, `rust`, `scala`, `scheme`, `scss`, `solidity`, `sql`, `starlark`, `swift`, `svelte`, `systemd`, `tailwind`, `thrift`, `tsv`, `tsx`, `typescript`, `typst`, `v`, `verilog`, `vhdl`, `vue`, `wgsl`, `xml`, `zig`.`
- `PURGE_MANIFEST.md:74:| `crates/proto` | Generated protobuf definitions for backend RPC | Server communication schemas. |`

---

### `purescript`
- **Category**: C. Programming Languages
- **Primary Crate Location**: `crates/languages/src/purescript`
- **Path Status**: `Pruned / Not Present`

**Dependencies & References Found:**

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:57:* **Purge List**: `c`, `cpp`, `c_sharp`, `clojure`, `cmake`, `css`, `dart`, `dockerfile`, `elixir`, `elm`, `erb`, `erlang`, `fortran`, `gleam`, `glsl`, `go`, `groovy`, `haskell`, `helm`, `html`, `java`, `javascript`, `json5`, `julia`, `kotlin`, `lua`, `make`, `nix`, `objc`, `ocaml`, `pascal`, `perl`, `php`, `po`, `proto`, `purescript`, `python`, `r`, `racket`, `ruby`, `rust`, `scala`, `scheme`, `scss`, `solidity`, `sql`, `starlark`, `swift`, `svelte`, `systemd`, `tailwind`, `thrift`, `tsv`, `tsx`, `typescript`, `typst`, `v`, `verilog`, `vhdl`, `vue`, `wgsl`, `xml`, `zig`.`

---

### `python`
- **Category**: C. Programming Languages
- **Primary Crate Location**: `crates/languages/src/python`
- **Path Status**: `Pruned / Not Present`

**Dependencies & References Found:**

#### Manifest Declarations (`Cargo.toml`)
- `Cargo.toml:739:pet = { git = "https://github.com/microsoft/python-environment-tools.git", rev = "bb8e04607b96a3865d6aa4bb2a5a5a82ce05b5f0" }`
- `Cargo.toml:740:pet-conda = { git = "https://github.com/microsoft/python-environment-tools.git", rev = "bb8e04607b96a3865d6aa4bb2a5a5a82ce05b5f0" }`
- `Cargo.toml:741:pet-core = { git = "https://github.com/microsoft/python-environment-tools.git", rev = "bb8e04607b96a3865d6aa4bb2a5a5a82ce05b5f0" }`
- `Cargo.toml:742:pet-fs = { git = "https://github.com/microsoft/python-environment-tools.git", rev = "bb8e04607b96a3865d6aa4bb2a5a5a82ce05b5f0" }`
- `Cargo.toml:743:pet-poetry = { git = "https://github.com/microsoft/python-environment-tools.git", rev = "bb8e04607b96a3865d6aa4bb2a5a5a82ce05b5f0" }`
- `Cargo.toml:744:pet-reporter = { git = "https://github.com/microsoft/python-environment-tools.git", rev = "bb8e04607b96a3865d6aa4bb2a5a5a82ce05b5f0" }`
- `Cargo.toml:745:pet-virtualenv = { git = "https://github.com/microsoft/python-environment-tools.git", rev = "bb8e04607b96a3865d6aa4bb2a5a5a82ce05b5f0" }`
- `Cargo.toml:861:tree-sitter-python = "0.25"`

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:57:* **Purge List**: `c`, `cpp`, `c_sharp`, `clojure`, `cmake`, `css`, `dart`, `dockerfile`, `elixir`, `elm`, `erb`, `erlang`, `fortran`, `gleam`, `glsl`, `go`, `groovy`, `haskell`, `helm`, `html`, `java`, `javascript`, `json5`, `julia`, `kotlin`, `lua`, `make`, `nix`, `objc`, `ocaml`, `pascal`, `perl`, `php`, `po`, `proto`, `purescript`, `python`, `r`, `racket`, `ruby`, `rust`, `scala`, `scheme`, `scss`, `solidity`, `sql`, `starlark`, `swift`, `svelte`, `systemd`, `tailwind`, `thrift`, `tsv`, `tsx`, `typescript`, `typst`, `v`, `verilog`, `vhdl`, `vue`, `wgsl`, `xml`, `zig`.`
- `display_markdown.md:36:* **Tree-Sitter Grammars** (`tree-sitter-*`): Language parsers for syntax highlighting code snippets inside fence blocks (e.g., ` ```rust `, ` ```json `, ` ```python `).`

---

### `r`
- **Category**: C. Programming Languages
- **Primary Crate Location**: `crates/languages/src/r`
- **Path Status**: `Pruned / Not Present`

**Dependencies & References Found:**

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:57:* **Purge List**: `c`, `cpp`, `c_sharp`, `clojure`, `cmake`, `css`, `dart`, `dockerfile`, `elixir`, `elm`, `erb`, `erlang`, `fortran`, `gleam`, `glsl`, `go`, `groovy`, `haskell`, `helm`, `html`, `java`, `javascript`, `json5`, `julia`, `kotlin`, `lua`, `make`, `nix`, `objc`, `ocaml`, `pascal`, `perl`, `php`, `po`, `proto`, `purescript`, `python`, `r`, `racket`, `ruby`, `rust`, `scala`, `scheme`, `scss`, `solidity`, `sql`, `starlark`, `swift`, `svelte`, `systemd`, `tailwind`, `thrift`, `tsv`, `tsx`, `typescript`, `typst`, `v`, `verilog`, `vhdl`, `vue`, `wgsl`, `xml`, `zig`.`

---

### `racket`
- **Category**: C. Programming Languages
- **Primary Crate Location**: `crates/languages/src/racket`
- **Path Status**: `Pruned / Not Present`

**Dependencies & References Found:**

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:57:* **Purge List**: `c`, `cpp`, `c_sharp`, `clojure`, `cmake`, `css`, `dart`, `dockerfile`, `elixir`, `elm`, `erb`, `erlang`, `fortran`, `gleam`, `glsl`, `go`, `groovy`, `haskell`, `helm`, `html`, `java`, `javascript`, `json5`, `julia`, `kotlin`, `lua`, `make`, `nix`, `objc`, `ocaml`, `pascal`, `perl`, `php`, `po`, `proto`, `purescript`, `python`, `r`, `racket`, `ruby`, `rust`, `scala`, `scheme`, `scss`, `solidity`, `sql`, `starlark`, `swift`, `svelte`, `systemd`, `tailwind`, `thrift`, `tsv`, `tsx`, `typescript`, `typst`, `v`, `verilog`, `vhdl`, `vue`, `wgsl`, `xml`, `zig`.`

---

### `ruby`
- **Category**: C. Programming Languages
- **Primary Crate Location**: `crates/languages/src/ruby`
- **Path Status**: `Pruned / Not Present`

**Dependencies & References Found:**

#### Manifest Declarations (`Cargo.toml`)
- `Cargo.toml:863:tree-sitter-ruby = "0.23"`

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:57:* **Purge List**: `c`, `cpp`, `c_sharp`, `clojure`, `cmake`, `css`, `dart`, `dockerfile`, `elixir`, `elm`, `erb`, `erlang`, `fortran`, `gleam`, `glsl`, `go`, `groovy`, `haskell`, `helm`, `html`, `java`, `javascript`, `json5`, `julia`, `kotlin`, `lua`, `make`, `nix`, `objc`, `ocaml`, `pascal`, `perl`, `php`, `po`, `proto`, `purescript`, `python`, `r`, `racket`, `ruby`, `rust`, `scala`, `scheme`, `scss`, `solidity`, `sql`, `starlark`, `swift`, `svelte`, `systemd`, `tailwind`, `thrift`, `tsv`, `tsx`, `typescript`, `typst`, `v`, `verilog`, `vhdl`, `vue`, `wgsl`, `xml`, `zig`.`

---

### `rust`
- **Category**: C. Programming Languages
- **Primary Crate Location**: `crates/languages/src/rust`
- **Path Status**: `Pruned / Not Present`

**Dependencies & References Found:**

#### Manifest Declarations (`Cargo.toml`)
- `Cargo.toml:688:notify-rust = "4"`
- `Cargo.toml:787:rust-embed = { version = "8.11", features = ["include-exclude"] }`
- `Cargo.toml:864:tree-sitter-rust = "0.24.2"`
- `Cargo.toml:974:livekit = { git = "https://github.com/zed-industries/livekit-rust-sdks", rev = "d0e27be0cdad89eadab3e36207cda0a2b6e359ee" }`
- `Cargo.toml:975:libwebrtc = { git = "https://github.com/zed-industries/livekit-rust-sdks", rev = "d0e27be0cdad89eadab3e36207cda0a2b6e359ee" }`
- `Cargo.toml:978:webrtc-sys = { git = "https://github.com/zed-industries/livekit-rust-sdks", rev = "d0e27be0cdad89eadab3e36207cda0a2b6e359ee" }`
- `Cargo.toml:1074:[workspace.lints.rust]`
- `Cargo.toml:1096:# rust-analyzer errors), or by having CI fix style nits automatically.`
- `Cargo.toml:1130:# nightly toolchain (see `tooling/lints/rust-toolchain.toml`) and is kept out of`

#### Configuration / Keymaps / Asset Files
- `rustfmt.toml:1:# https://github.com/rust-lang/rustfmt?tab=readme-ov-file#rusts-editions`
- `rust-toolchain.toml:4:components = [ "rustfmt", "clippy", "rust-analyzer", "rust-src" ]`

#### Documentation References (`.md`)
- `plan.md:87:```rust`
- `plan.md:184:```rust`
- `plan.md:232:```rust`
- `plan.md:363:2. Modify `crates/file_finder` to prioritize note title matching and tag search (`tag:#rust`).`
- `plan.md:370:- [The Rust Programming Language Book](https://doc.rust-lang.org/book/)`
- `plan.md:371:- [Asynchronous Programming in Rust](https://rust-lang.github.io/async-book/)`
- `PURGE_MANIFEST.md:41:Zed uses LSP to communicate with external language servers (`rust-analyzer`, `pyright`, `tsserver`). Obsidian uses an internal vault indexer for `[[WikiLinks]]` and `#tags`.`
- `PURGE_MANIFEST.md:57:* **Purge List**: `c`, `cpp`, `c_sharp`, `clojure`, `cmake`, `css`, `dart`, `dockerfile`, `elixir`, `elm`, `erb`, `erlang`, `fortran`, `gleam`, `glsl`, `go`, `groovy`, `haskell`, `helm`, `html`, `java`, `javascript`, `json5`, `julia`, `kotlin`, `lua`, `make`, `nix`, `objc`, `ocaml`, `pascal`, `perl`, `php`, `po`, `proto`, `purescript`, `python`, `r`, `racket`, `ruby`, `rust`, `scala`, `scheme`, `scss`, `solidity`, `sql`, `starlark`, `swift`, `svelte`, `systemd`, `tailwind`, `thrift`, `tsv`, `tsx`, `typescript`, `typst`, `v`, `verilog`, `vhdl`, `vue`, `wgsl`, `xml`, `zig`.`
- `display_markdown.md:36:* **Tree-Sitter Grammars** (`tree-sitter-*`): Language parsers for syntax highlighting code snippets inside fence blocks (e.g., ` ```rust `, ` ```json `, ` ```python `).`
- `gpui_notes.md:120:```rust`
- `gpui_notes.md:131:```rust`
- `gpui_notes.md:147:```rust`
- `text_engine.md:77:```rust`
- `text_engine.md:96:```rust`
- `text_engine.md:169:```rust`

---

### `scala`
- **Category**: C. Programming Languages
- **Primary Crate Location**: `crates/languages/src/scala`
- **Path Status**: `Pruned / Not Present`

**Dependencies & References Found:**

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:57:* **Purge List**: `c`, `cpp`, `c_sharp`, `clojure`, `cmake`, `css`, `dart`, `dockerfile`, `elixir`, `elm`, `erb`, `erlang`, `fortran`, `gleam`, `glsl`, `go`, `groovy`, `haskell`, `helm`, `html`, `java`, `javascript`, `json5`, `julia`, `kotlin`, `lua`, `make`, `nix`, `objc`, `ocaml`, `pascal`, `perl`, `php`, `po`, `proto`, `purescript`, `python`, `r`, `racket`, `ruby`, `rust`, `scala`, `scheme`, `scss`, `solidity`, `sql`, `starlark`, `swift`, `svelte`, `systemd`, `tailwind`, `thrift`, `tsv`, `tsx`, `typescript`, `typst`, `v`, `verilog`, `vhdl`, `vue`, `wgsl`, `xml`, `zig`.`

---

### `scheme`
- **Category**: C. Programming Languages
- **Primary Crate Location**: `crates/languages/src/scheme`
- **Path Status**: `Pruned / Not Present`

**Dependencies & References Found:**

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:57:* **Purge List**: `c`, `cpp`, `c_sharp`, `clojure`, `cmake`, `css`, `dart`, `dockerfile`, `elixir`, `elm`, `erb`, `erlang`, `fortran`, `gleam`, `glsl`, `go`, `groovy`, `haskell`, `helm`, `html`, `java`, `javascript`, `json5`, `julia`, `kotlin`, `lua`, `make`, `nix`, `objc`, `ocaml`, `pascal`, `perl`, `php`, `po`, `proto`, `purescript`, `python`, `r`, `racket`, `ruby`, `rust`, `scala`, `scheme`, `scss`, `solidity`, `sql`, `starlark`, `swift`, `svelte`, `systemd`, `tailwind`, `thrift`, `tsv`, `tsx`, `typescript`, `typst`, `v`, `verilog`, `vhdl`, `vue`, `wgsl`, `xml`, `zig`.`

---

### `scss`
- **Category**: C. Programming Languages
- **Primary Crate Location**: `crates/languages/src/scss`
- **Path Status**: `Pruned / Not Present`

**Dependencies & References Found:**

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:57:* **Purge List**: `c`, `cpp`, `c_sharp`, `clojure`, `cmake`, `css`, `dart`, `dockerfile`, `elixir`, `elm`, `erb`, `erlang`, `fortran`, `gleam`, `glsl`, `go`, `groovy`, `haskell`, `helm`, `html`, `java`, `javascript`, `json5`, `julia`, `kotlin`, `lua`, `make`, `nix`, `objc`, `ocaml`, `pascal`, `perl`, `php`, `po`, `proto`, `purescript`, `python`, `r`, `racket`, `ruby`, `rust`, `scala`, `scheme`, `scss`, `solidity`, `sql`, `starlark`, `swift`, `svelte`, `systemd`, `tailwind`, `thrift`, `tsv`, `tsx`, `typescript`, `typst`, `v`, `verilog`, `vhdl`, `vue`, `wgsl`, `xml`, `zig`.`

---

### `solidity`
- **Category**: C. Programming Languages
- **Primary Crate Location**: `crates/languages/src/solidity`
- **Path Status**: `Pruned / Not Present`

**Dependencies & References Found:**

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:57:* **Purge List**: `c`, `cpp`, `c_sharp`, `clojure`, `cmake`, `css`, `dart`, `dockerfile`, `elixir`, `elm`, `erb`, `erlang`, `fortran`, `gleam`, `glsl`, `go`, `groovy`, `haskell`, `helm`, `html`, `java`, `javascript`, `json5`, `julia`, `kotlin`, `lua`, `make`, `nix`, `objc`, `ocaml`, `pascal`, `perl`, `php`, `po`, `proto`, `purescript`, `python`, `r`, `racket`, `ruby`, `rust`, `scala`, `scheme`, `scss`, `solidity`, `sql`, `starlark`, `swift`, `svelte`, `systemd`, `tailwind`, `thrift`, `tsv`, `tsx`, `typescript`, `typst`, `v`, `verilog`, `vhdl`, `vue`, `wgsl`, `xml`, `zig`.`

---

### `sql`
- **Category**: C. Programming Languages
- **Primary Crate Location**: `crates/languages/src/sql`
- **Path Status**: `Pruned / Not Present`

**Dependencies & References Found:**

#### Configuration / Keymaps / Asset Files
- `typos.toml:21:    "crates/collab/migrations/20251208000000_test_schema.sql",`

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:57:* **Purge List**: `c`, `cpp`, `c_sharp`, `clojure`, `cmake`, `css`, `dart`, `dockerfile`, `elixir`, `elm`, `erb`, `erlang`, `fortran`, `gleam`, `glsl`, `go`, `groovy`, `haskell`, `helm`, `html`, `java`, `javascript`, `json5`, `julia`, `kotlin`, `lua`, `make`, `nix`, `objc`, `ocaml`, `pascal`, `perl`, `php`, `po`, `proto`, `purescript`, `python`, `r`, `racket`, `ruby`, `rust`, `scala`, `scheme`, `scss`, `solidity`, `sql`, `starlark`, `swift`, `svelte`, `systemd`, `tailwind`, `thrift`, `tsv`, `tsx`, `typescript`, `typst`, `v`, `verilog`, `vhdl`, `vue`, `wgsl`, `xml`, `zig`.`
- `plan.md:339:   ```sql`

---

### `starlark`
- **Category**: C. Programming Languages
- **Primary Crate Location**: `crates/languages/src/starlark`
- **Path Status**: `Pruned / Not Present`

**Dependencies & References Found:**

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:57:* **Purge List**: `c`, `cpp`, `c_sharp`, `clojure`, `cmake`, `css`, `dart`, `dockerfile`, `elixir`, `elm`, `erb`, `erlang`, `fortran`, `gleam`, `glsl`, `go`, `groovy`, `haskell`, `helm`, `html`, `java`, `javascript`, `json5`, `julia`, `kotlin`, `lua`, `make`, `nix`, `objc`, `ocaml`, `pascal`, `perl`, `php`, `po`, `proto`, `purescript`, `python`, `r`, `racket`, `ruby`, `rust`, `scala`, `scheme`, `scss`, `solidity`, `sql`, `starlark`, `swift`, `svelte`, `systemd`, `tailwind`, `thrift`, `tsv`, `tsx`, `typescript`, `typst`, `v`, `verilog`, `vhdl`, `vue`, `wgsl`, `xml`, `zig`.`

---

### `swift`
- **Category**: C. Programming Languages
- **Primary Crate Location**: `crates/languages/src/swift`
- **Path Status**: `Pruned / Not Present`

**Dependencies & References Found:**

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:57:* **Purge List**: `c`, `cpp`, `c_sharp`, `clojure`, `cmake`, `css`, `dart`, `dockerfile`, `elixir`, `elm`, `erb`, `erlang`, `fortran`, `gleam`, `glsl`, `go`, `groovy`, `haskell`, `helm`, `html`, `java`, `javascript`, `json5`, `julia`, `kotlin`, `lua`, `make`, `nix`, `objc`, `ocaml`, `pascal`, `perl`, `php`, `po`, `proto`, `purescript`, `python`, `r`, `racket`, `ruby`, `rust`, `scala`, `scheme`, `scss`, `solidity`, `sql`, `starlark`, `swift`, `svelte`, `systemd`, `tailwind`, `thrift`, `tsv`, `tsx`, `typescript`, `typst`, `v`, `verilog`, `vhdl`, `vue`, `wgsl`, `xml`, `zig`.`

---

### `svelte`
- **Category**: C. Programming Languages
- **Primary Crate Location**: `crates/languages/src/svelte`
- **Path Status**: `Pruned / Not Present`

**Dependencies & References Found:**

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:57:* **Purge List**: `c`, `cpp`, `c_sharp`, `clojure`, `cmake`, `css`, `dart`, `dockerfile`, `elixir`, `elm`, `erb`, `erlang`, `fortran`, `gleam`, `glsl`, `go`, `groovy`, `haskell`, `helm`, `html`, `java`, `javascript`, `json5`, `julia`, `kotlin`, `lua`, `make`, `nix`, `objc`, `ocaml`, `pascal`, `perl`, `php`, `po`, `proto`, `purescript`, `python`, `r`, `racket`, `ruby`, `rust`, `scala`, `scheme`, `scss`, `solidity`, `sql`, `starlark`, `swift`, `svelte`, `systemd`, `tailwind`, `thrift`, `tsv`, `tsx`, `typescript`, `typst`, `v`, `verilog`, `vhdl`, `vue`, `wgsl`, `xml`, `zig`.`

---

### `systemd`
- **Category**: C. Programming Languages
- **Primary Crate Location**: `crates/languages/src/systemd`
- **Path Status**: `Pruned / Not Present`

**Dependencies & References Found:**

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:57:* **Purge List**: `c`, `cpp`, `c_sharp`, `clojure`, `cmake`, `css`, `dart`, `dockerfile`, `elixir`, `elm`, `erb`, `erlang`, `fortran`, `gleam`, `glsl`, `go`, `groovy`, `haskell`, `helm`, `html`, `java`, `javascript`, `json5`, `julia`, `kotlin`, `lua`, `make`, `nix`, `objc`, `ocaml`, `pascal`, `perl`, `php`, `po`, `proto`, `purescript`, `python`, `r`, `racket`, `ruby`, `rust`, `scala`, `scheme`, `scss`, `solidity`, `sql`, `starlark`, `swift`, `svelte`, `systemd`, `tailwind`, `thrift`, `tsv`, `tsx`, `typescript`, `typst`, `v`, `verilog`, `vhdl`, `vue`, `wgsl`, `xml`, `zig`.`

---

### `tailwind`
- **Category**: C. Programming Languages
- **Primary Crate Location**: `crates/languages/src/tailwind`
- **Path Status**: `Pruned / Not Present`

**Dependencies & References Found:**

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:57:* **Purge List**: `c`, `cpp`, `c_sharp`, `clojure`, `cmake`, `css`, `dart`, `dockerfile`, `elixir`, `elm`, `erb`, `erlang`, `fortran`, `gleam`, `glsl`, `go`, `groovy`, `haskell`, `helm`, `html`, `java`, `javascript`, `json5`, `julia`, `kotlin`, `lua`, `make`, `nix`, `objc`, `ocaml`, `pascal`, `perl`, `php`, `po`, `proto`, `purescript`, `python`, `r`, `racket`, `ruby`, `rust`, `scala`, `scheme`, `scss`, `solidity`, `sql`, `starlark`, `swift`, `svelte`, `systemd`, `tailwind`, `thrift`, `tsv`, `tsx`, `typescript`, `typst`, `v`, `verilog`, `vhdl`, `vue`, `wgsl`, `xml`, `zig`.`

---

### `thrift`
- **Category**: C. Programming Languages
- **Primary Crate Location**: `crates/languages/src/thrift`
- **Path Status**: `Pruned / Not Present`

**Dependencies & References Found:**

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:57:* **Purge List**: `c`, `cpp`, `c_sharp`, `clojure`, `cmake`, `css`, `dart`, `dockerfile`, `elixir`, `elm`, `erb`, `erlang`, `fortran`, `gleam`, `glsl`, `go`, `groovy`, `haskell`, `helm`, `html`, `java`, `javascript`, `json5`, `julia`, `kotlin`, `lua`, `make`, `nix`, `objc`, `ocaml`, `pascal`, `perl`, `php`, `po`, `proto`, `purescript`, `python`, `r`, `racket`, `ruby`, `rust`, `scala`, `scheme`, `scss`, `solidity`, `sql`, `starlark`, `swift`, `svelte`, `systemd`, `tailwind`, `thrift`, `tsv`, `tsx`, `typescript`, `typst`, `v`, `verilog`, `vhdl`, `vue`, `wgsl`, `xml`, `zig`.`

---

### `tsv`
- **Category**: C. Programming Languages
- **Primary Crate Location**: `crates/languages/src/tsv`
- **Path Status**: `Pruned / Not Present`

**Dependencies & References Found:**

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:57:* **Purge List**: `c`, `cpp`, `c_sharp`, `clojure`, `cmake`, `css`, `dart`, `dockerfile`, `elixir`, `elm`, `erb`, `erlang`, `fortran`, `gleam`, `glsl`, `go`, `groovy`, `haskell`, `helm`, `html`, `java`, `javascript`, `json5`, `julia`, `kotlin`, `lua`, `make`, `nix`, `objc`, `ocaml`, `pascal`, `perl`, `php`, `po`, `proto`, `purescript`, `python`, `r`, `racket`, `ruby`, `rust`, `scala`, `scheme`, `scss`, `solidity`, `sql`, `starlark`, `swift`, `svelte`, `systemd`, `tailwind`, `thrift`, `tsv`, `tsx`, `typescript`, `typst`, `v`, `verilog`, `vhdl`, `vue`, `wgsl`, `xml`, `zig`.`

---

### `tsx`
- **Category**: C. Programming Languages
- **Primary Crate Location**: `crates/languages/src/tsx`
- **Path Status**: `Pruned / Not Present`

**Dependencies & References Found:**

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:57:* **Purge List**: `c`, `cpp`, `c_sharp`, `clojure`, `cmake`, `css`, `dart`, `dockerfile`, `elixir`, `elm`, `erb`, `erlang`, `fortran`, `gleam`, `glsl`, `go`, `groovy`, `haskell`, `helm`, `html`, `java`, `javascript`, `json5`, `julia`, `kotlin`, `lua`, `make`, `nix`, `objc`, `ocaml`, `pascal`, `perl`, `php`, `po`, `proto`, `purescript`, `python`, `r`, `racket`, `ruby`, `rust`, `scala`, `scheme`, `scss`, `solidity`, `sql`, `starlark`, `swift`, `svelte`, `systemd`, `tailwind`, `thrift`, `tsv`, `tsx`, `typescript`, `typst`, `v`, `verilog`, `vhdl`, `vue`, `wgsl`, `xml`, `zig`.`

---

### `typescript`
- **Category**: C. Programming Languages
- **Primary Crate Location**: `crates/languages/src/typescript`
- **Path Status**: `Pruned / Not Present`

**Dependencies & References Found:**

#### Manifest Declarations (`Cargo.toml`)
- `Cargo.toml:865:tree-sitter-typescript = { git = "https://github.com/zed-industries/tree-sitter-typescript", rev = "e2c53597d6a5d9cf7bbe8dccde576fe1e46c5899" } # https://github.com/tree-sitter/tree-sitter-typescript/pull/347`

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:57:* **Purge List**: `c`, `cpp`, `c_sharp`, `clojure`, `cmake`, `css`, `dart`, `dockerfile`, `elixir`, `elm`, `erb`, `erlang`, `fortran`, `gleam`, `glsl`, `go`, `groovy`, `haskell`, `helm`, `html`, `java`, `javascript`, `json5`, `julia`, `kotlin`, `lua`, `make`, `nix`, `objc`, `ocaml`, `pascal`, `perl`, `php`, `po`, `proto`, `purescript`, `python`, `r`, `racket`, `ruby`, `rust`, `scala`, `scheme`, `scss`, `solidity`, `sql`, `starlark`, `swift`, `svelte`, `systemd`, `tailwind`, `thrift`, `tsv`, `tsx`, `typescript`, `typst`, `v`, `verilog`, `vhdl`, `vue`, `wgsl`, `xml`, `zig`.`

---

### `typst`
- **Category**: C. Programming Languages
- **Primary Crate Location**: `crates/languages/src/typst`
- **Path Status**: `Pruned / Not Present`

**Dependencies & References Found:**

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:57:* **Purge List**: `c`, `cpp`, `c_sharp`, `clojure`, `cmake`, `css`, `dart`, `dockerfile`, `elixir`, `elm`, `erb`, `erlang`, `fortran`, `gleam`, `glsl`, `go`, `groovy`, `haskell`, `helm`, `html`, `java`, `javascript`, `json5`, `julia`, `kotlin`, `lua`, `make`, `nix`, `objc`, `ocaml`, `pascal`, `perl`, `php`, `po`, `proto`, `purescript`, `python`, `r`, `racket`, `ruby`, `rust`, `scala`, `scheme`, `scss`, `solidity`, `sql`, `starlark`, `swift`, `svelte`, `systemd`, `tailwind`, `thrift`, `tsv`, `tsx`, `typescript`, `typst`, `v`, `verilog`, `vhdl`, `vue`, `wgsl`, `xml`, `zig`.`

---

### `v`
- **Category**: C. Programming Languages
- **Primary Crate Location**: `crates/languages/src/v`
- **Path Status**: `Pruned / Not Present`

**Dependencies & References Found:**

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:57:* **Purge List**: `c`, `cpp`, `c_sharp`, `clojure`, `cmake`, `css`, `dart`, `dockerfile`, `elixir`, `elm`, `erb`, `erlang`, `fortran`, `gleam`, `glsl`, `go`, `groovy`, `haskell`, `helm`, `html`, `java`, `javascript`, `json5`, `julia`, `kotlin`, `lua`, `make`, `nix`, `objc`, `ocaml`, `pascal`, `perl`, `php`, `po`, `proto`, `purescript`, `python`, `r`, `racket`, `ruby`, `rust`, `scala`, `scheme`, `scss`, `solidity`, `sql`, `starlark`, `swift`, `svelte`, `systemd`, `tailwind`, `thrift`, `tsv`, `tsx`, `typescript`, `typst`, `v`, `verilog`, `vhdl`, `vue`, `wgsl`, `xml`, `zig`.`

---

### `verilog`
- **Category**: C. Programming Languages
- **Primary Crate Location**: `crates/languages/src/verilog`
- **Path Status**: `Pruned / Not Present`

**Dependencies & References Found:**

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:57:* **Purge List**: `c`, `cpp`, `c_sharp`, `clojure`, `cmake`, `css`, `dart`, `dockerfile`, `elixir`, `elm`, `erb`, `erlang`, `fortran`, `gleam`, `glsl`, `go`, `groovy`, `haskell`, `helm`, `html`, `java`, `javascript`, `json5`, `julia`, `kotlin`, `lua`, `make`, `nix`, `objc`, `ocaml`, `pascal`, `perl`, `php`, `po`, `proto`, `purescript`, `python`, `r`, `racket`, `ruby`, `rust`, `scala`, `scheme`, `scss`, `solidity`, `sql`, `starlark`, `swift`, `svelte`, `systemd`, `tailwind`, `thrift`, `tsv`, `tsx`, `typescript`, `typst`, `v`, `verilog`, `vhdl`, `vue`, `wgsl`, `xml`, `zig`.`

---

### `vhdl`
- **Category**: C. Programming Languages
- **Primary Crate Location**: `crates/languages/src/vhdl`
- **Path Status**: `Pruned / Not Present`

**Dependencies & References Found:**

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:57:* **Purge List**: `c`, `cpp`, `c_sharp`, `clojure`, `cmake`, `css`, `dart`, `dockerfile`, `elixir`, `elm`, `erb`, `erlang`, `fortran`, `gleam`, `glsl`, `go`, `groovy`, `haskell`, `helm`, `html`, `java`, `javascript`, `json5`, `julia`, `kotlin`, `lua`, `make`, `nix`, `objc`, `ocaml`, `pascal`, `perl`, `php`, `po`, `proto`, `purescript`, `python`, `r`, `racket`, `ruby`, `rust`, `scala`, `scheme`, `scss`, `solidity`, `sql`, `starlark`, `swift`, `svelte`, `systemd`, `tailwind`, `thrift`, `tsv`, `tsx`, `typescript`, `typst`, `v`, `verilog`, `vhdl`, `vue`, `wgsl`, `xml`, `zig`.`

---

### `vue`
- **Category**: C. Programming Languages
- **Primary Crate Location**: `crates/languages/src/vue`
- **Path Status**: `Pruned / Not Present`

**Dependencies & References Found:**

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:57:* **Purge List**: `c`, `cpp`, `c_sharp`, `clojure`, `cmake`, `css`, `dart`, `dockerfile`, `elixir`, `elm`, `erb`, `erlang`, `fortran`, `gleam`, `glsl`, `go`, `groovy`, `haskell`, `helm`, `html`, `java`, `javascript`, `json5`, `julia`, `kotlin`, `lua`, `make`, `nix`, `objc`, `ocaml`, `pascal`, `perl`, `php`, `po`, `proto`, `purescript`, `python`, `r`, `racket`, `ruby`, `rust`, `scala`, `scheme`, `scss`, `solidity`, `sql`, `starlark`, `swift`, `svelte`, `systemd`, `tailwind`, `thrift`, `tsv`, `tsx`, `typescript`, `typst`, `v`, `verilog`, `vhdl`, `vue`, `wgsl`, `xml`, `zig`.`

---

### `wgsl`
- **Category**: C. Programming Languages
- **Primary Crate Location**: `crates/languages/src/wgsl`
- **Path Status**: `Pruned / Not Present`

**Dependencies & References Found:**

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:57:* **Purge List**: `c`, `cpp`, `c_sharp`, `clojure`, `cmake`, `css`, `dart`, `dockerfile`, `elixir`, `elm`, `erb`, `erlang`, `fortran`, `gleam`, `glsl`, `go`, `groovy`, `haskell`, `helm`, `html`, `java`, `javascript`, `json5`, `julia`, `kotlin`, `lua`, `make`, `nix`, `objc`, `ocaml`, `pascal`, `perl`, `php`, `po`, `proto`, `purescript`, `python`, `r`, `racket`, `ruby`, `rust`, `scala`, `scheme`, `scss`, `solidity`, `sql`, `starlark`, `swift`, `svelte`, `systemd`, `tailwind`, `thrift`, `tsv`, `tsx`, `typescript`, `typst`, `v`, `verilog`, `vhdl`, `vue`, `wgsl`, `xml`, `zig`.`

---

### `xml`
- **Category**: C. Programming Languages
- **Primary Crate Location**: `crates/languages/src/xml`
- **Path Status**: `Pruned / Not Present`

**Dependencies & References Found:**

#### Manifest Declarations (`Cargo.toml`)
- `Cargo.toml:761:quick-xml = "0.38"`

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:57:* **Purge List**: `c`, `cpp`, `c_sharp`, `clojure`, `cmake`, `css`, `dart`, `dockerfile`, `elixir`, `elm`, `erb`, `erlang`, `fortran`, `gleam`, `glsl`, `go`, `groovy`, `haskell`, `helm`, `html`, `java`, `javascript`, `json5`, `julia`, `kotlin`, `lua`, `make`, `nix`, `objc`, `ocaml`, `pascal`, `perl`, `php`, `po`, `proto`, `purescript`, `python`, `r`, `racket`, `ruby`, `rust`, `scala`, `scheme`, `scss`, `solidity`, `sql`, `starlark`, `swift`, `svelte`, `systemd`, `tailwind`, `thrift`, `tsv`, `tsx`, `typescript`, `typst`, `v`, `verilog`, `vhdl`, `vue`, `wgsl`, `xml`, `zig`.`

---

### `zig`
- **Category**: C. Programming Languages
- **Primary Crate Location**: `crates/languages/src/zig`
- **Path Status**: `Pruned / Not Present`

**Dependencies & References Found:**

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:57:* **Purge List**: `c`, `cpp`, `c_sharp`, `clojure`, `cmake`, `css`, `dart`, `dockerfile`, `elixir`, `elm`, `erb`, `erlang`, `fortran`, `gleam`, `glsl`, `go`, `groovy`, `haskell`, `helm`, `html`, `java`, `javascript`, `json5`, `julia`, `kotlin`, `lua`, `make`, `nix`, `objc`, `ocaml`, `pascal`, `perl`, `php`, `po`, `proto`, `purescript`, `python`, `r`, `racket`, `ruby`, `rust`, `scala`, `scheme`, `scss`, `solidity`, `sql`, `starlark`, `swift`, `svelte`, `systemd`, `tailwind`, `thrift`, `tsv`, `tsx`, `typescript`, `typst`, `v`, `verilog`, `vhdl`, `vue`, `wgsl`, `xml`, `zig`.`

---

## D. Server-Side RPC & Multi-User Collaboration Infrastructure
### `collab`
- **Category**: D. Server-Side RPC & Multi-User Collaboration Infrastructure
- **Primary Crate Location**: `crates/collab`
- **Path Status**: `Present`

**Dependencies & References Found:**

#### Manifest Declarations (`Cargo.toml`)
- `Cargo.toml:35:    "crates/collab",`
- `Cargo.toml:306:collab = { path = "crates/collab" }`

#### Configuration / Keymaps / Asset Files
- `lychee.toml:10:    # Don't fail CI check if collab is down`
- `lychee.toml:11:    'https://staging-collab.zed.dev/',`
- `lychee.toml:12:    "https://collab.zed.dev",`
- `typos.toml:21:    "crates/collab/migrations/20251208000000_test_schema.sql",`

#### Documentation References (`.md`)
- `plan.md:285:- `collab`, `collab_ui`, `rpc`, `proto`, `client`, `call`, `livekit_*`, `cloud_*`, `channel`, `remote`, `remote_connection`, `remote_server`, `dev_container`, `sandbox``
- `PURGE_MANIFEST.md:11:2. **Purge IDE & Development Bloat**: Eliminate Debug Adapters (DAP), Language Server Protocol (LSP), embedded terminal emulators, REPLs, remote SSH/WebRTC collaboration (`collab`, `livekit`), and 50+ programming language syntax grammars.`
- `PURGE_MANIFEST.md:67:| `crates/collab` | Remote collaboration backend server | Obsidian vaults run locally on the user's filesystem. |`
- `CONTRIBUTING.md:177:- [`collab`](/crates/collab) is the collaboration server itself, driving the collaboration features such as project sharing.`

---

### `collab_ui`
- **Category**: D. Server-Side RPC & Multi-User Collaboration Infrastructure
- **Primary Crate Location**: `crates/collab_ui`
- **Path Status**: `Present`

**Dependencies & References Found:**

#### Manifest Declarations (`Cargo.toml`)
- `Cargo.toml:36:    "crates/collab_ui",`
- `Cargo.toml:307:collab_ui = { path = "crates/collab_ui" }`

#### Documentation References (`.md`)
- `plan.md:285:- `collab`, `collab_ui`, `rpc`, `proto`, `client`, `call`, `livekit_*`, `cloud_*`, `channel`, `remote`, `remote_connection`, `remote_server`, `dev_container`, `sandbox``
- `PURGE_MANIFEST.md:68:| `crates/collab_ui` | Peer avatars, follow mode, and channel chat UI | IDE co-editing UI; unneeded in note app. |`

---

### `livekit_api`
- **Category**: D. Server-Side RPC & Multi-User Collaboration Infrastructure
- **Primary Crate Location**: `crates/livekit_api`
- **Path Status**: `Present`

**Dependencies & References Found:**

#### Manifest Declarations (`Cargo.toml`)
- `Cargo.toml:128:    "crates/livekit_api",`
- `Cargo.toml:394:livekit_api = { path = "crates/livekit_api" }`

#### Configuration / Keymaps / Asset Files
- `typos.toml:27:    "crates/livekit_api/",`

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:69:| `crates/livekit_api` | WebRTC audio/video call integration | Voice/video channel streaming for remote dev teams. |`

---

### `livekit_client`
- **Category**: D. Server-Side RPC & Multi-User Collaboration Infrastructure
- **Primary Crate Location**: `crates/livekit_client`
- **Path Status**: `Present`

**Dependencies & References Found:**

#### Manifest Declarations (`Cargo.toml`)
- `Cargo.toml:129:    "crates/livekit_client",`
- `Cargo.toml:395:livekit_client = { path = "crates/livekit_client" }`

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:70:| `crates/livekit_client` | LiveKit WebRTC client integration | Video/voice client; unnecessary overhead. |`

---

### `call`
- **Category**: D. Server-Side RPC & Multi-User Collaboration Infrastructure
- **Primary Crate Location**: `crates/call`
- **Path Status**: `Present`

**Dependencies & References Found:**

#### Manifest Declarations (`Cargo.toml`)
- `Cargo.toml:26:    "crates/call",`
- `Cargo.toml:297:call = { path = "crates/call" }`

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:69:| `crates/livekit_api` | WebRTC audio/video call integration | Voice/video channel streaming for remote dev teams. |`
- `PURGE_MANIFEST.md:71:| `crates/call` | Call management logic | Voice call session control. |`
- `plan.md:285:- `collab`, `collab_ui`, `rpc`, `proto`, `client`, `call`, `livekit_*`, `cloud_*`, `channel`, `remote`, `remote_connection`, `remote_server`, `dev_container`, `sandbox``

---

### `channel`
- **Category**: D. Server-Side RPC & Multi-User Collaboration Infrastructure
- **Primary Crate Location**: `crates/channel`
- **Path Status**: `Present`

**Dependencies & References Found:**

#### Manifest Declarations (`Cargo.toml`)
- `Cargo.toml:27:    "crates/channel",`
- `Cargo.toml:298:channel = { path = "crates/channel" }`
- `Cargo.toml:531:async-channel = "2.5.0"`

#### Configuration / Keymaps / Asset Files
- `rust-toolchain.toml:2:channel = "1.97.1"`

#### Documentation References (`.md`)
- `plan.md:67:- **Channels**: Uses `postage::watch`, `async-channel`, and `smol::channel` for lock-free message passing between background worker threads and the main UI loop.`
- `plan.md:285:- `collab`, `collab_ui`, `rpc`, `proto`, `client`, `call`, `livekit_*`, `cloud_*`, `channel`, `remote`, `remote_connection`, `remote_server`, `dev_container`, `sandbox``
- `PURGE_MANIFEST.md:68:| `crates/collab_ui` | Peer avatars, follow mode, and channel chat UI | IDE co-editing UI; unneeded in note app. |`
- `PURGE_MANIFEST.md:69:| `crates/livekit_api` | WebRTC audio/video call integration | Voice/video channel streaming for remote dev teams. |`
- `PURGE_MANIFEST.md:72:| `crates/channel` | Text chat channels for collaborative rooms | IDE room chat system. |`

---

### `rpc`
- **Category**: D. Server-Side RPC & Multi-User Collaboration Infrastructure
- **Primary Crate Location**: `crates/rpc`
- **Path Status**: `Present`

**Dependencies & References Found:**

#### Manifest Declarations (`Cargo.toml`)
- `Cargo.toml:180:    "crates/rpc",`
- `Cargo.toml:446:rpc = { path = "crates/rpc" }`

#### Configuration / Keymaps / Asset Files
- `typos.toml:39:    "crates/rpc/src/auth.rs",`

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:73:| `crates/rpc` | Protocol buffers & async RPC transport layer | Backend client-server protocol. |`
- `plan.md:285:- `collab`, `collab_ui`, `rpc`, `proto`, `client`, `call`, `livekit_*`, `cloud_*`, `channel`, `remote`, `remote_connection`, `remote_server`, `dev_container`, `sandbox``
- `CONTRIBUTING.md:178:- [`rpc`](/crates/rpc) defines messages to be exchanged with collaboration server.`

---

### `proto`
- **Category**: D. Server-Side RPC & Multi-User Collaboration Infrastructure
- **Primary Crate Location**: `crates/proto`
- **Path Status**: `Present`

**Dependencies & References Found:**

#### Manifest Declarations (`Cargo.toml`)
- `Cargo.toml:168:    "crates/proto",`
- `Cargo.toml:254:    "extensions/proto",`
- `Cargo.toml:434:proto = { path = "crates/proto" }`

#### Configuration / Keymaps / Asset Files
- `typos.toml:43:    "extensions/proto/extension.toml",`
- `typos.toml:44:    "extensions/proto/src/language_servers/protols.rs",`

#### Documentation References (`.md`)
- `plan.md:285:- `collab`, `collab_ui`, `rpc`, `proto`, `client`, `call`, `livekit_*`, `cloud_*`, `channel`, `remote`, `remote_connection`, `remote_server`, `dev_container`, `sandbox``
- `PURGE_MANIFEST.md:57:* **Purge List**: `c`, `cpp`, `c_sharp`, `clojure`, `cmake`, `css`, `dart`, `dockerfile`, `elixir`, `elm`, `erb`, `erlang`, `fortran`, `gleam`, `glsl`, `go`, `groovy`, `haskell`, `helm`, `html`, `java`, `javascript`, `json5`, `julia`, `kotlin`, `lua`, `make`, `nix`, `objc`, `ocaml`, `pascal`, `perl`, `php`, `po`, `proto`, `purescript`, `python`, `r`, `racket`, `ruby`, `rust`, `scala`, `scheme`, `scss`, `solidity`, `sql`, `starlark`, `swift`, `svelte`, `systemd`, `tailwind`, `thrift`, `tsv`, `tsx`, `typescript`, `typst`, `v`, `verilog`, `vhdl`, `vue`, `wgsl`, `xml`, `zig`.`
- `PURGE_MANIFEST.md:74:| `crates/proto` | Generated protobuf definitions for backend RPC | Server communication schemas. |`

---

### `client`
- **Category**: D. Server-Side RPC & Multi-User Collaboration Infrastructure
- **Primary Crate Location**: `crates/client`
- **Path Status**: `Present`

**Dependencies & References Found:**

#### Manifest Declarations (`Cargo.toml`)
- `Cargo.toml:29:    "crates/client",`
- `Cargo.toml:300:client = { path = "crates/client" }`
- `Cargo.toml:518:agent-client-protocol = { version = "=2.0.0", features = ["unstable"] }`
- `Cargo.toml:556:aws-smithy-runtime-api = { version = "1.9.2", features = ["http-1x", "client"] }`
- `Cargo.toml:670:jupyter-websocket-client = "1.1.0"`

#### Documentation References (`.md`)
- `plan.md:285:- `collab`, `collab_ui`, `rpc`, `proto`, `client`, `call`, `livekit_*`, `cloud_*`, `channel`, `remote`, `remote_connection`, `remote_server`, `dev_container`, `sandbox``
- `PURGE_MANIFEST.md:25:| `crates/dap` | Debug Adapter Protocol client | Not needed for Markdown document editing. |`
- `PURGE_MANIFEST.md:45:| `crates/lsp` | LSP client implementation (JSON-RPC protocol for diagnostics, completion) | Replaced by internal SQLite Wikilink & Tag indexer. |`
- `PURGE_MANIFEST.md:70:| `crates/livekit_client` | LiveKit WebRTC client integration | Video/voice client; unnecessary overhead. |`
- `PURGE_MANIFEST.md:73:| `crates/rpc` | Protocol buffers & async RPC transport layer | Backend client-server protocol. |`
- `PURGE_MANIFEST.md:75:| `crates/client` | Connection client for Zed's cloud service | Centralized cloud server auth & connection. |`
- `PURGE_MANIFEST.md:76:| `crates/cloud_api_client` | Cloud API client | Server API interface. |`
- `PURGE_MANIFEST.md:78:| `crates/cloud_llm_client` | Proxy client for Zed's hosted LLM service | Direct local/API key LLM access is preferred via kept AI crates. |`

---

### `cloud_api_client`
- **Category**: D. Server-Side RPC & Multi-User Collaboration Infrastructure
- **Primary Crate Location**: `crates/cloud_api_client`
- **Path Status**: `Present`

**Dependencies & References Found:**

#### Manifest Declarations (`Cargo.toml`)
- `Cargo.toml:31:    "crates/cloud_api_client",`
- `Cargo.toml:302:cloud_api_client = { path = "crates/cloud_api_client" }`

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:76:| `crates/cloud_api_client` | Cloud API client | Server API interface. |`

---

### `cloud_api_types`
- **Category**: D. Server-Side RPC & Multi-User Collaboration Infrastructure
- **Primary Crate Location**: `crates/cloud_api_types`
- **Path Status**: `Present`

**Dependencies & References Found:**

#### Manifest Declarations (`Cargo.toml`)
- `Cargo.toml:32:    "crates/cloud_api_types",`
- `Cargo.toml:303:cloud_api_types = { path = "crates/cloud_api_types" }`

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:77:| `crates/cloud_api_types` | Data types for Zed cloud backend | Cloud schema types. |`

---

### `cloud_llm_client`
- **Category**: D. Server-Side RPC & Multi-User Collaboration Infrastructure
- **Primary Crate Location**: `crates/cloud_llm_client`
- **Path Status**: `Present`

**Dependencies & References Found:**

#### Manifest Declarations (`Cargo.toml`)
- `Cargo.toml:33:    "crates/cloud_llm_client",`
- `Cargo.toml:304:cloud_llm_client = { path = "crates/cloud_llm_client" }`

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:78:| `crates/cloud_llm_client` | Proxy client for Zed's hosted LLM service | Direct local/API key LLM access is preferred via kept AI crates. |`

---

### `remote`
- **Category**: D. Server-Side RPC & Multi-User Collaboration Infrastructure
- **Primary Crate Location**: `crates/remote`
- **Path Status**: `Present`

**Dependencies & References Found:**

#### Manifest Declarations (`Cargo.toml`)
- `Cargo.toml:174:    "crates/remote",`
- `Cargo.toml:439:remote = { path = "crates/remote" }`

#### Configuration / Keymaps / Asset Files
- `rust-toolchain.toml:8:    "x86_64-unknown-linux-musl", # remote server`

#### Documentation References (`.md`)
- `plan.md:285:- `collab`, `collab_ui`, `rpc`, `proto`, `client`, `call`, `livekit_*`, `cloud_*`, `channel`, `remote`, `remote_connection`, `remote_server`, `dev_container`, `sandbox``
- `PURGE_MANIFEST.md:11:2. **Purge IDE & Development Bloat**: Eliminate Debug Adapters (DAP), Language Server Protocol (LSP), embedded terminal emulators, REPLs, remote SSH/WebRTC collaboration (`collab`, `livekit`), and 50+ programming language syntax grammars.`
- `PURGE_MANIFEST.md:63:Zed includes custom backend server components for remote pair-programming, screen sharing, and server-side RPC. Obsidian is a local-first application operating on local directories.`
- `PURGE_MANIFEST.md:69:| `crates/livekit_api` | WebRTC audio/video call integration | Voice/video channel streaming for remote dev teams. |`
- `PURGE_MANIFEST.md:79:| `crates/remote` | Remote SSH and headless editor engine | Headless server editing. |`
- `PURGE_MANIFEST.md:81:| `crates/remote_server` | Binary executed on remote Linux hosts | Headless remote agent binary. |`
- `display_markdown.md:53:To display local and remote images embedded in Markdown documents:`
- `display_markdown.md:58:* **Async HTTP Loader (`http_client`)**: Background fetching of remote images (`https://...`) without blocking UI thread layout.`

---

### `remote_connection`
- **Category**: D. Server-Side RPC & Multi-User Collaboration Infrastructure
- **Primary Crate Location**: `crates/remote_connection`
- **Path Status**: `Present`

**Dependencies & References Found:**

#### Manifest Declarations (`Cargo.toml`)
- `Cargo.toml:175:    "crates/remote_connection",`
- `Cargo.toml:440:remote_connection = { path = "crates/remote_connection" }`

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:80:| `crates/remote_connection` | SSH/TCP tunnel establishment | Remote development server connection. |`
- `plan.md:285:- `collab`, `collab_ui`, `rpc`, `proto`, `client`, `call`, `livekit_*`, `cloud_*`, `channel`, `remote`, `remote_connection`, `remote_server`, `dev_container`, `sandbox``

---

### `remote_server`
- **Category**: D. Server-Side RPC & Multi-User Collaboration Infrastructure
- **Primary Crate Location**: `crates/remote_server`
- **Path Status**: `Present`

**Dependencies & References Found:**

#### Manifest Declarations (`Cargo.toml`)
- `Cargo.toml:176:    "crates/remote_server",`
- `Cargo.toml:441:remote_server = { path = "crates/remote_server" }`

#### Documentation References (`.md`)
- `plan.md:285:- `collab`, `collab_ui`, `rpc`, `proto`, `client`, `call`, `livekit_*`, `cloud_*`, `channel`, `remote`, `remote_connection`, `remote_server`, `dev_container`, `sandbox``
- `PURGE_MANIFEST.md:81:| `crates/remote_server` | Binary executed on remote Linux hosts | Headless remote agent binary. |`

---

### `dev_container`
- **Category**: D. Server-Side RPC & Multi-User Collaboration Infrastructure
- **Primary Crate Location**: `crates/dev_container`
- **Path Status**: `Present`

**Dependencies & References Found:**

#### Manifest Declarations (`Cargo.toml`)
- `Cargo.toml:54:    "crates/dev_container",`
- `Cargo.toml:328:dev_container = { path = "crates/dev_container" }`

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:82:| `crates/dev_container` | Docker dev container orchestration | Software development environment provisioning. |`
- `plan.md:285:- `collab`, `collab_ui`, `rpc`, `proto`, `client`, `call`, `livekit_*`, `cloud_*`, `channel`, `remote`, `remote_connection`, `remote_server`, `dev_container`, `sandbox``

---

### `sandbox`
- **Category**: D. Server-Side RPC & Multi-User Collaboration Infrastructure
- **Primary Crate Location**: `crates/sandbox`
- **Path Status**: `Present`

**Dependencies & References Found:**

#### Manifest Declarations (`Cargo.toml`)
- `Cargo.toml:181:    "crates/sandbox",`
- `Cargo.toml:447:sandbox = { path = "crates/sandbox" }`

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:83:| `crates/sandbox` | Process sandboxing for code execution | Code execution isolation. |`
- `plan.md:285:- `collab`, `collab_ui`, `rpc`, `proto`, `client`, `call`, `livekit_*`, `cloud_*`, `channel`, `remote`, `remote_connection`, `remote_server`, `dev_container`, `sandbox``

---

## E. Code Edit Prediction & Specialized Code Generation Models
### `edit_prediction`
- **Category**: E. Code Edit Prediction & Specialized Code Generation Models
- **Primary Crate Location**: `crates/edit_prediction`
- **Path Status**: `Present`

**Dependencies & References Found:**

#### Manifest Declarations (`Cargo.toml`)
- `Cargo.toml:57:    "crates/edit_prediction",`
- `Cargo.toml:330:edit_prediction = { path = "crates/edit_prediction" }`

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:92:| `crates/edit_prediction` | Next-edit prediction engine | Designed for code auto-completion; not applicable to note taking. |`
- `plan.md:291:- `agent`, `agent_*`, `anthropic`, `copilot*`, `open_ai`, `deepseek`, `ollama`, `language_model*`, `google_ai`, `bedrock`, `codestral`, `x_ai`, `edit_prediction*``

---

### `edit_prediction_ui`
- **Category**: E. Code Edit Prediction & Specialized Code Generation Models
- **Primary Crate Location**: `crates/edit_prediction_ui`
- **Path Status**: `Present`

**Dependencies & References Found:**

#### Manifest Declarations (`Cargo.toml`)
- `Cargo.toml:62:    "crates/edit_prediction_ui",`
- `Cargo.toml:334:edit_prediction_ui = { path = "crates/edit_prediction_ui" }`
- `Cargo.toml:1031:edit_prediction_ui = { codegen-units = 1 }`

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:93:| `crates/edit_prediction_ui` | UI overlay for code edit predictions | Inline code ghost text UI. |`

---

### `edit_prediction_cli`
- **Category**: E. Code Edit Prediction & Specialized Code Generation Models
- **Primary Crate Location**: `crates/edit_prediction_cli`
- **Path Status**: `Present`

**Dependencies & References Found:**

#### Manifest Declarations (`Cargo.toml`)
- `Cargo.toml:58:    "crates/edit_prediction_cli",`

#### Configuration / Keymaps / Asset Files
- `typos.toml:69:    "crates/edit_prediction_cli/src/split_commit.rs",`
- `typos.toml:72:    "crates/edit_prediction_cli/evals/",`

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:94:| `crates/edit_prediction_cli` | CLI evaluation for edit predictions | Evaluation binary. |`

---

### `edit_prediction_context`
- **Category**: E. Code Edit Prediction & Specialized Code Generation Models
- **Primary Crate Location**: `crates/edit_prediction_context`
- **Path Status**: `Present`

**Dependencies & References Found:**

#### Manifest Declarations (`Cargo.toml`)
- `Cargo.toml:59:    "crates/edit_prediction_context",`
- `Cargo.toml:331:edit_prediction_context = { path = "crates/edit_prediction_context" }`

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:95:| `crates/edit_prediction_context`| AST context generator for code completions | Code AST context extractor. |`

---

### `edit_prediction_metrics`
- **Category**: E. Code Edit Prediction & Specialized Code Generation Models
- **Primary Crate Location**: `crates/edit_prediction_metrics`
- **Path Status**: `Present`

**Dependencies & References Found:**

#### Manifest Declarations (`Cargo.toml`)
- `Cargo.toml:60:    "crates/edit_prediction_metrics",`
- `Cargo.toml:332:edit_prediction_metrics = { path = "crates/edit_prediction_metrics" }`

#### Configuration / Keymaps / Asset Files
- `typos.toml:70:    "crates/edit_prediction_metrics/src/kept_rate.rs",`

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:96:| `crates/edit_prediction_metrics`| Metrics telemetry for code inline completions | Code completion telemetry. |`

---

### `edit_prediction_types`
- **Category**: E. Code Edit Prediction & Specialized Code Generation Models
- **Primary Crate Location**: `crates/edit_prediction_types`
- **Path Status**: `Present`

**Dependencies & References Found:**

#### Manifest Declarations (`Cargo.toml`)
- `Cargo.toml:61:    "crates/edit_prediction_types",`
- `Cargo.toml:333:edit_prediction_types = { path = "crates/edit_prediction_types" }`

#### Documentation References (`.md`)
- `PURGE_MANIFEST.md:97:| `crates/edit_prediction_types`  | Structs for code prediction | Type definitions. |`

---

