# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Rules

These rules are **mandatory** for every Claude instance working on this repo.

### Git workflow

1. **Feature branches** — NEVER work directly on `main`. Always create a branch first.
2. **Branch naming** — Use prefixed names: `feat/xxx`, `fix/xxx`, `chore/xxx`, `refactor/xxx`, `docs/xxx`.
3. **Merge** — Only merge to `main` when everything works (tests pass, clippy clean, fmt clean). Delete the branch after merge.
6. **Atomic commits** — 1 commit = 1 concept. Better 3 small focused commits than 1 giant commit. Each commit should be self-contained and pass CI on its own.
7. **Commit message format** — `type: clear title` where type is `feat`, `fix`, `chore`, `refactor`, or `docs`. Add bullet points in the body for details when needed.
8. **Documentation** — After every significant change, update `CLAUDE.md` first (implementation state, module structure, test count), then `README.md` if user-facing features changed.
9. **Wiki** — After every significant change, also update the relevant pages in `docs/wiki/` to keep the wiki in sync with the codebase. Wiki content is published to `github.com/supercosti21/zero_layer.wiki.git` (separate repo). The `docs/wiki/` folder is kept locally but gitignored from main.
10. **GitHub metadata** — When changes affect the project scope, features, or tech stack, update the GitHub repository description and topics to stay in sync. See the "GitHub Metadata" section below for current values and the `gh` commands to update them.
11. **Tests** — Every new feature or bug fix must include corresponding unit tests. Run `cargo test` before committing to ensure nothing is broken. Never commit code that fails tests.
12. **CI must pass** — Every commit must pass the full CI pipeline (`cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test`). Do not merge branches that fail CI.

## Project Overview

**Zero Layer (ZL)** is a universal Linux package manager with native binary translation, written in Rust. It installs packages from any source (pacman, apt, AUR, GitHub releases) on any Linux system by translating them natively — no containers, VMs, or isolation layers. All translation happens at install time; installed packages run with zero overhead.

**Binary name**: `zl`
**Rust edition**: 2024 (requires Rust 1.85+)
**License**: MIT

## Build Commands

```bash
cargo build                  # Debug build
cargo build --release        # Release build
cargo run -- <subcommand>    # Run (e.g., cargo run -- install firefox)
cargo test                   # Run all tests (303 tests: 136 bin + 167 lib)
cargo test <name>            # Run a single test by name
cargo test -- --nocapture    # Run tests with stdout visible
cargo clippy                 # Lint
cargo fmt                    # Format
cargo fmt -- --check         # Check formatting without modifying
```

There are no integration tests — all tests are unit tests inside `#[cfg(test)]` modules within source files.

**CI**: GitHub Actions workflow (`.github/workflows/ci.yml`) runs `cargo fmt --check`, `cargo clippy -- -D warnings`, and `cargo test` on every push to `main` and all PRs.

## Architecture

### Install flow (the core operation)

1. **Detect system** — `SystemProfile::detect()` auto-detects arch, dynamic linker, libc, lib dirs, filesystem layout
2. **Resolve source** — if `--from` omitted, queries ALL plugins in parallel (including AUR `-bin` variants); user picks from full list via interactive `dialoguer::Select` (`pick_source()` in `cli/install.rs`)
3. **Resolve deps** — recursive dependency resolution with cycle detection + **cross-source fallback** (queries all plugins if dep not found in primary source, lets user choose) (`cli/deps.rs`)
4. **Check conflicts** — 5 types: file ownership, binary name, library soname, declared conflicts, version constraints (`core/conflicts.rs`)
5. **Download** in parallel (4 threads via `thread::scope`) with progress bars and retry — **step [1/4] indicator**
6. **Verify** — SHA256 checksum + GPG signature (best-effort) — **step [2/4]**
7. **Extract**, **normalize the layout to FHS** if the source ships a non-FHS archive (see below), and analyze ELF binaries with `goblin`
8. **Arch check** — verify ELF `e_machine` matches host architecture before patching (warning on mismatch)
9. **Patch** ELF binaries with `elb` (set interpreter, RUNPATH) — **parallel patching** via `thread::scope` for packages with multiple ELFs — **step [3/4]**. Statically linked binaries (no PT_INTERP, no DT_NEEDED — this includes static-pie, the usual shape of musl release builds) are **skipped**: RUNPATH would be inert, and rewriting their dynamic section corrupts them.
10. **Remap** FHS paths to ZL-managed directories (`core/path/`)
11. **Install** atomically — `Transaction` tracks all changes; rollback on any failure
12. **Post-install checks** — warn about missing shared libraries not found in ZL DB or system lib dirs
13. **Track** in redb database + dependency graph
14. **Record history** — install event stored in HISTORY table for `zl history`/`zl rollback`
15. **Summary** — colored output with step [4/4] completion indicator

