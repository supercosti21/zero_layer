# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Rules

- **Every time you make changes to the codebase**, update this CLAUDE.md file to reflect the new state (implementation status, module structure, known issues, etc.).
- **If changes affect user-facing features, architecture, or usage**, also update README.md accordingly.

## Project Overview

**Zero Layer (ZL)** is a universal Linux package manager with native binary translation, written in Rust. It installs packages from any source (pacman, apt, rpm, AppImage, GitHub releases, pip, npm, cargo, etc.) on any Linux system by translating them natively — no containers, VMs, or isolation layers. After installation, translated packages are indistinguishable from native ones. Zero runtime overhead: all translation happens at install time.

**Binary name**: `zl`
**License**: GPL v3

## Build Commands

```bash
cargo build              # Build the project
cargo run -- <subcommand> # Run (e.g., cargo run -- install firefox)
cargo test               # Run all tests
cargo test <name>        # Run a single test by name
cargo clippy             # Lint with clippy
cargo fmt                # Format code
cargo fmt -- --check     # Check formatting without modifying files
```

## Architecture

### How it works (install flow)
1. **Detect system** — auto-detect arch, interpreter, libc, lib dirs, layout (SystemProfile)
2. **Resolve** — recursive dependency resolution with cycle detection
3. **Check conflicts** — file ownership, binary/library name, version constraints, declared conflicts
4. **Download** packages in parallel (up to 4 threads) with progress bars and retry
5. **Verify** — SHA256 checksum + GPG signature verification (best-effort)
6. **Extract** and analyze all ELF binaries with `goblin`
7. **Patch** binaries with `elb` (interpreter, RUNPATH) using detected page size
8. **Remap** FHS paths to target system structure using dynamic prefix map
9. **Install** with atomic transaction — rollback on any failure
10. **Track** every file in a dependency graph and persistent database
11. **Verify** everything resolves correctly post-install (using detected lib dirs)

### Module structure
```
src/
  main.rs              # Entry point: CLI parsing + SystemProfile detection + dispatch
  lib.rs               # Re-exports core public API
  error.rs             # ZlError enum (thiserror), ZlResult type alias, retry_with_backoff()
  config.rs            # Config parsing (~/.config/zl/config.toml), SystemConfig for overrides
  paths.rs             # ZL directory layout (~/.local/share/zl/)
  system/
    mod.rs             # SystemProfile struct — auto-detected host system profile
    arch.rs            # CPU architecture detection (x86_64, aarch64, armv7, riscv64, etc.)
    interpreter.rs     # Dynamic linker detection (reads PT_INTERP from /bin/sh via goblin)
    libc.rs            # C library detection (glibc/musl/bionic from interpreter name)
    paths.rs           # Host lib/bin path discovery (ldconfig, ld.so.conf, multiarch, NixOS)
    detect.rs          # System layout detection (FHS, MergedUsr, NixOS, Guix, Termux, GoboLinux)
  cli/
    mod.rs             # Cli struct (clap derive), Commands enum, all arg structs
    deps.rs            # Dependency resolution: recursive resolve, install plan, cycle detection
    install.rs         # Install: conflict check, parallel download, verification, transaction-wrapped install, multi-version support
    remove.rs          # Remove: delete files, symlinks, deps, DB entries, cascade orphans, version-specific removal
    search.rs          # Search subcommand handler
    update.rs          # Update: check newer versions, skip pinned, remove old + install new
    upgrade.rs         # Upgrade: mass upgrade all packages with summary + confirmation
    list.rs            # List: --explicit, --deps, --orphans filters
    info.rs            # Detailed package info: deps, reverse deps, disk usage, pin status
    cache.rs           # Cache management: list files + sizes, clean all cached files
    completions.rs     # Shell completions generation (bash/zsh/fish) via clap_complete
    pin.rs             # Pin/unpin packages to prevent updates
    lockfile.rs        # Export/import installed packages as JSON lockfile
    selfupdate.rs      # Self-update: download + replace ZL binary from GitHub releases
    env.rs             # Ephemeral environments: temporary/named isolated shells
  core/
    build/
      mod.rs           # BuildSpec, BuildSystem enum, detect_build_system(), build_package()
      systems.rs       # Build system implementations: autotools, cmake, meson, cargo, make
    elf/
      analysis.rs      # Read ELF metadata with goblin (interpreter, needed libs, rpath, soname)
      patcher.rs       # Patch ELF with elb (set interpreter, set runpath) — uses profile.page_size
    path/
      mod.rs           # PathMapping struct — dynamic FHS-to-ZL path translation via SystemProfile
      remapper.rs      # Rewrite paths in text files and shebangs
    transaction.rs     # Atomic install transaction: tracks files/dirs/symlinks/DB, rollback on failure
    conflicts.rs       # Conflict detection: file ownership, binary/lib name, version constraints, declared conflicts
    verify.rs          # Package verification: SHA256 checksum + GPG signature (best-effort)
    graph/
      model.rs         # PackageId, PackageNode, DependencyEdge, DepGraph (petgraph)
      resolver.rs      # Topological sort, cycle detection, orphan detection
      verifier.rs      # Post-install verification — uses profile.lib_dirs for lib search
    db/
      schema.rs        # redb table definitions (PACKAGES, FILE_OWNERS, LIB_INDEX, DEPENDENCIES, PINNED)
      ops.rs           # CRUD: packages, file ownership, lib index, dependencies, pinning, multi-version queries, plugin metadata
  plugin/
    mod.rs             # SourcePlugin trait, PackageCandidate, ExtractedPackage, PluginRegistry
    pacman/
      mod.rs           # PacmanPlugin: SourcePlugin + provides-based resolution
      mirror.rs        # Mirror list parsing and URL construction
      database.rs      # Sync DB download/parsing with retry (pacman desc format)
      package.rs       # .pkg.tar.zst download with retry, extraction, .PKGINFO parsing
    aur/
      mod.rs           # AurPlugin: live AUR RPC v5 queries, git clone + makepkg build
    apt/
      mod.rs           # AptPlugin: Packages.gz sync, per-distro mirror/suite config
      index.rs         # APT Packages index parser (RFC 2822-like format)
      deb.rs           # .deb extraction (ar → data.tar.{gz,xz,zst,bz2}) + download
    github/
      mod.rs           # GithubPlugin: GitHub Releases API, smart asset selection, extract
```

