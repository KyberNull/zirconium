---
title: R
description: "Configure R language support in Zed, including formatting and syntax highlighting."
---

# R

R support is available via multiple R Zed extensions:

- [posit-dev/air](https://github.com/posit-dev/air/tree/main/editors/zed)
  - Formatter: [posit-dev/air](https://posit-dev.github.io/air/)

## REPL

### Ark Installation

To use the Zed REPL with R you need to install [Ark](https://github.com/posit-dev/ark), an R Kernel for Jupyter applications.
You can download the latest version from the [Ark GitHub Releases](https://github.com/posit-dev/ark/releases) and then extract the `ark` binary to a directory in your `PATH`.

For example to install the latest non-debug build:

```sh
# macOS
cd /tmp
curl -L -o ark-latest-darwin.zip \
    $(curl -s "https://api.github.com/repos/posit-dev/ark/releases/latest" | \
    jq -r '.assets[] | select(.name | contains("darwin-universal") and (contains("debug") | not)) | .browser_download_url')
unzip ark-latest-darwin.zip ark
sudo mv /tmp/ark /usr/local/bin/
```

```sh
# Linux X86_64
cd /tmp
curl -L -o ark-latest-linux.zip \
    $(curl -s "https://api.github.com/repos/posit-dev/ark/releases/latest" \
        | jq -r '.assets[] | select(.name | contains("linux-x64") and (contains("debug") | not)) | .browser_download_url'
    )
unzip ark-latest-linux.zip ark
sudo mv /tmp/ark /usr/local/bin/
```

-->
