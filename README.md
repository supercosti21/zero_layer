<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 128 128'%3E%3Cdefs%3E%3ClinearGradient id='g' x1='0' y1='0' x2='1' y2='1'%3E%3Cstop offset='0' stop-color='%237c5bf0'/%3E%3Cstop offset='1' stop-color='%23a78bfa'/%3E%3C/linearGradient%3E%3C/defs%3E%3Crect width='128' height='128' rx='28' fill='url(%23g)'/%3E%3Cpath d='M38 42h42l-34 44h42' stroke='white' stroke-width='10' stroke-linecap='round' stroke-linejoin='round' fill='none'/%3E%3Ccircle cx='94' cy='42' r='5.5' fill='white' opacity='0.85'/%3E%3C/svg%3E">
    <img src="data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 128 128'%3E%3Cdefs%3E%3ClinearGradient id='g' x1='0' y1='0' x2='1' y2='1'%3E%3Cstop offset='0' stop-color='%237c5bf0'/%3E%3Cstop offset='1' stop-color='%23a78bfa'/%3E%3C/linearGradient%3E%3C/defs%3E%3Crect width='128' height='128' rx='28' fill='url(%23g)'/%3E%3Cpath d='M38 42h42l-34 44h42' stroke='white' stroke-width='10' stroke-linecap='round' stroke-linejoin='round' fill='none'/%3E%3Ccircle cx='94' cy='42' r='5.5' fill='white' opacity='0.85'/%3E%3C/svg%3E" alt="Zero Layer" width="120">
  </picture>
</p>

# Zero Layer (ZL)

**Universal Linux package manager with native binary translation.**

Install packages from any source on any Linux distro. ZL translates binaries at install time — no containers, no VMs, zero runtime overhead.

```bash
zl install firefox --from pacman      # Arch package on Ubuntu? No problem.
zl install ripgrep --from github      # GitHub release, patched and ready.
zl search vim                          # Search all sources at once.
```

## Installation

### Pre-built binary (recommended)

```bash
curl -Lo zl https://github.com/supercosti21/zero_layer/releases/latest/download/zl-x86_64-unknown-linux-gnu
chmod +x zl
sudo mv zl /usr/local/bin/
```

Or install without sudo:

```bash
mkdir -p ~/.local/bin
curl -Lo ~/.local/bin/zl https://github.com/supercosti21/zero_layer/releases/latest/download/zl-x86_64-unknown-linux-gnu
chmod +x ~/.local/bin/zl
```

### Build from source

Requires Rust 1.85+.

```bash
git clone https://github.com/supercosti21/zero_layer.git
cd zero_layer
cargo build --release
cp target/release/zl ~/.local/bin/
```

### Self-update

```bash
zl self-update
```

### Setup

**Add ZL to your PATH:**

```bash
# Bash (~/.bashrc) or Zsh (~/.zshrc)
export PATH="$HOME/.local/share/zl/bin:$PATH"

# Fish
fish_add_path ~/.local/share/zl/bin
```

**Enable shell completions:**

```bash
# Bash
eval "$(zl completions bash)"

# Zsh
eval "$(zl completions zsh)"

# Fish
zl completions fish > ~/.config/fish/completions/zl.fish
```

## Package Sources

ZL supports 13 package sources out of the box:

| Source | Description | Example |
|--------|-------------|---------|
| **pacman** | Arch Linux repos | `zl install firefox --from pacman` |
| **aur** | Arch User Repository | `zl install yay --from aur` |
| **apt** | Debian/Ubuntu repos | `zl install vim --from apt` |
| **dnf** | Fedora/RHEL repos | `zl install gcc --from dnf` |
| **zypper** | openSUSE/SLES repos | `zl install vim --from zypper` |
| **apk** | Alpine Linux repos | `zl install curl --from apk` |
| **xbps** | Void Linux repos | `zl install curl --from xbps` |
| **portage** | Gentoo binhost | `zl install vim --from portage` |
| **nix** | Nix packages (nixpkgs) | `zl search ripgrep --from nix` |
| **github** | GitHub Releases | `zl install sharkdp/bat --from github` |
| **flatpak** | Flathub | `zl search firefox --from flatpak` |
| **snap** | Snapcraft Store | `zl search firefox --from snap` |
| **appimage** | AppImageHub | `zl search firefox --from appimage` |

### Managing sources

On first run, ZL detects your distro and suggests which sources to enable. You can change this anytime:

```bash
zl sources list                     # Show all sources (enabled/disabled)
zl sources enable pacman aur        # Enable specific sources
zl sources disable snap flatpak     # Disable specific sources
zl sources only pacman aur github   # Enable ONLY these, disable all others
zl sources reset                    # Re-enable all sources
```

You can also filter per-command:

```bash
zl search vim --from pacman          # Search one source
zl search vim --from pacman,apt      # Search multiple sources (comma-separated)
zl install vim --from apt            # Install from a specific source
```

### Source configuration

