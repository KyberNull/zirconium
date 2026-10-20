# Contributing to Zirconium

Zirconium is a Markdown-first note-taking app built on [Zed](https://github.com/zed-industries/zed). Much of the inherited Zed code is being purged, so check the open [issues](https://github.com/KyberNull/zirconium/issues) before starting work.

## Before you start

- Open or comment on an issue first for anything larger than a small fix, so the approach can be agreed before you write code.
- Keep each pull request to one change.

## Development

- Check: `cargo check --workspace --all-targets`
- Lint: `./script/clippy` (not `cargo clippy`)
- Test: `cargo nextest run --workspace`, or `cargo nextest run -p <crate>` for one crate. Nextest skips doctests; run `cargo test --doc -p <crate>` for those.

Follow the coding guidelines in [`.rules`](.rules).

## Pull requests

- Use a clear, capitalized, imperative title without a conventional-commit prefix or trailing punctuation.
- Link the issue with `Closes #<number>`.
- Describe what changed and how you verified it.
- End the description with a `Release Notes:` section: a blank line, then one bullet (`- Added ...`, `- Fixed ...`, `- Improved ...`, or `- N/A`).

## Licensing

Zirconium builds on Zed and keeps its licenses: GPL-3.0-or-later for most code, with some components under Apache-2.0. See the license files in this repository.
