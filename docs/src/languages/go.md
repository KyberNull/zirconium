---
title: Go
description: "Configure Go language support in Zed, including language servers, formatting, and debugging."
---

# Go

Go support is available natively in Zed.

- Tree-sitter: [tree-sitter/tree-sitter-go](https://github.com/tree-sitter/tree-sitter-go)
- Language Server: [golang/tools/tree/master/gopls](https://github.com/golang/tools/tree/master/gopls)

## Setup

We recommend installing gopls via go's package manager and not via Homebrew or your Linux distribution's package manager.

1. Make sure you have uninstalled any version of gopls you have installed via your package manager:

```sh
# MacOS homebrew
brew remove gopls
# Ubuntu
sudo apt-get remove gopls
sudo snap remove gopls
# Arch
sudo pacman -R gopls
```

2. Install/Update `gopls` to the latest version using the go module tool:

```sh
go install golang.org/x/tools/gopls@latest
```

3. Ensure that `gopls` is in your path:

```sh
which gopls
gopls version
```

If `gopls` is not found you will likely need to add `export PATH="$PATH:$HOME/go/bin"` to your `.zshrc` / `.bash_profile`

## Inlay Hints

Zed sets the following initialization options for inlay hints:

```json
"hints": {
    "assignVariableTypes": true,
    "compositeLiteralFields": true,
    "compositeLiteralTypes": true,
    "constantValues": true,
    "functionTypeParameters": true,
    "parameterNames": true,
    "rangeVariableTypes": true
}
```

to make the language server send back inlay hints when Zed has them enabled in the settings.

Use

```json
"lsp": {
    "gopls": {
        "initialization_options": {
            "hints": {
                // ....
            }
        }
    }
}
```

to override these settings.

See [gopls inlayHints documentation](https://github.com/golang/tools/blob/master/gopls/doc/inlayHints.md) for more information.

## Code Lens

Zed enables the `test` code lens for `gopls` by default. This shows "run test" and "run benchmark" links above `Test` and `Benchmark` functions in `*_test.go` files. To use them, enable the `code_lens` setting:

```json [settings]
{
  "code_lens": "on"
}
```

You can override the default code lens settings in your `settings.json`:

```json [settings]
{
  "lsp": {
    "gopls": {
      "initialization_options": {
        "codelenses": {
          "test": true,
          "generate": true,
          "regenerate_cgo": true,
          "tidy": true,
          "upgrade_dependency": true,
          "vendor": true
        }
      }
    }
  }
}
```

See [gopls code lenses documentation](https://go.dev/gopls/codelenses) for more information.


## Go Mod

- Tree-sitter: [camdencheek/tree-sitter-go-mod](https://github.com/camdencheek/tree-sitter-go-mod)
- Language Server: N/A

## Go Sum

- Tree-sitter: [amaanq/tree-sitter-go-sum](https://github.com/amaanq/tree-sitter-go-sum)
- Language Server: N/A

## Go Work

- Tree-sitter:
  [tree-sitter-go-work](https://github.com/d1y/tree-sitter-go-work)
- Language Server: N/A

## Using the Tailwind CSS Language Server with Templ

To get all the features (autocomplete, linting, etc.) from the [Tailwind CSS language server](https://github.com/tailwindlabs/tailwindcss-intellisense/tree/HEAD/packages/tailwindcss-language-server#readme) in [Templ](https://github.com/a-h/templ) files, you need to enable the language server for Templ and configure where it should look for CSS classes by adding the following to your `settings.json`:

```json [settings]
{
  "languages": {
    "Templ": {
      "language_servers": ["tailwindcss-language-server", "..."]
    }
  },
  "lsp": {
    "tailwindcss-language-server": {
      "settings": {
        "includeLanguages": {
          "templ": "html"
        },
        "experimental": {
          "classRegex": ["class=\"([^\"]*)\""]
        }
      }
    }
  }
}
```

> Note: Unlike other languages, you need to tell Tailwind to treat `.templ` files as HTML explicitly.

This gives you Tailwind CSS completions inside `class="..."` attributes in your `.templ` files.
