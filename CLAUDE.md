# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

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
- [x] `main.rs` — CLI dispatch (stub handlers, tracing init)
- [x] `lib.rs` — Re-exports core modules
- [x] `core/elf/analysis.rs` — Full ELF analysis with goblin (analyze, scan_directory, is_elf_file)
- [x] `core/elf/patcher.rs` — ELF patching with elb (patch_for_zl, set_interpreter, set_runpath)
- [x] `core/path/mod.rs` — PathMapping struct with remap logic
- [x] `core/path/fhs.rs` — FHS constants and interpreter detection
- [x] `core/path/remapper.rs` — Text file and shebang remapping
- [x] `plugin/mod.rs` — SourcePlugin trait, PackageCandidate, ExtractedPackage, PluginRegistry
- [x] `core/graph/model.rs` — PackageId, PackageNode, DependencyEdge, DepGraph structs
- [x] `core/db/schema.rs` — redb table definitions (PACKAGES, FILE_OWNERS, LIB_INDEX, DEPENDENCIES, PLUGIN_META)

### Stub files (exist but contain only TODO comments)
- [ ] `core/graph/resolver.rs` — needs topological sort, cycle detection, orphan detection
- [ ] `core/graph/verifier.rs` — needs post-install verification logic
- [ ] `core/db/ops.rs` — needs ZlDatabase struct with CRUD operations
- [ ] `plugin/pacman/mod.rs` — needs PacmanPlugin implementing SourcePlugin
- [ ] `plugin/pacman/mirror.rs` — needs mirror list parsing and URL construction
- [ ] `plugin/pacman/database.rs` — needs sync DB download and parsing
- [ ] `plugin/pacman/package.rs` — needs .pkg.tar.zst download, extraction, .PKGINFO parsing

### Known issues
- `core/elf/patcher.rs` line 120: `set_runpath()` passes `&c_runpath` but needs `&*c_runpath` (dereference to `CStr`) — causes compilation error
- CLI subcommand handlers in `main.rs` are stubs (print "not yet implemented")