### New commands (v0.2)

- **`zl run <package>`** — download, extract, patch, execute a package without installing. Temp dir auto-cleaned on exit.
- **`zl history list`** — show install/remove/upgrade history with timestamps
- **`zl history rollback [N]`** — undo the last N operations (installs can be rolled back; removes show reinstall hint)
- **`zl why <package>`** — trace dependency chain explaining why a package is installed
- **`zl doctor`** — full system diagnostics: DB integrity, broken symlinks, missing libs, orphans, disk usage, system profile
- **`zl size [package]`** — disk usage per package with file breakdown, dep costs. `--sort` for largest first.
- **`zl diff <package>`** — show version/dep/size changes before updating
- **`zl audit [package]`** — check installed packages for known CVEs via OSV.dev API
- **`zl cache dedup`** — deduplicate identical shared libraries across packages using hardlinks

### Search flow (`zl search`)

1. **Parallel queries** — all plugins are queried via `thread::scope` simultaneously (not sequentially)
2. **Relevance scoring** — each result is scored: exact name match (100), starts-with (80), contains in name (60), description-only (30)
3. **AUR binary discovery** — if the query doesn't end with `-bin`, automatically also fetches `-bin`, `-appimage`, `-prebuilt` variants and tags them `[binary]`
4. **Sorted output** — results sorted by relevance (default), name, or version via `--sort`
5. **Filtering** — `--exact` shows only exact name matches; `--from` limits to a single source; `--limit` controls results per source
6. **Colored output** — exact matches highlighted green+bold, versions in yellow, source headers in cyan

### Removal flow (`zl remove --cascade`)

1. **Preview before action** — `--cascade` always shows what will and won't be removed before prompting
2. **Orphan detection** — only removes packages that are: (a) tracked in ZL's DB, (b) marked as implicit (`explicit: false`), (c) not depended on by any remaining package (checked via both dependency table and shared lib needs)
3. **Shared dep protection** — dependencies used by other packages are listed as "Keeping (needed by X)" and never removed
4. **Dry-run support** — `--dry-run` with `--cascade` shows the full removal plan without touching anything
5. **History recording** — removal events stored for `zl history`/`zl rollback`

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
- **`ZlDatabase`** (`core/db/ops.rs`): redb-based persistent store. Tables: PACKAGES, FILE_OWNERS, LIB_INDEX, DEPENDENCIES, PINNED, PLUGIN_METADATA, HISTORY. Since the redb 4 upgrade a `zl.redb` written by ZL <= 0.2.0 (redb file format v2) cannot be opened; `open()` maps `DatabaseError::UpgradeRequired` to a message telling the user to delete the file and reinstall.
- **`PathMapping`** (`core/path/mod.rs`): Dynamic FHS-to-ZL path translation using SystemProfile.
- **`PackageCandidate` / `ExtractedPackage`** (`plugin/mod.rs`): Common types shared across all plugins for package metadata and extracted content.
- **`PluginInfo`** (`plugin/mod.rs`): Plugin metadata for the remote plugin registry.
- **`HistoryEntry`** (`core/db/ops.rs`): Tracks install/remove/upgrade/rollback events with timestamps.

### Plugin system

All plugins implement `SourcePlugin` and are registered in `main.rs`. To add a new plugin:
1. Create `src/plugin/<name>/mod.rs` implementing `SourcePlugin`
2. Add `pub mod <name>;` in `src/plugin/mod.rs`
3. Instantiate and register in `main.rs`'s `run()` function

