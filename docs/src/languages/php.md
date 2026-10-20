---
title: PHP
description: "Configure PHP language support in Zed, including formatting and syntax highlighting."
---

# PHP

PHP support is available through the [PHP extension](https://github.com/zed-extensions/php).

- Tree-sitter: [tree-sitter/tree-sitter-php](https://github.com/tree-sitter/tree-sitter-php)

## Install PHP

The PHP extension requires PHP to be installed and available in your `PATH`:

```sh
# macOS via Homebrew
brew install php

# Debian/Ubuntu
sudo apt-get install php-cli

# CentOS 8+/RHEL
sudo dnf install php-cli

# Arch Linux
sudo pacman -S php

# check PHP path
## macOS and Linux
which php

## Windows
where php
```

## PHPDoc

Zed supports syntax highlighting for PHPDoc comments.

- Tree-sitter: [claytonrcarter/tree-sitter-phpdoc](https://github.com/claytonrcarter/tree-sitter-phpdoc)