### Key abstractions
- **SystemProfile** (`system/mod.rs`): auto-detected host profile (arch, interpreter, libc, lib dirs, layout). Built once at startup, passed to all modules. Replaces all hardcoded FHS assumptions.
- **SourcePlugin trait** (`plugin/mod.rs`): interface every package source implements (search, resolve, download, extract, sync)
- **InstallPlan** (`cli/deps.rs`): result of recursive dependency resolution — ordered list of packages to install
- **PathMapping** (`core/path/mod.rs`): maps FHS paths to ZL-managed paths for a specific package, using SystemProfile
- **Transaction** (`core/transaction.rs`): atomic install transaction tracking filesystem + DB changes with rollback support
- **ConflictReport** (`core/conflicts.rs`): pre-install conflict detection (file ownership, binary/library names, version constraints, declared conflicts)
- **VerifyResult** (`core/verify.rs`): package integrity verification — SHA256 checksum + GPG signature
- **BuildSpec/BuildSystem** (`core/build/mod.rs`): source build specification with auto-detection of build systems
- **DepGraph** (`core/graph/model.rs`): petgraph-based dependency graph tracking all packages and their relationships
- **ZlDatabase** (`core/db/ops.rs`): redb-based persistent storage for installed packages, file ownership, library index, dependency tracking, package pinning, multi-version queries

### Key crates
| Crate | Purpose |
|-------|---------|
| `goblin` | Read ELF metadata (interpreter, needed libs, rpath, soname) |
| `elb` | Patch ELF binaries (set interpreter, set runpath) — pure Rust patchelf |
| `petgraph` | Dependency graph with topological sort, cycle detection |
| `redb` | Embedded key-value database (pure Rust, ACID, no C deps) |
| `libc` | System detection (sysconf for page size) |
| `clap` (derive) | CLI argument parsing |
| `clap_complete` | Shell completions generation (bash/zsh/fish) |
| `indicatif` | Progress bars for downloads and installs |
| `reqwest` (blocking+json) | HTTP downloads + JSON deserialization |
| `tar` + `zstd` + `flate2` | Archive extraction |
| `xz2` | XZ decompression (.tar.xz, Packages.xz) |
| `ar` | Ar archive reading (.deb format) |
| `bzip2` | BZip2 decompression (data.tar.bz2 in old .deb) |
| `zip` | Zip extraction (GitHub release assets) |
| `sha2` | SHA256 checksums for package verification |

