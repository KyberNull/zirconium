---
title: Ansible
description: "Configure Ansible language support in Zed, including formatting and syntax highlighting."
---

# Ansible

Support for Ansible in Zed is provided via a community-maintained [Ansible extension](https://github.com/kartikvashistha/zed-ansible).

- Tree-sitter: [zed-industries/tree-sitter-yaml](https://github.com/zed-industries/tree-sitter-yaml)

## Setup

### File detection

To avoid mishandling non-Ansible YAML files, the Ansible Language is not associated with any file extensions by default.

To change this behavior, you can add a `"file_types"` section to Zed settings inside your project (`.zed/settings.json`) or your Zed user settings (`~/.config/zed/settings.json`) to match your folder/naming conventions. For example:

```json [settings]
{
  "file_types": {
    "Ansible": [
      "**.ansible.yml",
      "**.ansible.yaml",
      "**/defaults/*.yml",
      "**/defaults/*.yaml",
      "**/meta/*.yml",
      "**/meta/*.yaml",
      "**/tasks/*.yml",
      "**/tasks/*.yaml",
      "**/handlers/*.yml",
      "**/handlers/*.yaml",
      "**/group_vars/*.yml",
      "**/group_vars/*.yaml",
      "**/host_vars/*.yml",
      "**/host_vars/*.yaml",
      "**/playbooks/*.yml",
      "**/playbooks/*.yaml",
      "**playbook*.yml",
      "**playbook*.yaml"
    ]
  }
}
```

Feel free to modify this list as per your needs.
