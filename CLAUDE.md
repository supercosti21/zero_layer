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
  error.rs             # ZlError enum (thiserror), ZlResult type alias
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
    install.rs         # Install subcommand handler (full flow, uses SystemProfile)
    remove.rs          # Remove subcommand handler
    search.rs          # Search subcommand handler
    update.rs          # Update subcommand handler (uses SystemProfile)
    list.rs            # List subcommand handler
  core/
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
      ops.rs           # CRUD operations on the database
  plugin/
    mod.rs             # SourcePlugin trait, PackageCandidate, ExtractedPackage, PluginRegistry
    pacman/
      mod.rs           # PacmanPlugin implementing SourcePlugin
      mirror.rs        # Mirror list parsing and URL construction
      database.rs      # Sync DB download/parsing (pacman desc format)
      package.rs       # .pkg.tar.zst download, extraction, .PKGINFO parsing
```

### Key abstractions
- **SystemProfile** (`system/mod.rs`): auto-detected host profile (arch, interpreter, libc, lib dirs, layout). Built once at startup, passed to all modules. Replaces all hardcoded FHS assumptions.
- **SourcePlugin trait** (`plugin/mod.rs`): interface every package source implements (search, resolve, download, extract, sync)
- **PathMapping** (`core/path/mod.rs`): maps FHS paths to ZL-managed paths for a specific package, using SystemProfile
- **DepGraph** (`core/graph/model.rs`): petgraph-based dependency graph tracking all packages and their relationships
- **ZlDatabase** (`core/db/ops.rs`): redb-based persistent storage for installed packages, file ownership, library index

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
- **Sync HTTP for Phase 1**: simpler; async for parallel downloads in future phases

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

## Implementation Status (Phase 1: Core + Pacman plugin + Universal distro support)

### Fully implemented
- [x] Project skeleton: Cargo.toml with all dependencies, full directory structure
- [x] `error.rs` — ZlError enum with all variants, ZlResult alias
- [x] `paths.rs` — ZlPaths struct with ensure_dirs()
- [x] `config.rs` — ZlConfig, GeneralConfig, SystemConfig, PluginConfig deserialization
- [x] `cli/mod.rs` — Cli struct, Commands enum, all arg structs (clap derive)
- [x] `cli/install.rs` — Full install flow: sync, resolve, download, extract, patch, remap, symlink, DB (uses SystemProfile)
- [x] `cli/remove.rs` — Remove: delete files, symlinks, DB entries, cascade orphans
- [x] `cli/search.rs` — Search across plugins, sync + display results
- [x] `cli/update.rs` — Update: check newer versions, remove old + install new (uses SystemProfile)
- [x] `cli/list.rs` — List installed packages from DB
- [x] `main.rs` — Full bootstrap: config, SystemProfile detection, paths, DB, plugin registry, CLI dispatch
- [x] `lib.rs` — Re-exports core modules including system
- [x] `system/mod.rs` — SystemProfile struct, detect(), apply_overrides(), system_lib_exists()
- [x] `system/arch.rs` — Arch enum, detect(), from_str(), is_64bit(), pacman_name()
- [x] `system/interpreter.rs` — detect_interpreter() via PT_INTERP of /bin/sh, fallback scan
- [x] `system/libc.rs` — LibC enum, detect_libc() from interpreter path, version detection
- [x] `system/paths.rs` — discover_lib_dirs(), discover_bin_dirs(), detect_multiarch_tuple(), fhs_source_prefixes()
- [x] `system/detect.rs` — SystemLayout enum, detect_layout(), detect_page_size() via sysconf
- [x] `core/elf/analysis.rs` — Full ELF analysis with goblin (analyze, scan_directory, is_elf_file)
- [x] `core/elf/patcher.rs` — ELF patching with elb, uses profile.page_size (not hardcoded)
- [x] `core/path/mod.rs` — PathMapping with dynamic prefix_map from SystemProfile (includes multiarch)
- [x] `core/path/remapper.rs` — Text file and shebang remapping
- [x] `plugin/mod.rs` — SourcePlugin trait, PackageCandidate, ExtractedPackage, PluginRegistry
- [x] `core/graph/model.rs` — PackageId, PackageNode, DependencyEdge, DepGraph structs
- [x] `core/db/schema.rs` — redb table definitions (PACKAGES, FILE_OWNERS, LIB_INDEX, DEPENDENCIES, PLUGIN_META)
- [x] `core/graph/resolver.rs` — topological sort, cycle detection, orphan detection, install ordering
- [x] `core/graph/verifier.rs` — post-install ELF verification using profile.lib_dirs (not hardcoded FHS_LIB_DIRS)
- [x] `core/db/ops.rs` — ZlDatabase CRUD: packages, file ownership, lib index, plugin metadata
- [x] `plugin/pacman/mod.rs` — PacmanPlugin implementing full SourcePlugin trait
- [x] `plugin/pacman/mirror.rs` — mirrorlist parsing, default mirrors, URL construction
- [x] `plugin/pacman/database.rs` — sync DB download, tar.gz parsing, desc file parsing
- [x] `plugin/pacman/package.rs` — .pkg.tar.zst download/extract, .PKGINFO parsing, SHA256 verification

### Phase 1 complete
All core modules, the Pacman plugin, and the SystemProfile detection system are fully implemented and wired together.
The project compiles and all 27 tests pass (including 15 new system detection tests).

### Removed
- `core/path/fhs.rs` — replaced by `system/` module. No more hardcoded FHS constants.

### Future work (Phase 2+)
- [ ] Additional plugins: APT, RPM, AppImage, GitHub Releases, pip, npm, cargo
- [ ] Async HTTP for parallel downloads
- [ ] Dependency auto-resolution (auto-install missing deps from same source)
- [ ] Cross-OS support (macOS/Homebrew via HostAdapter trait)
- [ ] Source build fallback (compile from source when binary is incompatible)