### ZL directory layout (runtime)
```
~/.local/share/zl/
  bin/          # Symlinks to executables (user adds to PATH)
  lib/          # Shared libraries (never duplicated)
  share/        # Shared data files
  etc/          # Config files
  packages/     # Per-package directories (name-version/)
  cache/        # Download cache
  envs/         # Ephemeral/named environment roots
  zl.redb       # Package database (PACKAGES, FILE_OWNERS, LIB_INDEX, DEPENDENCIES, PINNED tables)
```

### Design decisions
- **Single crate, no workspace**: plugins are compile-time modules with trait objects, not dynamic libraries
- **Dynamic system detection over hardcoded paths**: interpreter detected from /bin/sh's PT_INTERP, lib dirs from ldconfig + ld.so.conf, layout auto-classified
- **RUNPATH over RPATH**: modern standard, respects LD_LIBRARY_PATH
- **redb over SQLite**: pure Rust, maintains single-binary zero-deps constraint
- **elb over shelling out to patchelf**: pure Rust, no external dependency
- **Parallel downloads with thread::scope**: up to 4 concurrent downloads, no tokio needed
- **Retry with exponential backoff**: all HTTP operations retry up to 3 times (1s, 2s, 4s)
- **Atomic transactions**: installs are wrapped in Transaction; on failure, all filesystem + DB changes are rolled back
- **Pre-install conflict detection**: 5 types of conflicts checked before any files are written
- **Side-by-side versions**: multiple versions of the same package can coexist, with `zl switch` to change the active one
- **Ephemeral environments**: temporary isolated shells where packages disappear on exit

### SystemProfile detection chain
1. **Architecture**: `std::env::consts::ARCH` (compile-time, always correct for running binary)
2. **Page size**: `libc::sysconf(_SC_PAGESIZE)` (never hardcoded — 4K on x86_64, up to 64K on aarch64)
3. **Dynamic linker**: Read PT_INTERP from `/bin/sh` using goblin. Works on ANY distro.
4. **C library**: Derived from interpreter filename (`ld-linux-*` → glibc, `ld-musl-*` → musl)
5. **Library paths**: Combined from `ldconfig -p`, `/etc/ld.so.conf`, `LD_LIBRARY_PATH`, layout-specific dirs
6. **Layout**: Detected by filesystem markers (NixOS: `/nix/store`, Guix: `/gnu/store`, Termux: `$PREFIX`, merged: `/bin` → `/usr/bin`)

### Supported distros/layouts
- Standard FHS (Fedora, RHEL, SUSE, Void, etc.)
- Merged /usr (Arch, Ubuntu 22+, Fedora 33+, Debian 12+)
- Debian multiarch (`/usr/lib/x86_64-linux-gnu/`)
- Alpine/Void musl
- NixOS (`/nix/store`)
- GNU Guix (`/gnu/store`)
- Termux on Android
- GoboLinux
- Any architecture: x86_64, aarch64, armv7, riscv64, i686, s390x, ppc64le

## Implementation Status

### Phase 1: Core + Pacman plugin + Universal distro support (complete)
- [x] Project skeleton: Cargo.toml with all dependencies, full directory structure
- [x] `error.rs` — ZlError enum with all variants, ZlResult alias, retry_with_backoff()
- [x] `paths.rs` — ZlPaths struct with ensure_dirs()
- [x] `config.rs` — ZlConfig, GeneralConfig, SystemConfig, PluginConfig deserialization
- [x] `main.rs` — Full bootstrap: config, SystemProfile detection, paths, DB, plugin registry, CLI dispatch
- [x] `lib.rs` — Re-exports core modules including system
- [x] `system/` — Full SystemProfile detection (arch, interpreter, libc, lib dirs, layout)
- [x] `core/elf/` — ELF analysis (goblin) and patching (elb) with dynamic page size
- [x] `core/path/` — PathMapping with dynamic prefix map, script/shebang remapping
- [x] `core/graph/` — DepGraph, topological sort, cycle detection, orphan detection, verification
- [x] `core/db/` — redb tables, CRUD for packages/files/libs/deps/plugin metadata
- [x] `plugin/pacman/` — Full PacmanPlugin: sync, search, resolve, download, extract

