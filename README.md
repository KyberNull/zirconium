# Zirconium

**Write at the speed of thought.**

Zirconium is a note-taking app inspired by [Obsidian](https://obsidian.md), built on [Zed](https://github.com/zed-industries/zed).

---

### Development

Zirconium is a work in progress.

- Build and check: `cargo check --workspace --all-targets`
- Lint: `./script/clippy`
- Test: `cargo nextest run --workspace` (or `-p <crate>` for one crate). Nextest skips doctests, so run `cargo test --doc -p <crate>` for those.

See [CONTRIBUTING.md](CONTRIBUTING.md) before opening a pull request.

### Acknowledgements

Zirconium builds on [Zed](https://github.com/zed-industries/zed). See the upstream repository for its source code and development documentation.

### Licensing

Zirconium builds on Zed, which is licensed primarily under GPL-3.0-or-later, with some components under Apache-2.0. See the upstream [repository](https://github.com/zed-industries/zed) and the license files included in this project for applicable terms.
