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
2. Download package from selected source
3. Analyze all ELF binaries with `goblin`
4. Patch binaries with `elb` (interpreter, RUNPATH) using detected page size
5. Remap FHS paths to target system structure using dynamic prefix map
6. Patch scripts and config files replacing paths
7. Build/update dependency graph tracking every file
8. Verify everything resolves correctly post-install (using detected lib dirs)

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
    mod.rs             # Cli struct (clap derive), Commands enum
    deps.rs            # Dependency resolution: recursive resolve, install plan, cycle detection
    install.rs         # Install: dep resolution, parallel download, sequential install
    remove.rs          # Remove: delete files, symlinks, deps, DB entries, cascade orphans
    search.rs          # Search subcommand handler
    update.rs          # Update: check newer versions, remove old + install new
    list.rs            # List subcommand handler
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
    graph/
      model.rs         # PackageId, PackageNode, DependencyEdge, DepGraph (petgraph)
      resolver.rs      # Topological sort, cycle detection, orphan detection
      verifier.rs      # Post-install verification — uses profile.lib_dirs for lib search
    db/
      schema.rs        # redb table definitions (PACKAGES, FILE_OWNERS, LIB_INDEX, DEPENDENCIES)
      ops.rs           # CRUD: packages, file ownership, lib index, dependencies, plugin metadata
  plugin/
    mod.rs             # SourcePlugin trait, PackageCandidate, ExtractedPackage, PluginRegistry
    pacman/
      mod.rs           # PacmanPlugin: SourcePlugin + provides-based resolution
      mirror.rs        # Mirror list parsing and URL construction
      database.rs      # Sync DB download/parsing with retry (pacman desc format)
      package.rs       # .pkg.tar.zst download with retry, extraction, .PKGINFO parsing
```

### Key abstractions
- **SystemProfile** (`system/mod.rs`): auto-detected host profile (arch, interpreter, libc, lib dirs, layout). Built once at startup, passed to all modules. Replaces all hardcoded FHS assumptions.
- **SourcePlugin trait** (`plugin/mod.rs`): interface every package source implements (search, resolve, download, extract, sync)
- **InstallPlan** (`cli/deps.rs`): result of recursive dependency resolution — ordered list of packages to install
- **PathMapping** (`core/path/mod.rs`): maps FHS paths to ZL-managed paths for a specific package, using SystemProfile
- **BuildSpec/BuildSystem** (`core/build/mod.rs`): source build specification with auto-detection of build systems
- **DepGraph** (`core/graph/model.rs`): petgraph-based dependency graph tracking all packages and their relationships
- **ZlDatabase** (`core/db/ops.rs`): redb-based persistent storage for installed packages, file ownership, library index, dependency tracking

### Key crates
| Crate | Purpose |
|-------|---------|
| `goblin` | Read ELF metadata (interpreter, needed libs, rpath, soname) |
| `elb` | Patch ELF binaries (set interpreter, set runpath) — pure Rust patchelf |
| `petgraph` | Dependency graph with topological sort, cycle detection |
| `redb` | Embedded key-value database (pure Rust, ACID, no C deps) |
| `libc` | System detection (sysconf for page size) |
| `clap` (derive) | CLI argument parsing |
| `reqwest` (blocking) | HTTP downloads |
| `tar` + `zstd` + `flate2` | Archive extraction |

### ZL directory layout (runtime)
```
~/.local/share/zl/
  bin/          # Symlinks to executables (user adds to PATH)
  lib/          # Shared libraries (never duplicated)
  share/        # Shared data files
  etc/          # Config files
  packages/     # Per-package directories (name-version/)
  cache/        # Download cache
  zl.redb       # Package database
```

### Design decisions
- **Single crate, no workspace**: plugins are compile-time modules with trait objects, not dynamic libraries
- **Dynamic system detection over hardcoded paths**: interpreter detected from /bin/sh's PT_INTERP, lib dirs from ldconfig + ld.so.conf, layout auto-classified
- **RUNPATH over RPATH**: modern standard, respects LD_LIBRARY_PATH
- **redb over SQLite**: pure Rust, maintains single-binary zero-deps constraint
- **elb over shelling out to patchelf**: pure Rust, no external dependency
- **Parallel downloads with thread::scope**: up to 4 concurrent downloads, no tokio needed
- **Retry with exponential backoff**: all HTTP operations retry up to 3 times (1s, 2s, 4s)

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
- [x] `cli/mod.rs` — Cli struct, Commands enum, all arg structs (clap derive)
- [x] `main.rs` — Full bootstrap: config, SystemProfile detection, paths, DB, plugin registry, CLI dispatch
- [x] `lib.rs` — Re-exports core modules including system
- [x] `system/` — Full SystemProfile detection (arch, interpreter, libc, lib dirs, layout)
- [x] `core/elf/` — ELF analysis (goblin) and patching (elb) with dynamic page size
- [x] `core/path/` — PathMapping with dynamic prefix map, script/shebang remapping
- [x] `core/graph/` — DepGraph, topological sort, cycle detection, orphan detection, verification
- [x] `core/db/` — redb tables, CRUD for packages/files/libs/deps/plugin metadata
- [x] `plugin/pacman/` — Full PacmanPlugin: sync, search, resolve, download, extract

### Phase 2: Dep resolution + Parallel downloads + Source builds + Error handling (complete)
- [x] `error.rs` — Expanded ZlError: 15+ variants with user-friendly suggestions, retry_with_backoff()
- [x] `cli/deps.rs` — Recursive dependency resolution with cycle detection, install plan display
- [x] `cli/install.rs` — Full dep resolution, parallel downloads (4 threads), sequential install
- [x] `cli/remove.rs` — Improved orphan detection using dep table + lib needs
- [x] `cli/update.rs` — Uses install_single_package(), only updates explicit packages
- [x] `core/build/mod.rs` — BuildSpec, BuildSystem enum, detect_build_system(), build_package()
- [x] `core/build/systems.rs` — Autotools, CMake, Meson, Cargo, Make, custom script builders
- [x] `core/db/ops.rs` — Dependency tracking: register, get, reverse lookup, remove
- [x] `plugin/pacman/mod.rs` — Provides-based virtual package resolution (fallback in resolve())
- [x] `plugin/pacman/package.rs` — Download with retry (3 attempts, exponential backoff), checksum cache
- [x] `plugin/pacman/database.rs` — DB sync with retry (3 attempts, exponential backoff)

### All tests pass: 34 tests
The project compiles and all 34 tests pass.

### Removed
- `core/path/fhs.rs` — replaced by `system/` module. No more hardcoded FHS constants.

### Future work (Phase 3+)
- [ ] Additional plugins: APT, RPM, AppImage, GitHub Releases, pip, npm, cargo
- [ ] Cross-OS support (macOS/Homebrew via HostAdapter trait)
- [ ] Progress bars for downloads (indicatif is already in deps)
- [ ] Interactive conflict resolution (dialoguer is already in deps)