Each source can be configured in `~/.config/zl/config.toml`:

```toml
[general]
sources = ["pacman", "aur", "github"]   # Only use these sources (optional)

[plugins.pacman]
mirrorlist = "/etc/pacman.d/mirrorlist"
repos = ["core", "extra"]

[plugins.apt]
mirror = "http://archive.ubuntu.com/ubuntu"
suite = "noble"
components = ["main", "universe"]

[plugins.dnf]
mirror = "https://dl.fedoraproject.org/pub/fedora/linux"
release = "40"

[plugins.github]
token = "ghp_..."   # Optional: avoid rate limiting
```

## Usage

### Install

```bash
zl install <package>                 # Auto-detect best source
zl install <package> --from pacman   # From a specific source
zl install owner/repo --from github  # From GitHub Releases
zl install <package> --version 1.0   # Specific version
zl -y install <package>              # Skip confirmation
zl --dry-run install <package>       # Preview without changes
```

### Remove

```bash
zl remove <package>                  # Remove package
zl remove <package> --cascade        # Also remove unused dependencies
zl --dry-run remove <package>        # Preview
```

### Search

```bash
zl search <query>                    # Search all enabled sources
zl search <query> --from pacman      # Search specific source
zl search <query> --exact            # Exact name matches only
zl search <query> --sort name        # Sort by name (default: relevance)
```

### Update & Upgrade

```bash
zl update                            # Sync indexes and update packages
zl update --from apt                 # Update only APT packages
zl upgrade                           # Upgrade all packages
zl upgrade --check                   # Preview upgrades without changes
```

### Info & List

```bash
zl list                              # All installed packages
zl list --explicit                   # Only explicitly installed
zl list --orphans                    # Unused dependencies
zl info <package>                    # Detailed package info
```

### Run without installing

```bash
zl run <package>                     # Download, run, cleanup
zl run <package> -- --help           # Pass args to the binary
```

### Multi-version management

```bash
zl install python --version 3.11
zl install python --version 3.12    # Side-by-side
zl switch python 3.12               # Activate 3.12
```

### Ephemeral environments

```bash
zl env shell                         # Temporary env (deleted on exit)
zl env shell myproject               # Named env (persists)
zl env list                          # List environments
zl env delete myproject              # Delete an environment
```

### Diagnostics

```bash
zl doctor                            # System health check
zl why <package>                     # Why is this installed?
zl size                              # Disk usage per package
zl diff <package>                    # What would change on update?
zl audit                             # Check for known CVEs
```

### History & Rollback

```bash
zl history list                      # Show operation history
zl history rollback                  # Undo last operation
zl history rollback 3                # Undo last 3 operations
```

### Cache

```bash
zl cache list                        # Show cached downloads
zl cache clean                       # Clear cache
zl cache dedup                       # Deduplicate shared libraries
```

### Other

```bash
zl pin <package>                     # Prevent updates
zl unpin <package>                   # Allow updates
zl export                            # Export lockfile (zl-lock.json)
zl import zl-lock.json               # Import lockfile
```

### Global flags

| Flag | Description |
|------|-------------|
| `-v` | Verbose output |
| `-vv` | Debug output |
| `-y` | Auto-confirm prompts |
| `--dry-run` | Preview without changes |
| `--skip-verify` | Skip checksum/GPG verification |
| `--root <path>` | Custom ZL root directory |

## How It Works

When you run `zl install <package>`:

1. **Resolve** — find the package and all its dependencies
2. **Check conflicts** — file ownership, binary names, library sonames, version constraints
3. **Download** — parallel downloads (4 threads) with progress bars and retry
4. **Verify** — SHA256 checksum + GPG signature
5. **Extract** — unpack the archive (supports deb, rpm, tar, nar, squashfs, zip, AppImage)
6. **Patch** — set the correct dynamic linker and RUNPATH on all ELF binaries
7. **Install** — atomic transaction with automatic rollback on failure
8. **Track** — record files, dependencies, and history in the database

All translation happens at install time. Installed packages run with zero overhead.

## Architecture

### Directory layout

```
~/.local/share/zl/
  bin/          # Symlinks to executables (add to PATH)
  lib/          # Shared libraries
  packages/     # Per-package directories
  cache/        # Download cache
  envs/         # Environments
  zl.redb       # Database
```

### Key design choices

- **Single binary, pure Rust** — no C dependencies
- **Auto-detects everything** — arch, dynamic linker, libc, library paths, filesystem layout
- **Works on any distro** — Arch, Ubuntu, Fedora, Alpine, NixOS, Void, Gentoo, Termux...
- **Atomic transactions** — install fails = full rollback
- **Parallel downloads and ELF patching** — fast installs
- **Cross-source dependency resolution** — deps not found in the primary source are searched across all other sources

## Development

```bash
cargo build              # Build
cargo test               # Run all 264 tests
cargo clippy             # Lint
cargo fmt                # Format
```

## License

MIT — see [LICENSE](LICENSE) for details.
