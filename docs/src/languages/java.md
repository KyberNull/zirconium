---
title: Java
description: "Configure Java language support in Zed, including formatting and syntax highlighting."
---

# Java

Java language support in Zed is provided by:

- Zed Java: [zed-extensions/java](https://github.com/zed-extensions/java)
- Tree-sitter: [tree-sitter/tree-sitter-java](https://github.com/tree-sitter/tree-sitter-java)

## Install OpenJDK

You will need to install a Java runtime (OpenJDK).

- macOS: `brew install openjdk`
- Ubuntu: `sudo add-apt-repository ppa:openjdk-23 && sudo apt-get install openjdk-23`
- Windows: `choco install openjdk`
- Arch Linux: `sudo pacman -S jre-openjdk-headless`

Or manually download and install [OpenJDK 23](https://jdk.java.net/23/).

## Extension Install

You can install by opening {#action zed::Extensions}({#kb zed::Extensions}) and searching for `java`.

## Launch Scripts (aka Tasks) in Windows

This extension provides tasks for running your application and tests from within Zed via little play buttons next to tests/entry points. However, due to current limitations of Zed's extension interface, we can not provide scripts that will work across Maven and Gradle on both Windows and Unix-compatible systems, so out of the box the launch scripts only work on Mac and Linux.

There is a fairly straightforward fix that you can apply to make it work on Windows by supplying your own task scripts. Please see [this Issue](https://github.com/zed-extensions/java/issues/94) for information on how to do that and read the [Tasks section in Zed's documentation](https://zed.dev/docs/tasks) for more information.

## See also

[Zed Java Repo](https://github.com/zed-extensions/java)
[Eclipse JDTLS Repo](https://github.com/eclipse-jdtls/eclipse.jdt.ls)
