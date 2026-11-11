---
title: Contributing
description: Build Sigilfall from source, run the tests and the lints, use a separate data folder, and build this documentation site.
---

## Requirements

- Rust 1.88 or later. `rust-toolchain.toml` pins the stable channel with
  `clippy` and `rustfmt`; `rustup toolchain install`, run in the repository,
  installs them.
- To build this site: Node.js and npm.

## Build and run

```sh
cargo build --release --locked          # target/release/sigilfall
cargo run --locked -p sigilfall -- --help
```

## Tests and checks

These are the checks CI runs on Linux, Windows and macOS:

```sh
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo clippy --workspace --locked -- -D warnings   # the shipped code alone
cargo fmt --all --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked
```

CI also checks that the workspace builds on Rust 1.88, and checks the
licenses and security advisories of every dependency against `deny.toml`.

Every item of the code is documented: missing documentation is a warning,
and CI turns warnings into errors.

## A separate data folder

`SIGILFALL_DATA_DIR` points the game at another folder for its save and
options, so a test session never touches your own run:

```sh
SIGILFALL_DATA_DIR=./scratch-data cargo run --locked -p sigilfall
```

The folder is created if missing; an empty value is ignored. In PowerShell:

```powershell
$env:SIGILFALL_DATA_DIR = ".\scratch-data"; cargo run --locked -p sigilfall
```

## Changing the content

Monsters, sigils, items, spawn tables and floor kinds are RON files in
`crates/sigilfall-core/content`, with the meaning of each field written at
the head of the file. Every entry is named in English and in French. A test
checks that the shipped files are valid, so `cargo test --workspace --locked`
reports a faulty entry with its file, entry, field and line.

## Releases

Releases are built by [`dist`](https://github.com/axodotdev/cargo-dist) from
`dist-workspace.toml`: pushing a tag such as `v0.1.0` builds the binaries and
installers and publishes them as a GitHub release.

## This documentation site

The site lives in `site/` and is built with Starlight. English pages are in
`site/src/content/docs/`, French pages in `site/src/content/docs/fr/` with the
same file names, and the terms used in French in `site/glossary.md`.

```sh
cd site
npm ci
npm run dev     # local preview
npm run build   # site/dist, with every internal link checked
```

The [sigilfall-core reference](/sigilfall/reference/core/) is generated from
the `//!` comments of the crate by `npm run docs:api`, which `dev` and
`build` run first: to change it, change the Rust comment. The full rustdoc is
built and copied under `api/` by the docs workflow
(`.github/workflows/docs.yml`). To preview it locally:

```sh
cargo doc --no-deps -p sigilfall-core --locked
cp -r target/doc site/public/api
```
