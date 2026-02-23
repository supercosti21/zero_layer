# Architecture

This page explains how Zero Layer works internally — the data flow, key abstractions, directory layout, and design decisions.

---

## Table of Contents

- [High-Level Overview](#high-level-overview)
- [Startup Flow](#startup-flow)
- [Install Flow (Step by Step)](#install-flow-step-by-step)
- [Directory Layout](#directory-layout)
- [Key Abstractions](#key-abstractions)
- [Database](#database)
- [Plugin System](#plugin-system)
- [Design Decisions](#design-decisions)

---

## High-Level Overview

```
┌─────────────┐     ┌──────────────┐     ┌───────────────┐
│   CLI Args   │────>│  Command     │────>│  Plugin       │
│  (clap)      │     │  Dispatch    │     │  Registry     │
└─────────────┘     └──────────────┘     └───────┬───────┘
                                                  │
                    ┌─────────────────────────────┤
                    │         │         │         │
                ┌───┴───┐ ┌──┴──┐ ┌───┴──┐ ┌───┴───┐
                │pacman │ │ apt │ │github│ │ ...   │   ← SourcePlugin trait
                └───┬───┘ └──┬──┘ └───┬──┘ └───┬───┘
                    │        │        │        │
                    └────────┴────────┴────────┘
                                  │
                    ┌─────────────┴──────────────┐
                    │       Core Pipeline         │
                    │  Download → Verify →        │
                    │  Extract → Patch ELF →      │
                    │  Remap → Install →          │
                    │  Track in DB                │
                    └─────────────────────────────┘
```

ZL is a single binary that:
1. Parses CLI arguments (clap)
2. Detects the host system (SystemProfile)
3. Loads configuration (config.toml)
4. Registers all plugins (SourcePlugin trait objects)
5. Dispatches to the appropriate command handler
6. Each handler uses the plugin registry and core pipeline as needed

---

## Startup Flow

When you run any `zl` command, the following happens in `main.rs`:

```
main()
  └─> Parse CLI args (clap)
  └─> Init tracing (default: warn, -v: info, -vv: debug)
  └─> run()
        ├─> Early-exit commands (completions, self-update) → done
        ├─> Load ZlConfig from ~/.config/zl/config.toml
        ├─> Detect SystemProfile (arch, interpreter, libc, libs, layout)
        ├─> Apply config overrides to SystemProfile
        ├─> Create ZlPaths (directory structure)
        ├─> Ensure all directories exist
        ├─> Open ZlDatabase (redb)
        ├─> Register all 13 plugins
        ├─> Apply source filtering (config.general.sources)
        ├─> Init each enabled plugin (pass PluginConfig)
        ├─> Build AppContext (paths, db, registry, profile, flags)
        └─> Dispatch to command handler
```

### First-run detection

If `~/.config/zl/config.toml` doesn't exist, ZL runs the first-run wizard before plugin setup. The wizard detects the distro and lets the user choose sources interactively.

### Plugin registration order

Plugins are registered in this order (which also determines priority when multiple sources have the same package):

1. pacman
2. aur
3. apt
4. github
5. dnf
6. zypper
7. apk
8. xbps
9. portage
10. nix
11. flatpak
12. snap
13. appimage

After registration, `registry.retain_sources()` removes any plugins not in the user's source filter.

---

## Install Flow (Step by Step)

The install command is the most complex operation. Here's exactly what happens:

### 1. Source Resolution

```
User runs: zl install firefox --from pacman

  If --from specified:
    → Query only that plugin
  If --from omitted:
    → Query ALL plugins in parallel (thread::scope)
    → Collect results
    → If 1 result: auto-select
    → If multiple: show interactive menu (dialoguer::Select)
    → User picks source
```

### 2. Dependency Resolution

```
resolve_dependencies(candidate, plugin, registry, db, profile)

  For each dependency of the package:
    → Skip if already installed in ZL DB
    → Skip if system-provided (lib exists in system lib dirs)
    → Try to resolve in primary source
    → If not found: query ALL other sources (cross-source fallback)
    → If found in multiple: show interactive picker
    → Recurse into dependency's dependencies
    → Detect cycles (visited set)
    → Collect unresolvable deps (warn, don't fail)

  Return: InstallPlan (ordered deps-first)
```

### 3. Conflict Detection

```
check_conflicts(plan, db)

  For each package in the plan:
    → File ownership: would any file overwrite another package's file?
    → Binary name: would two packages provide the same executable?
    → Library soname: would two packages provide the same .so?
    → Declared conflicts: does metadata declare conflicts?
    → Version constraints: are version requirements satisfied?

  If conflicts found: show them, prompt user
```

### 4. Download (Parallel)

```
download_all(plan, plugins)

  Spawn up to 4 threads (thread::scope)
  Each thread:
    → plugin.download(candidate, cache_dir)
    → Retry on failure (3 attempts, exponential backoff: 1s, 2s, 4s)
    → Show progress bar (indicatif)
```

### 5. Verification

```
verify_all(downloaded_files)

  For each file:
    → SHA256 checksum (if available in metadata)
    → Download .sig file from same URL + ".sig"
    → GPG verify with system gpg binary (best-effort)
    → Fail on checksum mismatch
    → Warn on missing/failed GPG (don't fail)
```

### 6. Extract & Analyze

```
extract_all(archives, plugins)

  For each archive:
    → plugin.extract(archive_path) → ExtractedPackage
    → Walk extracted files
    → Classify: ELF binaries, scripts, data files
    → For ELF files: read with goblin
      → Get e_machine (architecture check)
      → Get PT_INTERP (current interpreter)
      → Get DT_NEEDED (shared library dependencies)
      → Get DT_RPATH/DT_RUNPATH
      → Get DT_SONAME (if library)
```

### 7. ELF Patching (Parallel)

```
patch_all(elf_files, profile)

  For packages with >1 ELF: parallel patching (4-way chunking)
  For each ELF binary:
    → Set interpreter to system's dynamic linker (from SystemProfile)
    → Set RUNPATH to: $ORIGIN/../lib:~/.local/share/zl/lib
    → Uses `elb` crate (pure Rust patchelf alternative)
    → Uses RUNPATH (not RPATH) — modern standard
```

### 8. Path Remapping

```
remap_paths(extracted_files, profile)

  For scripts and config files:
    → Replace hardcoded /usr/lib → ~/.local/share/zl/lib
    → Replace hardcoded /usr/bin → ~/.local/share/zl/bin
    → Replace hardcoded /etc → ~/.local/share/zl/etc
    → Uses PathMapping based on SystemProfile
```

### 9. Atomic Install

```
install(extracted, paths, db)

  Transaction::new()
  For each file:
    → Copy to packages/<name>-<version>/
    → Create symlinks in bin/, lib/, share/
    → Track every file/dir/symlink in Transaction
    → Register in database (PACKAGES, FILE_OWNERS, LIB_INDEX, DEPENDENCIES)

  If any step fails:
    → Transaction::rollback()
    → Remove ALL files, symlinks, dirs created
    → Remove ALL database entries
    → System returns to pre-install state

  On success:
    → Transaction::commit()
    → Record in HISTORY table
```

### 10. Post-Install Checks

```
post_install_verify(package, db, profile)

  For each ELF binary:
    → Check every DT_NEEDED library
    → Look in: ZL lib dir, ZL packages, system lib dirs
    → Warn about any unresolvable libraries
```

---

## Directory Layout

```
~/.local/share/zl/                    ← ZL root (ZlPaths.root)
├── bin/                              ← Symlinks to executables
│   ├── firefox -> ../packages/firefox-120.0/usr/bin/firefox
│   ├── bat -> ../packages/bat-0.24.0/usr/bin/bat
│   └── ...
├── lib/                              ← Symlinks to shared libraries
│   ├── libfoo.so.1 -> ../packages/libfoo-1.2/usr/lib/libfoo.so.1
│   └── ...
├── share/                            ← Shared data files
├── etc/                              ← Configuration files
├── packages/                         ← Per-package directories
│   ├── firefox-120.0/
│   │   └── usr/
│   │       ├── bin/firefox           ← Patched ELF binary
│   │       ├── lib/                  ← Package's libraries
│   │       └── share/                ← Package's data
│   ├── bat-0.24.0/
│   │   └── ...
│   └── ...
├── cache/                            ← Download cache
│   ├── pacman/                       ← Per-plugin cache dirs
│   ├── apt/
│   └── ...
├── envs/                             ← Ephemeral environments
│   ├── tmp-1234567/                  ← Temporary (deleted on exit)
│   └── myproject/                    ← Named (persists)
└── zl.redb                           ← Package database
```

### XDG integration

ZL also symlinks desktop integration files:

```
~/.local/share/
├── applications/                     ← .desktop files (apps in launcher)
│   └── firefox.desktop -> zl/packages/firefox-120.0/usr/share/applications/firefox.desktop
└── icons/                            ← Icons (shown in desktop)
    └── firefox.png -> zl/packages/firefox-120.0/usr/share/icons/...
```

---

## Key Abstractions

### SystemProfile

Represents the host system. Detected once at startup, threaded through all modules.

```
SystemProfile {
    arch: Arch,                    // x86_64, aarch64, armv7, etc.
    interpreter: PathBuf,          // /lib64/ld-linux-x86-64.so.2
    libc: LibC,                    // Glibc or Musl
    lib_dirs: Vec<PathBuf>,        // [/usr/lib, /usr/lib64, /lib, ...]
    layout: SystemLayout,          // Fhs, MergedUsr, NixOS, etc.
}
```

**Detection methods:**
- **Arch**: from `std::env::consts::ARCH`
- **Interpreter**: reads `PT_INTERP` from `/bin/sh` using goblin
- **LibC**: inferred from interpreter name (`musl` in path → Musl, else Glibc)
- **Lib dirs**: from `ldconfig -p`, `ld.so.conf` parsing, `LD_LIBRARY_PATH`, and layout-specific defaults
- **Layout**: detected from filesystem markers (`/nix/store`, `/gnu`, `/data/data/com.termux`, etc.)

### SourcePlugin trait

The interface every package source implements:

```rust
trait SourcePlugin: Send + Sync {
    fn name(&self) -> &str;
    fn display_name(&self) -> &str;
    fn init(&mut self, config: &PluginConfig) -> ZlResult<()>;
    fn search(&self, query: &str) -> ZlResult<Vec<PackageCandidate>>;
    fn resolve(&self, name: &str, version: Option<&str>) -> ZlResult<Option<PackageCandidate>>;
    fn download(&self, candidate: &PackageCandidate, dest_dir: &Path) -> ZlResult<PathBuf>;
    fn extract(&self, archive_path: &Path) -> ZlResult<ExtractedPackage>;
    fn sync(&self) -> ZlResult<()>;
}
```

### PackageCandidate

Metadata for a package found by a plugin:

```
PackageCandidate {
    name: String,
    version: String,
    description: String,
    arch: String,
    source: String,              // e.g., "pacman/extra", "github", "apt/main"
    dependencies: Vec<String>,
    provides: Vec<String>,
    conflicts: Vec<String>,
    installed_size: u64,
    download_url: String,
    checksum: Option<String>,    // SHA256
}
```

### Transaction

Wraps an install operation for atomicity:

```
Transaction {
    files_created: Vec<PathBuf>,
    dirs_created: Vec<PathBuf>,
    symlinks_created: Vec<PathBuf>,
    db_entries: Vec<String>,
}

// On success: commit() — does nothing (files already in place)
// On failure: rollback() — removes everything tracked
```

### DepGraph

petgraph-based dependency graph:

```
DepGraph {
    graph: DiGraph<PackageNode, DependencyEdge>,
    node_map: HashMap<String, NodeIndex>,
}
```

Supports:
- Topological sort (install order)
- Cycle detection
- Orphan detection (nodes with no incoming edges that aren't explicit)
- Reverse dependency lookup

### ZlDatabase

redb-based persistent storage with these tables:

| Table | Key | Value | Purpose |
|-------|-----|-------|---------|
| PACKAGES | name-version | PackageRecord | Installed package metadata |
| FILE_OWNERS | file path | package name | Which package owns each file |
| LIB_INDEX | soname | package name | Which package provides each library |
| DEPENDENCIES | package | dep list | Dependency relationships |
| PINNED | package name | bool | Pinned packages |
| PLUGIN_METADATA | key | value | Plugin-specific persistent data |
| HISTORY | timestamp | HistoryEntry | Install/remove/upgrade history |

---

## Plugin System

### Registration

Each plugin is instantiated and registered in `main.rs`:

```rust
// Simplified from actual code
let mut registry = PluginRegistry::new();

registry.register(Box::new(PacmanPlugin::new()));
registry.register(Box::new(AurPlugin::new()));
registry.register(Box::new(AptPlugin::new()));
// ... all 13 plugins

// Apply source filter
if let Some(sources) = config.enabled_sources() {
    registry.retain_sources(sources);
}

// Initialize each plugin
for plugin in registry.plugins_mut() {
    let plugin_config = config.plugin_config(plugin.name());
    plugin.init(&plugin_config)?;
}
```

### Plugin lifecycle

1. **Construct**: `PluginType::new()` — creates with defaults
2. **Register**: added to `PluginRegistry`
3. **Filter**: `retain_sources()` removes unwanted plugins
4. **Init**: `plugin.init(config)` — applies user configuration, creates cache dirs
5. **Sync**: `plugin.sync()` — downloads/updates database (called by `zl update`)
6. **Query**: `plugin.search()` / `plugin.resolve()` — find packages
7. **Download**: `plugin.download()` — fetch package archive
8. **Extract**: `plugin.extract()` — unpack archive to temp dir

---

## Design Decisions

### Single binary, no C dependencies

ZL uses pure-Rust alternatives for everything:
- **redb** instead of SQLite (no C compilation needed)
- **elb** instead of patchelf (no external binary needed)
- **goblin** instead of libelf (pure Rust ELF parsing)
- No tokio — uses `std::thread::scope` for parallelism

This means ZL compiles anywhere Rust compiles and has zero runtime dependencies.

### Dynamic detection over hardcoded paths

Nothing is hardcoded. ZL detects:
- The interpreter from a real ELF binary's `PT_INTERP`
- Library directories from `ldconfig` and `ld.so.conf`
- Filesystem layout from directory markers

This makes ZL work on any Linux system without configuration.

### RUNPATH over RPATH

ZL sets `RUNPATH` (not `RPATH`) on patched binaries. The difference:
- `RPATH` is searched before `LD_LIBRARY_PATH` (overrides user environment)
- `RUNPATH` is searched after `LD_LIBRARY_PATH` (respects user environment)

`RUNPATH` is the modern standard and gives users more control.

### Atomic transactions

Every install operation is wrapped in a `Transaction`. If anything fails:
- All files are removed
- All symlinks are cleaned up
- All database entries are rolled back
- The system is left in its pre-install state

This prevents broken partial installs.

### Parallel operations

ZL uses `std::thread::scope` (not async/tokio) for parallelism:
- **Downloads**: 4 concurrent threads
- **ELF patching**: packages with multiple ELFs are patched in parallel (4-way chunking)
- **Search queries**: all plugins are queried simultaneously

### Cross-source dependency resolution

When a dependency isn't found in the primary source, ZL queries all other enabled sources. This is critical for cross-distro installs — an Arch package might depend on a library that's available from APT or GitHub.

---

## Crate Dependencies

| Crate | Purpose |
|-------|---------|
| **goblin** | Read ELF metadata (interpreter, needed libs, rpath, soname, machine type) |
| **elb** | Patch ELF binaries (set interpreter, set runpath) |
| **petgraph** | Dependency graph with topological sort, cycle detection |
| **redb** | Embedded key-value database (pure Rust, ACID) |
| **clap** (derive) | CLI argument parsing |
| **reqwest** (blocking+json) | HTTP client for all API calls |
| **tar** + **zstd** + **flate2** + **xz2** + **bzip2** | Archive decompression |
| **ar** | Debian `.deb` archive extraction |
| **zip** | ZIP archive extraction |
| **quick-xml** | RPM repodata XML parsing |
| **cpio** | RPM CPIO payload extraction |
| **sha2** | SHA256 checksums |
| **indicatif** | Progress bars |
| **dialoguer** | Interactive prompts and menus |
| **console** | Colored terminal output |
| **serde** + **toml** | Configuration file parsing |
| **walkdir** | Recursive directory traversal |
| **tempfile** | Temporary directories for extraction |
| **tracing** | Structured logging |

---

## Next Steps

- [[ELF Binary Translation]] — Deep dive into how ELF patching works
- [[Dependency Resolution]] — How cross-source dependency resolution works
- [[Plugin Development]] — How to create a new source plugin
