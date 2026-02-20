# Zero Layer (ZL)

**Universal Linux package manager with native binary translation.**

Install packages from any source — pacman, apt, rpm, AppImage, GitHub releases, pip, npm, cargo — on any Linux system. ZL translates packages natively at install time: no containers, no VMs, no isolation layers. After installation, packages are indistinguishable from native ones. Zero runtime overhead.

## How It Works

```
zl install firefox --from pacman
```

1. **Download** the package from the selected source (Arch repos, Debian repos, GitHub, etc.)
2. **Analyze** all ELF binaries — detect interpreters, shared library dependencies, RPATH/RUNPATH
3. **Patch** binaries — set the correct dynamic linker and RUNPATH for the target system
4. **Remap** hardcoded FHS paths (`/usr/lib`, `/usr/bin`, `/etc`) to ZL-managed directories
5. **Patch** scripts and config files, rewriting shebangs and embedded paths
6. **Track** every file in a dependency graph and persistent database
7. **Verify** that all ELF binaries can resolve their dependencies post-install

All translation happens at install time. Once installed, a package runs with zero overhead.

## Installation

### Build from source

Requires Rust 1.85+ (edition 2024).

```bash
git clone https://github.com/supercosti21/zero_layer.git
cd zero_layer
cargo build --release
# Binary is at target/release/zl
```

Add ZL's bin directory to your PATH:

```bash
export PATH="$HOME/.local/share/zl/bin:$PATH"
```

## Usage

```bash
zl install <package>              # Install a package
zl install <package> --from pacman # Install from a specific source
zl install <package> --version 1.0 # Install a specific version
zl remove <package>                # Remove a package
zl remove <package> --cascade      # Remove package and orphaned deps
zl search <query>                  # Search for packages
zl search <query> --from pacman    # Search a specific source
zl update                          # Update all packages
zl update <package>                # Update a specific package
zl list                            # List installed packages
```

Global flags:

```bash
zl -v ...    # Verbose output
zl -y ...    # Auto-confirm prompts
zl --root /custom/path ...  # Use a custom ZL root directory
```

## Architecture

### Directory Layout

ZL manages all packages under `~/.local/share/zl/`:

```
~/.local/share/zl/
  bin/          # Symlinks to executables (user adds to PATH)
  lib/          # Shared libraries (deduplicated across packages)
  share/        # Shared data files
  etc/          # Config files
  packages/     # Per-package directories (name-version/)
  cache/        # Download cache
  zl.redb       # Package database
```

### Plugin System

Package sources are implemented as **plugins** behind the `SourcePlugin` trait. Each plugin handles search, resolve, download, and extraction for its source format. Currently implemented:

- **Pacman** — Arch Linux official repositories (core, extra)

Planned:
- APT (Debian/Ubuntu)
- RPM (Fedora/RHEL)
- AppImage
- GitHub Releases
- pip, npm, cargo

### Key Design Choices

- **Pure Rust, single binary** — no C dependencies, no dynamic linking required
- **ELF patching with `elb`** — pure-Rust patchelf alternative, sets interpreter and RUNPATH
- **RUNPATH over RPATH** — modern standard, respects `LD_LIBRARY_PATH`
- **`redb` database** — pure-Rust embedded key-value store (ACID, no SQLite/C dependency)
- **`petgraph` dependency graph** — topological sort, cycle detection, orphan detection

## Configuration

Optional config file at `~/.config/zl/config.toml`:

```toml
[general]
root = "/custom/zl/root"   # Override default root directory
auto_confirm = false        # Auto-confirm prompts

[plugins.pacman]
enabled = true
mirrorlist = "/etc/pacman.d/mirrorlist"  # Custom mirrorlist path
arch = "x86_64"
repos = ["core", "extra"]
```

## Development

```bash
cargo build              # Build
cargo test               # Run all tests
cargo test <name>        # Run a single test
cargo clippy             # Lint
cargo fmt                # Format
```

## License

GPL v3 — see [LICENSE](LICENSE) for details.
