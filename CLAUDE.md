# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Rules

- **Every time you make changes to the codebase**, update this CLAUDE.md file to reflect the new state (architecture, known issues, etc.).
- **If changes affect user-facing features, architecture, or usage**, also update README.md accordingly.

## Project Overview

**Zero Layer (ZL)** is a universal Linux package manager with native binary translation, written in Rust. It installs packages from any source (pacman, apt, AUR, GitHub releases) on any Linux system by translating them natively — no containers, VMs, or isolation layers. All translation happens at install time; installed packages run with zero overhead.

**Binary name**: `zl`
**Rust edition**: 2024 (requires Rust 1.85+)
**License**: GPL v3

## Build Commands

```bash
cargo build                  # Debug build
cargo build --release        # Release build
cargo run -- <subcommand>    # Run (e.g., cargo run -- install firefox)
cargo test                   # Run all tests (154 tests: 74 bin + 80 lib)
cargo test <name>            # Run a single test by name
cargo test -- --nocapture    # Run tests with stdout visible
cargo clippy                 # Lint
cargo fmt                    # Format
cargo fmt -- --check         # Check formatting without modifying
```

There are no integration tests — all tests are unit tests inside `#[cfg(test)]` modules within source files. There is no CI configuration; run `cargo test && cargo clippy && cargo fmt -- --check` locally before committing.

## Architecture

### Install flow (the core operation)

1. **Detect system** — `SystemProfile::detect()` auto-detects arch, dynamic linker, libc, lib dirs, filesystem layout
2. **Resolve source** — if `--from` omitted, queries ALL plugins in parallel; user picks if multiple hits (`pick_source()` in `cli/install.rs`)
3. **Resolve deps** — recursive dependency resolution with cycle detection (`cli/deps.rs`)
4. **Check conflicts** — 5 types: file ownership, binary name, library soname, declared conflicts, version constraints (`core/conflicts.rs`)
5. **Download** in parallel (4 threads via `thread::scope`) with progress bars and retry
6. **Verify** — SHA256 checksum + GPG signature (best-effort)
7. **Extract** and analyze ELF binaries with `goblin`
8. **Patch** ELF binaries with `elb` (set interpreter, RUNPATH) using detected page size
9. **Remap** FHS paths to ZL-managed directories (`core/path/`)
10. **Install** atomically — `Transaction` tracks all changes; rollback on any failure
11. **Track** in redb database + dependency graph

### Startup flow (`main.rs`)

`main()` → parse CLI → init tracing → `run()`:
1. Early-exit commands (`completions`, `self-update`) run without setup
2. Load config from `~/.config/zl/config.toml`
3. Detect `SystemProfile` and apply config overrides
4. Create `ZlPaths`, ensure directory structure exists
5. Open `ZlDatabase` (redb)
6. Register all plugins: pacman → aur → apt → github (order = priority)
7. Dispatch to command handler

### Key abstractions

- **`SystemProfile`** (`system/mod.rs`): Host profile (arch, interpreter, libc, lib dirs, layout). Built once, threaded through all modules. Replaces all hardcoded FHS assumptions.
- **`SourcePlugin` trait** (`plugin/mod.rs`): Interface every package source implements — `name()`, `search()`, `resolve()`, `download()`, `extract()`, `sync()`. Plugins are compile-time modules with trait objects, not dynamic libraries.
- **`Transaction`** (`core/transaction.rs`): Atomic install — tracks files/dirs/symlinks/DB entries created during install, rolls back everything on failure.
- **`DepGraph`** (`core/graph/model.rs`): petgraph-based dependency graph with topological sort, cycle detection, orphan detection.
- **`ZlDatabase`** (`core/db/ops.rs`): redb-based persistent store. Tables: PACKAGES, FILE_OWNERS, LIB_INDEX, DEPENDENCIES, PINNED, PLUGIN_METADATA.
- **`PathMapping`** (`core/path/mod.rs`): Dynamic FHS-to-ZL path translation using SystemProfile.
- **`PackageCandidate` / `ExtractedPackage`** (`plugin/mod.rs`): Common types shared across all plugins for package metadata and extracted content.

### Plugin system

All plugins implement `SourcePlugin` and are registered in `main.rs`. To add a new plugin:
1. Create `src/plugin/<name>/mod.rs` implementing `SourcePlugin`
2. Add `pub mod <name>;` in `src/plugin/mod.rs`
3. Instantiate and register in `main.rs`'s `run()` function

Current plugins: `pacman` (Arch repos), `aur` (AUR RPC v5 + makepkg), `apt` (Packages.gz + .deb), `github` (Releases API).

### Command dispatch pattern

Each CLI command lives in `src/cli/<command>.rs` with a `pub fn handle(...)` function. The `handle` function receives the parsed args struct plus shared state (`ZlPaths`, `ZlDatabase`, `PluginRegistry`, `SystemProfile`, flags). Commands are dispatched via a `match` in `main.rs`.

### Error handling

- `ZlError` enum in `error.rs` (thiserror) for domain errors with `.suggestion()` hints
- `anyhow::Result` at the top level (`run()` returns `anyhow::Result<()>`)
- `retry_with_backoff()` in `error.rs` for HTTP retries (3 attempts: 1s, 2s, 4s)
- Tracing: default level `warn`; `-v` = info, `-vv` = debug

### Key design constraints

- **Single binary, zero C deps**: redb over SQLite, elb over patchelf, no tokio (thread::scope for parallelism)
- **Dynamic detection over hardcoded paths**: interpreter from /bin/sh's PT_INTERP, lib dirs from ldconfig + ld.so.conf
- **RUNPATH over RPATH**: modern standard, respects LD_LIBRARY_PATH
- **Atomic transactions**: every install is wrapped; failure = full rollback

### ZL directory layout (runtime)

```
~/.local/share/zl/
  bin/          # Symlinks to executables (user adds to PATH)
  lib/          # Shared libraries
  share/        # Shared data files
  etc/          # Config files
  packages/     # Per-package directories (name-version/)
  cache/        # Download cache
  envs/         # Ephemeral/named environment roots
  zl.redb       # Package database
```

### Key crates

| Crate | Purpose |
|-------|---------|
| `goblin` | Read ELF metadata (interpreter, needed libs, rpath, soname) |
| `elb` | Patch ELF binaries (set interpreter, set runpath) |
| `petgraph` | Dependency graph with topological sort, cycle detection |
| `redb` | Embedded key-value database (pure Rust, ACID) |
| `clap` (derive) | CLI argument parsing |
| `reqwest` (blocking+json) | HTTP client |
| `tar` + `zstd` + `flate2` + `xz2` + `bzip2` + `ar` + `zip` | Archive formats |
| `sha2` | SHA256 checksums |
| `indicatif` + `dialoguer` | Progress bars and interactive prompts |

### Known compiler warnings

There are existing dead-code warnings for `core/build/` (build system support is implemented but not yet wired into any plugin) and a few unused fields/functions in `config.rs` and `cli/completions.rs`. These are expected — the build system will be used by future plugins.