Current plugins (13 total):
- **`pacman`** — Arch Linux repositories (syncs .db files, pkg.tar.zst extraction)
- **`aur`** — AUR RPC v5 + makepkg (with `-bin` variant discovery)
- **`apt`** — Debian/Ubuntu (Packages.gz + .deb extraction)
- **`dnf`** — Fedora/RHEL/CentOS (RPM repodata XML + shared RPM extraction)
- **`zypper`** — openSUSE/SLES (RPM repodata, shares RPM module with dnf)
- **`apk`** — Alpine Linux (APKINDEX.tar.gz + .apk tar.gz)
- **`xbps`** — Void Linux (binary plist repodata + tar.zst)
- **`portage`** — Gentoo binhost (Packages index + .tbz2/.gpkg.tar)
- **`nix`** — Nix packages (search.nixos.org API + NAR archive extraction)
- **`flatpak`** — Flathub (Flathub API v2 + flatpak CLI)
- **`snap`** — Snapcraft Store (API v2 + squashfs)
- **`appimage`** — AppImageHub (feed.json + self-contained executables)
- **`github`** — GitHub Releases (API + smart asset selection)

Shared modules: `plugin/rpm/` (RPM repodata XML parsing + cpio extraction, used by dnf + zypper).

**Plugins must produce an FHS layout.** `create_bin_symlinks` links only what it finds in `core::path::FHS_BIN_DIRS` (`usr/bin`, `bin`, `sbin`, …), so a plugin that leaves executables anywhere else installs them without ever putting them on PATH — silently, since the install still reports success. Distro plugins get this for free; the `github` plugin normalizes explicitly in `normalize_archive_layout()` (unwrap a lone top-level directory, then move root-level ELF programs into `usr/bin`). Use the `FHS_BIN_DIRS` constant rather than a local copy.

Remote plugin registry: `fetch_remote_registry()` fetches `PluginInfo` from a URL for future plugin marketplace.

### Source filtering

Users control which plugins ZL loads via three mechanisms:

1. **Config file** (`~/.config/zl/config.toml`):
   ```toml
   [general]
   sources = ["pacman", "aur", "apt", "github"]  # whitelist; omit for all
   ```
2. **`zl sources` command**: `list`, `enable <names>`, `disable <names>`, `only <names>`, `reset`
3. **`--from` flag** (per-command): `--from pacman,apt` (comma-separated list)
4. **First-run wizard**: on first launch (no config.toml), auto-detects distro and lets user pick sources interactively

`PluginConfig` implements `Default` **by hand** with `enabled: true`. Do not replace it with `#[derive(Default)]`: `plugin_config()` falls back to `Default::default()` for every plugin without a `[plugins.<name>]` table, and the derived `false` silently prevented `main.rs` from registering any plugin at all.

### Command dispatch pattern

Each CLI command lives in `src/cli/<command>.rs` with a `pub fn handle(...)` function. Most `handle` functions receive the parsed args struct plus an `AppContext` reference (defined in `cli/mod.rs`), which bundles shared state: `ZlPaths`, `ZlDatabase`, `PluginRegistry`, `SystemProfile`, and flags (`auto_yes`, `dry_run`, `skip_verify`). Commands are dispatched via a `match` in `main.rs`.

**Full command list**: `install`, `remove`, `search`, `update`, `upgrade`, `list`, `info`, `cache` (list/clean/dedup), `completions`, `pin`, `unpin`, `export`, `import`, `switch`, `self-update`, `env` (shell/list/delete), `run`, `history` (list/rollback), `why`, `doctor`, `size`, `diff`, `audit`, `sources` (list/enable/disable/only/reset).

### Error handling

- `ZlError` enum in `error.rs` (thiserror, boxed where needed to keep size small) for domain errors with `.suggestion()` hints
- Plugin-specific error suggestions: pacman mirror issues, APT repo failures, GitHub rate limits, AUR build failures (base-devel, PGP keys), architecture mismatches, self-update permissions
- `anyhow::Result` at the top level (`run()` returns `anyhow::Result<()>`)
- `retry_with_backoff()` in `error.rs` for HTTP retries (3 attempts: 1s, 2s, 4s)
- Tracing: default level `warn`; `-v` = info, `-vv` = debug

### Key design constraints

