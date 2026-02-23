# Contributing

This guide covers the development workflow, code style, and testing practices for contributing to Zero Layer.

---

## Table of Contents

- [Getting Started](#getting-started)
- [Development Workflow](#development-workflow)
- [Code Style](#code-style)
- [Testing](#testing)
- [Project Structure](#project-structure)
- [Adding a New Command](#adding-a-new-command)
- [Adding a New Plugin](#adding-a-new-plugin)

---

## Getting Started

### Prerequisites

- **Rust 1.85+** (edition 2024)
- **Git**
- A Linux system (for testing)

### Setup

```bash
git clone https://github.com/supercosti21/zero_layer.git
cd zero_layer
cargo build
cargo test
```

### Build commands

```bash
cargo build                  # Debug build
cargo build --release        # Release build (optimized)
cargo run -- <subcommand>    # Run (e.g., cargo run -- search firefox)
cargo test                   # Run all 264 tests
cargo test <name>            # Run a single test by name
cargo test -- --nocapture    # Run tests with stdout visible
cargo clippy                 # Lint (must pass with zero warnings)
cargo fmt                    # Format code
cargo fmt -- --check         # Check formatting without modifying
```

---

## Development Workflow

### Branch rules

1. **Never work directly on `main`**. Always create a feature branch.
2. **Branch naming**: Use prefixed names:
   - `feat/add-brew-plugin` — new features
   - `fix/checksum-mismatch` — bug fixes
   - `chore/update-deps` — maintenance
   - `refactor/simplify-download` — code improvements
   - `docs/update-wiki` — documentation
3. **Merge to main** only when everything passes (tests, clippy, fmt).
4. **Delete the branch** after merge.

### Commit style

Atomic commits — 1 commit = 1 concept:

```
feat: add Homebrew plugin for macOS support

- Implement search via brew API
- Support .tar.gz and .bottle.tar.gz formats
- Add tests for URL generation and archive detection
```

```
fix: handle empty APKINDEX entries without crashing

- Skip entries with missing P: (package name) field
- Add test for malformed APKINDEX data
```

**Types**: `feat`, `fix`, `chore`, `refactor`, `docs`

### CI checks

GitHub Actions runs on every push to `main` and all PRs:

1. `cargo fmt -- --check` — formatting
2. `cargo clippy -- -D warnings` — linting (warnings are errors)
3. `cargo test` — all tests must pass

All three must pass before merging.

### After significant changes

1. Update `CLAUDE.md` — implementation state, module structure, test count
2. Update `README.md` — if user-facing features changed
3. Update the wiki — if command behavior or configuration changed

---

## Code Style

### General rules

- Zero clippy warnings — `cargo clippy -- -D warnings` must pass clean
- Zero `cargo fmt` diff — all code is formatted
- Use `ZlResult<T>` for all fallible operations (not `anyhow` in library code)
- Use `tracing::info!()` / `tracing::warn!()` / `tracing::debug!()` for logging
- Use `console::style()` for colored output

### Naming conventions

| Item | Convention | Example |
|------|-----------|---------|
| Enum variants | PascalCase, no prefix | `Conflict::Declared` (not `DeclaredConflict`) |
| System layouts | PascalCase | `SystemLayout::MergedUsr` |
| Parse methods | `parse()` | `Arch::parse()` (not `from_str()`) |
| Plugin structs | `{Name}Plugin` | `PacmanPlugin`, `AptPlugin` |
| Plugin constructors | `new()` + `Default` | `PacmanPlugin::new()` / `PacmanPlugin::default()` |
| CLI enums | `ValueEnum` derive | `SortOrder::Relevance` |

### Error handling

```rust
// Use ZlError variants
Err(ZlError::Plugin {
    plugin: "myplugin".into(),
    message: format!("failed to parse: {}", e),
})

// Use retry_with_backoff for network operations
crate::error::retry_with_backoff(3, 1000, |attempt| {
    // ...
})

// Use ? operator for propagation
let data = std::fs::read(&path)?;  // io::Error auto-converts to ZlError
```

### Avoid over-engineering

- Don't add features beyond what was requested
- Don't add abstractions for one-time operations
- Don't add error handling for impossible scenarios
- Three similar lines of code is better than a premature abstraction
- Don't add comments where the code is self-evident

---

## Testing

### Test structure

All tests are unit tests inside `#[cfg(test)]` modules within source files. There are no separate integration test files.

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_my_feature() {
        // Test code
    }
}
```

### Current test count: 264

Distributed across:
- Core modules: conflicts, ELF analysis, path mapping, DB operations, graph model, transaction, verify
- Plugins: pacman, aur, apt, github, dnf, zypper, apk, xbps, portage, nix, flatpak, snap, appimage
- CLI: search scoring, sources, system detection
- Features: cache dedup, run, doctor, size, history, why

### What to test

1. **Default construction** — verify defaults are sensible
2. **Parsing** — test with sample data (valid and invalid)
3. **URL generation** — test that URLs are formed correctly
4. **Edge cases** — empty input, missing fields, malformed data
5. **Error handling** — verify errors are returned (not panics)

### Running tests

```bash
cargo test                       # All tests
cargo test pacman                # Tests containing "pacman"
cargo test test_xbps_plugin      # Specific test
cargo test -- --nocapture        # See stdout
cargo test -- --test-threads=1   # Sequential (for debugging)
```

---

## Project Structure

```
src/
├── main.rs              # Entry point, startup, plugin registration
├── config.rs            # Configuration loading/saving
├── error.rs             # Error types, retry logic
├── system/
│   └── mod.rs           # SystemProfile detection
├── core/
│   ├── elf/
│   │   ├── analysis.rs  # ELF reading (goblin)
│   │   └── patch.rs     # ELF patching (elb)
│   ├── db/
│   │   └── ops.rs       # Database operations (redb)
│   ├── graph/
│   │   └── model.rs     # Dependency graph (petgraph)
│   ├── path/
│   │   └── mod.rs       # FHS path remapping
│   ├── build/           # Source build scaffolding
│   ├── conflicts.rs     # Conflict detection (5 types)
│   ├── transaction.rs   # Atomic install/rollback
│   └── verify.rs        # Checksum and GPG verification
├── plugin/
│   ├── mod.rs           # SourcePlugin trait, PluginRegistry
│   ├── pacman/          # Arch Linux repos
│   ├── aur/             # Arch User Repository
│   ├── apt/             # Debian/Ubuntu
│   ├── github/          # GitHub Releases
│   ├── dnf/             # Fedora/RHEL
│   ├── zypper/          # openSUSE
│   ├── apk_alpine/      # Alpine Linux
│   ├── xbps/            # Void Linux
│   ├── portage/         # Gentoo
│   ├── nix/             # Nix packages
│   ├── flatpak/         # Flathub
│   ├── snap/            # Snapcraft
│   ├── appimage/        # AppImageHub
│   └── rpm/             # Shared RPM extraction/parsing
└── cli/
    ├── mod.rs           # CLI definitions, AppContext
    ├── install.rs       # Install flow
    ├── remove.rs        # Remove flow
    ├── search.rs        # Search with scoring
    ├── update.rs        # Update
    ├── upgrade.rs       # Upgrade
    ├── list.rs          # List installed
    ├── info.rs          # Package info
    ├── run.rs           # Run without install
    ├── sources.rs       # Source management
    ├── deps.rs          # Dependency resolution
    ├── cache.rs         # Cache management
    ├── pin.rs           # Pin/unpin
    ├── switch.rs        # Version switching
    ├── env.rs           # Ephemeral environments
    ├── export.rs        # Lockfile export
    ├── import.rs        # Lockfile import
    ├── doctor.rs        # System diagnostics
    ├── why.rs           # Dependency chain tracing
    ├── size.rs          # Disk usage
    ├── diff.rs          # Version diff
    ├── audit.rs         # CVE auditing
    ├── history.rs       # History/rollback
    └── self_update.rs   # Self-update
```

---

## Adding a New Command

1. **Define the command** in `src/cli/mod.rs`:

```rust
// In the Commands enum
MyCommand(MyCommandArgs),

// The args struct
#[derive(clap::Args)]
pub struct MyCommandArgs {
    pub name: String,
    #[arg(long)]
    pub flag: bool,
}
```

2. **Create the handler** at `src/cli/mycommand.rs`:

```rust
use crate::cli::AppContext;
use crate::error::ZlResult;
use super::MyCommandArgs;

pub fn handle(args: MyCommandArgs, ctx: &AppContext) -> ZlResult<()> {
    // Implementation
    Ok(())
}
```

3. **Add module declaration** in `src/cli/mod.rs`:

```rust
pub mod mycommand;
```

4. **Add dispatch** in `src/main.rs`:

```rust
Commands::MyCommand(args) => cli::mycommand::handle(args, &ctx),
```

---

## Adding a New Plugin

See [[Plugin Development]] for a complete step-by-step guide.

---

## Key Design Constraints

Keep these in mind when contributing:

1. **Single binary, zero C deps** — use pure-Rust crates only
2. **No tokio** — use `std::thread::scope` for parallelism
3. **Dynamic detection** — never hardcode paths or assumptions
4. **RUNPATH over RPATH** — modern standard
5. **Atomic transactions** — every install must be rollback-safe
6. **Zero clippy warnings** — `cargo clippy -- -D warnings` must be clean

---

## Next Steps

- [[Plugin Development]] — Detailed plugin development guide
- [[Architecture]] — System architecture and data flow
- [[Troubleshooting]] — Common development issues