### Phase 2: Dep resolution + Parallel downloads + Source builds + Error handling (complete)
- [x] `cli/deps.rs` — Recursive dependency resolution with cycle detection, install plan display
- [x] `cli/install.rs` — Full dep resolution, parallel downloads (4 threads), sequential install
- [x] `cli/remove.rs` — Improved orphan detection using dep table + lib needs
- [x] `cli/update.rs` — Uses install_single_package(), only updates explicit packages
- [x] `core/build/` — BuildSpec, BuildSystem enum, detect + build for autotools/cmake/meson/cargo/make
- [x] `core/db/ops.rs` — Dependency tracking: register, get, reverse lookup, remove
- [x] `plugin/pacman/mod.rs` — Provides-based virtual package resolution
- [x] `plugin/pacman/package.rs` — Download with retry, checksum cache verification
- [x] `plugin/pacman/database.rs` — DB sync with retry

### Phase 3: Safety, UX, and package management features (complete)
- [x] `core/transaction.rs` — Atomic install transactions: track files/dirs/symlinks/DB entries, rollback on failure
- [x] `core/conflicts.rs` — Pre-install conflict detection: file ownership, binary name, library soname, declared conflicts, version constraints (with component-by-component comparison)
- [x] `core/db/schema.rs` — Added PINNED table for package pinning
- [x] `core/db/ops.rs` — Added pin_package(), unpin_package(), is_pinned(), list_pinned()
- [x] `cli/install.rs` — Integrated: conflict checking, transaction-wrapped installs, progress bars (indicatif)
- [x] `cli/update.rs` — Respects pinned packages (skips them during updates)
- [x] `cli/info.rs` — Detailed package info: name, version, source, status, deps, reverse deps, disk usage
- [x] `cli/cache.rs` — Cache management: `zl cache list` (files + sizes), `zl cache clean` (free space)
- [x] `cli/completions.rs` — Shell completions: `zl completions bash/zsh/fish` via clap_complete
- [x] `cli/list.rs` — Enhanced: `--explicit`, `--deps`, `--orphans` filters, pin status display
- [x] `cli/pin.rs` — Pin/unpin packages: `zl pin <pkg>`, `zl unpin <pkg>`
- [x] `cli/lockfile.rs` — Lockfile: `zl export [file]`, `zl import <file>` (JSON format)
- [x] `cli/mod.rs` — All new commands wired up: Info, Cache, Completions, Pin, Unpin, Export, Import

### Phase 4: Security, multi-version, environments, and mass upgrade (complete)
- [x] `core/verify.rs` — Package verification: SHA256 checksum validation + GPG signature verification (best-effort, uses system gpg when available)
- [x] `cli/install.rs` — Integrated verification pipeline: all downloads verified before install, `--skip-verify` flag to bypass
- [x] `cli/install.rs` — Dry-run mode (`--dry-run` / `--simulate`): shows install plan without making changes
- [x] `cli/remove.rs` — Dry-run support, version-specific removal (`--version`), multi-version aware
- [x] `cli/update.rs` — Dry-run support, integrated verification
- [x] `cli/upgrade.rs` — Mass upgrade: `zl upgrade` checks all packages, shows summary, confirms, upgrades in batch. `--check` for preview-only, `--from` to filter by source
- [x] `cli/install.rs` — Multi-version support: install multiple versions side-by-side (e.g., `zl install firefox --version 120.0` and `--version 121.0`)
- [x] `cli/install.rs` — `zl switch <pkg> <version>`: change which version's binaries are active (symlinked to bin/)
- [x] `core/db/ops.rs` — Added `get_all_versions(name)`: query all installed versions of a package
- [x] `cli/selfupdate.rs` — Self-update: `zl self-update` downloads latest release from GitHub, verifies architecture, atomically replaces binary
- [x] `cli/env.rs` — Ephemeral environments: `zl env shell [name]` spawns an isolated shell with its own ZL root. Temporary envs are auto-deleted on exit. Named envs persist.
- [x] `cli/env.rs` — Environment management: `zl env list`, `zl env delete <name>`
- [x] `cli/mod.rs` — Global flags: `--dry-run`/`--simulate`, `--skip-verify`
- [x] `error.rs` — New error variants: GpgVerification, SelfUpdate, Environment