- **Single binary, zero C deps**: redb over SQLite, elb over patchelf, no tokio (thread::scope for parallelism), rustls over OpenSSL (reqwest 0.13's default — `openssl-sys` is not in the dependency tree)
- **Dynamic detection over hardcoded paths**: interpreter from /bin/sh's PT_INTERP, lib dirs from ldconfig + ld.so.conf
- **RUNPATH over RPATH**: modern standard, respects LD_LIBRARY_PATH
- **Atomic transactions**: every install is wrapped; failure = full rollback
- **Colored output**: uses `console` crate throughout (search, list, doctor, diff, audit, size, install steps)
- **Parallel ELF patching**: packages with >1 ELF are patched concurrently via `thread::scope` with 4-way chunking
- **Cross-source dep resolution**: when a dependency is not found in the primary source, all other sources are queried and the user chooses

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
  zl.redb       # Package database (includes HISTORY table)
```

### Key crates

| Crate | Purpose |
|-------|---------|
| `goblin` | Read ELF metadata (interpreter, needed libs, rpath, soname, machine type) |
| `elb` | Patch ELF binaries (set interpreter, set runpath) |
| `petgraph` | Dependency graph with topological sort, cycle detection |
| `redb` | Embedded key-value database (pure Rust, ACID). v4 — reads only the v3+ file format |
| `clap` (derive) | CLI argument parsing (with `ValueEnum` for `SortOrder`) |
| `reqwest` (blocking+json) | HTTP client (rustls TLS backend by default) |
| `tar` + `zstd` + `flate2` + `xz2` + `bzip2` + `ar` + `zip` | Archive formats |
| `sha2` | SHA256 checksums — always via `core::verify::sha256_hex`, since sha2 0.11's digest output has no `LowerHex` impl |
| `indicatif` + `dialoguer` | Progress bars and interactive prompts |
| `console` | Colored terminal output |
| `quick-xml` | RPM repodata XML parsing (dnf, zypper plugins) |
| `cpio` | RPM payload extraction (cpio archives inside RPMs) |

### Code quality

- **Zero clippy warnings**: `cargo clippy -- -D warnings` passes clean
- **Zero `cargo fmt` diff**: all code is formatted
- **303 tests**: comprehensive coverage of core modules (conflicts, ELF, path mapping, DB, graph, transaction, verify, plugins, search scoring, system detection, cache dedup, run, doctor, size, history, why, RPM repodata, NAR, source filtering)

### Naming conventions

- `SystemLayout` variants use PascalCase: `Fhs`, `MergedUsr`, `NixOS`, `Guix`, `Termux`, `GoboLinux`, `Custom`
- `Conflict` variants avoid repeating the enum name: `Declared` (not `DeclaredConflict`), `Version` (not `VersionConflict`)
- `Arch::parse()` and `SystemLayout::parse()` instead of `from_str()` (avoids confusion with `std::str::FromStr` trait)
- `SortOrder` enum (in `cli/mod.rs`) uses `ValueEnum` derive for clap: `Relevance`, `Name`, `Version`
- `HistoryAction` enum: `Install`, `Remove`, `Upgrade`, `Rollback`
- Structs with simple `new()` constructors also implement `Default` (via `#[derive(Default)]` or manual impl): `PluginRegistry`, `PacmanPlugin`, `AptPlugin`, `AurPlugin`, `GithubPlugin`, `DepGraph`, `Transaction`
- The `core/build/` module uses `#![allow(dead_code)]` since it is scaffolding for future source-build support
- `DepGraph`, `DependencyEdge`, `DepType` have `#[allow(dead_code)]` — they are part of the graph model used for future features
- `ArchMismatch` error variant has `#[allow(dead_code)]` — available for strict arch enforcement in future
- `PluginInfo`, `fetch_remote_registry`, `list_info` have `#[allow(dead_code)]` — scaffolding for remote plugin marketplace

## GitHub Metadata

Keep the repository description and topics in sync with the project state. Update them whenever features, scope, or tech stack change significantly.

### Current description

```
Universal Linux package manager with native binary translation. Install packages from any source (pacman, apt, AUR, GitHub releases) on any Linux system — no containers, no VMs, zero runtime overhead. Written in Rust.
```

### Current topics

```
linux, package-manager, rust, elf, binary-translation, cli, apt, pacman, aur, dnf, nix, flatpak, cross-distribution, dependency-management
```

### Commands to update

```bash
# Set description
gh repo edit --description "Universal Linux package manager with native binary translation. Install packages from any source (pacman, apt, AUR, GitHub releases) on any Linux system — no containers, no VMs, zero runtime overhead. Written in Rust."

# Set topics (replaces all topics)
gh repo edit --add-topic linux --add-topic package-manager --add-topic rust --add-topic elf --add-topic binary-translation --add-topic cli --add-topic apt --add-topic pacman --add-topic aur --add-topic cross-distribution --add-topic dependency-management
```
