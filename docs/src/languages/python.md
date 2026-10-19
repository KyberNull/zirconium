---
title: Python
description: "Configure Python language support in Zed, including formatting."
---

# How to Set Up Python in Zed

Python support is available natively in Zed.

- Tree-sitter: [tree-sitter-python](https://github.com/zed-industries/tree-sitter-python)

## Install Python

You'll need both Zed and Python installed before you can begin.

### Step 1: Install Python

Zed does not bundle a Python runtime, so you’ll need to install one yourself.
Choose one of the following options:

- uv (recommended)

```bash
curl -LsSf https://astral.sh/uv/install.sh | sh
```

To learn more, visit [Astral’s installation guide](https://docs.astral.sh/uv/getting-started/installation/).

- Homebrew:

```bash
brew install python
```

- Python.org installer: Download the latest version from [python.org/downloads](https://python.org/downloads).

### Step 2: Verify Python Installation

Confirm Python is installed and available in your shell:

```bash
python3 --version
```

You should see an output like `Python 3.x.x`.

## Open Your First Python Project in Zed

Once Zed and Python are installed, open a folder containing Python code to start working.

### Step 1: Launch Zed with a Python Project

Open Zed.
From the menu bar, choose File > Open Folder, or launch from the terminal:

```bash
zed path/to/your/project
```

Zed will recognize `.py` files automatically using its native tree-sitter-python parser, with no plugins or manual setup required.

### Step 2: Use the Integrated Terminal (Optional)

Zed includes an integrated terminal, accessible from the bottom panel. If Zed detects that your project is using a [virtual environment](#virtual-environments), it will be activated automatically in newly-created terminals. You can configure this behavior with the [`detect_venv`](../reference/all-settings.md#terminal-detect_venv) setting.

## Virtual Environments

[Virtual environments](https://docs.python.org/3/library/venv.html) are a useful tool for fixing a Python version and set of dependencies for a specific project, in a way that's isolated from other projects on the same machine. Zed has built-in support for discovering, configuring, and activating virtual environments, based on the language-agnostic concept of a [toolchain](../toolchains.md).

Note that if you have a global Python installation, it is also counted as a toolchain for Zed's purposes.

### Create a Virtual Environment

If your project doesn't have a virtual environment set up already, you can create one as follows:

```bash
python3 -m venv .venv
```

Alternatively, if you're using `uv`, running `uv sync` will create a virtual environment the first time you run it.

### Selecting a Toolchain

For most projects, Zed will automatically select the right Python toolchain. In complex projects with multiple virtual environments, it might be necessary to override this selection. You can use the [toolchain selector](../toolchains.md#selecting-toolchains) to pick a toolchain from the list discovered by Zed, or [specify the path to a toolchain manually](../toolchains.md#adding-toolchains-manually) if it's not on the list.
