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
1. Download package from selected source
2. Analyze all ELF binaries with `goblin`
3. Patch binaries with `elb` (interpreter, RUNPATH)
4. Remap FHS paths to target system structure
5. Patch scripts and config files replacing paths
6. Build/update dependency graph tracking every file
7. Verify everything resolves correctly post-install

### Module structure
```
src/
  main.rs              # Entry point: CLI parsing + dispatch
  lib.rs               # Re-exports core public API
  error.rs             # ZlError enum (thiserror), ZlResult type alias
  config.rs            # Config parsing (~/.config/zl/config.toml)
  paths.rs             # ZL directory layout (~/.local/share/zl/)
  cli/
    mod.rs             # Cli struct (clap derive), Commands enum
    install.rs         # Install subcommand handler (full flow)
    remove.rs          # Remove subcommand handler
    search.rs          # Search subcommand handler
    update.rs          # Update subcommand handler
    list.rs            # List subcommand handler
  core/
    elf/
      analysis.rs      # Read ELF metadata with goblin (interpreter, needed libs, rpath, soname)
      patcher.rs       # Patch ELF with elb (set interpreter, set runpath)
    path/
      mod.rs           # PathMapping struct — FHS-to-ZL path translation
      fhs.rs           # FHS path constants, system interpreter detection
      remapper.rs      # Rewrite paths in text files and shebangs
    graph/
      model.rs         # PackageId, PackageNode, DependencyEdge, DepGraph (petgraph)
      resolver.rs      # Topological sort, cycle detection, orphan detection
      verifier.rs      # Post-install verification (can all ELFs find their deps?)
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
- **SourcePlugin trait** (`plugin/mod.rs`): interface every package source implements (search, resolve, download, extract, sync)
- **PathMapping** (`core/path/mod.rs`): maps FHS paths to ZL-managed paths for a specific package
- **DepGraph** (`core/graph/model.rs`): petgraph-based dependency graph tracking all packages and their relationships
- **ZlDatabase** (`core/db/ops.rs`): redb-based persistent storage for installed packages, file ownership, library index

### Key crates
| Crate | Purpose |
|-------|---------|
| `goblin` | Read ELF metadata (interpreter, needed libs, rpath, soname) |
| `elb` | Patch ELF binaries (set interpreter, set runpath) — pure Rust patchelf |
| `petgraph` | Dependency graph with topological sort, cycle detection |
| `redb` | Embedded key-value database (pure Rust, ACID, no C deps) |
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
- **RUNPATH over RPATH**: modern standard, respects LD_LIBRARY_PATH
- **redb over SQLite**: pure Rust, maintains single-binary zero-deps constraint
- **elb over shelling out to patchelf**: pure Rust, no external dependency
- **Sync HTTP for Phase 1**: simpler; async for parallel downloads in future phases

## Implementation Status (Phase 1: Core + Pacman plugin)

### Fully implemented
- [x] Project skeleton: Cargo.toml with all dependencies, full directory structure
- [x] `error.rs` — ZlError enum with all variants, ZlResult alias
- [x] `paths.rs` — ZlPaths struct with ensure_dirs()
- [x] `config.rs` — ZlConfig, GeneralConfig, PluginConfig deserialization
- [x] `cli/mod.rs` — Cli struct, Commands enum, all arg structs (clap derive)
- [x] `cli/install.rs` — Full install flow: sync, resolve, download, extract, patch, remap, symlink, DB
- [x] `cli/remove.rs` — Remove: delete files, symlinks, DB entries, cascade orphans
- [x] `cli/search.rs` — Search across plugins, sync + display results
- [x] `cli/update.rs` — Update: check newer versions, remove old + install new
- [x] `cli/list.rs` — List installed packages from DB
- [x] `main.rs` — Full bootstrap: config, paths, DB, plugin registry, CLI dispatch
- [x] `lib.rs` — Re-exports core modules
- [x] `core/elf/analysis.rs` — Full ELF analysis with goblin (analyze, scan_directory, is_elf_file)
- [x] `core/elf/patcher.rs` — ELF patching with elb (patch_for_zl, set_interpreter, set_runpath)
- [x] `core/path/mod.rs` — PathMapping struct with remap logic
- [x] `core/path/fhs.rs` — FHS constants and interpreter detection
- [x] `core/path/remapper.rs` — Text file and shebang remapping
- [x] `plugin/mod.rs` — SourcePlugin trait, PackageCandidate, ExtractedPackage, PluginRegistry
- [x] `core/graph/model.rs` — PackageId, PackageNode, DependencyEdge, DepGraph structs
- [x] `core/db/schema.rs` — redb table definitions (PACKAGES, FILE_OWNERS, LIB_INDEX, DEPENDENCIES, PLUGIN_META)

- [x] `core/graph/resolver.rs` — topological sort, cycle detection, orphan detection, install ordering
- [x] `core/graph/verifier.rs` — post-install ELF verification (missing libs, interpreter checks)
- [x] `core/db/ops.rs` — ZlDatabase CRUD: packages, file ownership, lib index, plugin metadata
- [x] `plugin/pacman/mod.rs` — PacmanPlugin implementing full SourcePlugin trait
- [x] `plugin/pacman/mirror.rs` — mirrorlist parsing, default mirrors, URL construction
- [x] `plugin/pacman/database.rs` — sync DB download, tar.gz parsing, desc file parsing
- [x] `plugin/pacman/package.rs` — .pkg.tar.zst download/extract, .PKGINFO parsing, SHA256 verification

### Phase 1 complete
All core modules and the Pacman plugin are fully implemented and wired together.
The project compiles and all 12 tests pass.

### Future work (Phase 2+)
- [ ] Additional plugins: APT, RPM, AppImage, GitHub Releases, pip, npm, cargo
- [ ] Async HTTP for parallel downloads
- [ ] Dependency auto-resolution (auto-install missing deps from same source)
- [ ] Multi-architecture support (aarch64, etc.)