### Phase 5c: Error handling + XDG integration (complete)
- [x] `core/db/ops.rs` — DB init: propagate `open_table` errors instead of silently swallowing
- [x] `cli/install.rs` — Script remap failures now logged with `tracing::warn!` instead of `let _ =`
- [x] `cli/install.rs` — Thread join panic handled without `unwrap()`; poisoned Mutex recovered with `into_inner()`
- [x] `cli/install.rs` — `install_xdg_assets()`: symlinks `.desktop` files to `$XDG_DATA_HOME/applications/`, icons to `$XDG_DATA_HOME/icons/` (full tree). Uses `dirs::data_local_dir()` — works on all distros
- [x] `cli/install.rs` — `patch_desktop_exec()`: rewrites `Exec=` in .desktop files to strip absolute path prefix so binary is found via PATH
- [x] `cli/selfupdate.rs` — Distinct error message for 404 (no releases) vs network errors
- [x] `error.rs` — Better hint for `SelfUpdate` permission denied: suggests `sudo zl self-update`
- [x] `cli/mod.rs` — `verbose` changed from `bool` to `u8` (`ArgAction::Count`): `-v` = info, `-vv` = debug
- [x] `main.rs` — Default log level changed from `info` to `warn` (clean output by default)
- [x] `cli/selfupdate.rs` — Fixed GitHub repo URL: `supercosti21/zero_layer` (was `zero-layer/zl`)

### Phase 5b: Interactive multi-source selection (complete)
- [x] `cli/install.rs` — `pick_source()`: when `--from` is omitted, resolves from ALL plugins in parallel, then:
  - 0 results → `PackageNotFound` error
  - 1 result → auto-selects (no prompt)
  - N results + `--yes` → picks first (highest-priority plugin, i.e. pacman)
  - N results → shows `dialoguer::Select` for interactive choice
- [x] `cli/install.rs` — `handle()` now determines `from: String` before syncing, so `plugin.sync()` and `deps::resolve_with_deps()` always use a known source name

### Phase 5: New plugins — AUR, APT, GitHub Releases (complete)
- [x] `plugin/aur/mod.rs` — AurPlugin: live AUR RPC API v5 (search/resolve), git clone + makepkg build. `zl install yay --from aur`
- [x] `plugin/apt/index.rs` — APT Packages index parser: RFC 2822-like format, dep list parsing, short-description extraction
- [x] `plugin/apt/deb.rs` — .deb extraction: ar → data.tar.{gz,xz,zst,bz2}, SHA256 cache validation, retry download
- [x] `plugin/apt/mod.rs` — AptPlugin: Packages.gz sync per component, in-memory DB, configurable mirror/suite/arch. `zl install vim --from apt`
- [x] `plugin/github/mod.rs` — GithubPlugin: GitHub Releases API, smart asset scoring (arch, musl, format), extract tar.gz/tar.xz/zip/AppImage/bare binary. `zl install sharkdp/bat --from github`
- [x] `Cargo.toml` — Added: `ar`, `bzip2`, `zip`, `xz2`, reqwest `json` feature
- [x] `plugin/mod.rs` — Added `pub mod aur; pub mod apt; pub mod github;`
- [x] `main.rs` — All three plugins registered at startup (AurPlugin, AptPlugin, GithubPlugin)
- [x] All tests pass: **79 tests** (was 72 → +7 new: 3 apt::index, 1 aur, 3 github)

### All tests pass: 79 tests

### Removed
- `core/path/fhs.rs` — replaced by `system/` module. No more hardcoded FHS constants.

### Future work (Phase 6+)
- [ ] Additional plugins: RPM, AppImage, pip, npm, cargo
- [ ] Cross-OS support (macOS/Homebrew via HostAdapter trait)
- [ ] Interactive conflict resolution (dialoguer is already in deps)
- [ ] Async HTTP for even faster parallel downloads
- [ ] Hook system: pre/post install scripts
- [ ] TUI interactive mode for search and selection
