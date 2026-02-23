<p align="center">
  <img src="assets/logo.png" alt="Zero Layer" width="200">
</p>

# Zero Layer (ZL)

**Universal Linux package manager with native binary translation.**

Install packages from any source — pacman, apt, rpm, AppImage, GitHub releases, pip, npm, cargo — on any Linux system. ZL translates packages natively at install time: no containers, no VMs, no isolation layers. After installation, packages are indistinguishable from native ones. Zero runtime overhead.

## How It Works

```
zl install firefox --from pacman
```

1. **Resolve** dependencies recursively — find all transitive deps from the source
2. **Check for conflicts** — file ownership, binary names, library sonames, version constraints
3. **Download** all packages in parallel (up to 4 concurrent) with progress bars and retry
4. **Verify** — SHA256 checksum + GPG signature verification
5. **Analyze** all ELF binaries — detect interpreters, shared library dependencies, RPATH/RUNPATH
6. **Patch** binaries — set the correct dynamic linker and RUNPATH for the target system
7. **Remap** hardcoded FHS paths (`/usr/lib`, `/usr/bin`, `/etc`) to ZL-managed directories
8. **Install** with atomic transactions — automatic rollback if anything fails
9. **Track** every file in a persistent database with dependency relationships
10. **Verify** that all ELF binaries can resolve their dependencies post-install

All translation happens at install time. Once installed, a package runs with zero overhead.

## Installation

### Option 1 — Pre-built binary (recommended)

Download the latest binary from the [releases page](https://github.com/supercosti21/zero_layer/releases):

```bash
curl -Lo zl https://github.com/supercosti21/zero_layer/releases/latest/download/zl-x86_64-unknown-linux-gnu
chmod +x zl
sudo mv zl /usr/local/bin/
```

Or install to your home directory (no sudo required):

```bash
mkdir -p ~/.local/bin
curl -Lo ~/.local/bin/zl https://github.com/supercosti21/zero_layer/releases/latest/download/zl-x86_64-unknown-linux-gnu
chmod +x ~/.local/bin/zl
# Make sure ~/.local/bin is in your PATH (see below)
```

> **Note:** v0.1.0 provides an x86_64 glibc binary (requires glibc 2.17+, works on any modern Linux distro).
> Static musl builds and aarch64 support are planned for v0.2.0.

### Option 2 — Build from source

Requires Rust 1.85+ (edition 2024).

**Install Rust** (if not already installed):

```bash
# Arch Linux
sudo pacman -S rust

# Other distros
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env
```

**Clone and build:**

```bash
git clone https://github.com/supercosti21/zero_layer.git
cd zero_layer
cargo build --release
```

**Install the binary:**

```bash
# System-wide (requires sudo)
sudo cp target/release/zl /usr/local/bin/

# Or user-local (no sudo)
mkdir -p ~/.local/bin
cp target/release/zl ~/.local/bin/
```

### Option 3 — Self-update (once ZL is already installed)

```bash
zl self-update    # Download and replace the binary with the latest release
```

---

### Post-install setup

**1. Add ZL's bin directory to your PATH** so that packages installed by ZL are accessible:

```bash
# Bash — add to ~/.bashrc
echo 'export PATH="$HOME/.local/share/zl/bin:$PATH"' >> ~/.bashrc
source ~/.bashrc

# Zsh — add to ~/.zshrc
echo 'export PATH="$HOME/.local/share/zl/bin:$PATH"' >> ~/.zshrc
source ~/.zshrc

# Fish — add to config.fish
fish_add_path ~/.local/share/zl/bin
```

Also add `~/.local/bin` to PATH if you installed the binary there:

```bash
# Bash / Zsh
echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.bashrc   # or ~/.zshrc

# Fish
fish_add_path ~/.local/bin
```

**2. Enable shell completions:**

```bash
# Bash — add to ~/.bashrc
echo 'eval "$(zl completions bash)"' >> ~/.bashrc

# Zsh — add to ~/.zshrc
echo 'eval "$(zl completions zsh)"' >> ~/.zshrc

# Fish — run once
zl completions fish > ~/.config/fish/completions/zl.fish
```

**3. Verify the installation:**

```bash
zl --help
zl --version
```

**4. Sync a package index** before installing (required for APT, recommended for Pacman):

```bash
# Pacman (Arch) — downloads and caches the package database
zl update --from pacman

# APT (Debian/Ubuntu) — downloads Packages.gz index
zl update --from apt
```

## Usage

### Quick start

```bash
# Search for a package across all configured sources
zl search ripgrep

# Install a package (auto-resolves and installs all dependencies)
zl install ripgrep

# Install from a specific source
zl install ripgrep --from pacman
zl install ripgrep --from github    # installs BurntSushi/ripgrep from GitHub Releases
zl install vim --from apt
zl install yay --from aur

# Remove a package
zl remove ripgrep

# List all installed packages
zl list
```

### Installing packages

```bash
# Basic install (ZL picks the best available source)
zl install <package>

# Specify source explicitly
zl install <package> --from pacman   # Arch repos
zl install <package> --from aur      # Arch User Repository (builds from source)
zl install <package> --from apt      # Debian/Ubuntu repos (sync index first)
zl install owner/repo --from github  # GitHub Releases (must use owner/repo format)

# Install a specific version
zl install firefox --version 120.0

# Preview what would be installed without making changes
zl --dry-run install firefox
zl --simulate install firefox        # same as --dry-run

# Skip checksum/GPG verification (not recommended)
zl --skip-verify install <package>

# Auto-confirm all prompts (for scripts)
zl -y install <package>
```

**Example — installing from GitHub Releases:**

```bash
# Must use owner/repo format
zl install sharkdp/bat --from github
zl install sharkdp/fd --from github
zl install BurntSushi/ripgrep --from github
zl install cli/cli --from github         # GitHub CLI

# Search first to find the full name
zl search bat --from github
```

**Example — installing from APT:**

```bash
# Sync the index first (run once, or when you want to refresh)
zl update --from apt

# Then install
zl install vim --from apt
zl install build-essential --from apt
```

### Removing packages

```bash
zl remove <package>                # Remove the package (keeps orphaned deps)
zl remove <package> --cascade      # Remove package + any deps no longer needed
zl remove <package> --version 1.0  # Remove a specific version only
zl --dry-run remove <package>      # Preview what would be removed
```

### Search & Info

```bash
zl search <query>                  # Search all registered sources
zl search <query> --from pacman    # Search a specific source only
zl search vim --from apt           # Search APT index (must sync first)
zl search ripgrep --from github    # Search GitHub repositories

zl info <package>                  # Detailed info about an installed package
                                   # (version, source, deps, reverse-deps, disk usage)
```

### Update & Upgrade

```bash
# Sync package indexes and update installed packages
zl update                          # Update all packages (skips pinned)
zl update <package>                # Update a specific package
zl update --from pacman            # Update only packages from pacman

# Mass upgrade
zl upgrade                         # Check all packages, show summary, confirm, then upgrade all
zl upgrade --check                 # Preview only — show what would be upgraded (no changes)
zl upgrade --from apt              # Upgrade only packages from a specific source
```

### Multi-Version Management

Install multiple versions of the same package side-by-side:

```bash
zl install python --version 3.11   # Install Python 3.11
zl install python --version 3.12   # Install Python 3.12 alongside 3.11
zl switch python 3.12              # Activate version 3.12 (update bin/ symlinks)
zl switch python 3.11              # Switch back to 3.11
zl remove python --version 3.11    # Remove only version 3.11
zl list                            # Shows active version with [active] marker
```

### Ephemeral Environments

Create isolated environments where packages disappear when you exit:

```bash
zl env shell                       # Enter a TEMPORARY environment (auto-deleted on exit)
zl env shell myproject             # Enter/create a NAMED environment (persists)
zl env list                        # List existing named environments
zl env delete myproject            # Delete a named environment
```

Inside an environment shell:
- A separate ZL root is used (`~/.local/share/zl/envs/<name>/`)
- The env's `bin/` and `lib/` are prepended to `PATH` and `LD_LIBRARY_PATH`
- Install packages with `zl --root $ZL_ENV_ROOT install <pkg>`
- Temporary environments are completely deleted when you type `exit`

### List

```bash
zl list                            # List all installed packages
zl list --explicit                 # Only explicitly installed packages
zl list --deps                     # Only packages installed as dependencies
zl list --orphans                  # Show orphaned dependencies
```

### Package Pinning

```bash
zl pin <package>                   # Pin a package (prevent updates)
zl unpin <package>                 # Unpin a package (allow updates)
```

### Run Without Installing

```bash
zl run <package>                   # Download, patch, execute, then cleanup
zl run <package> --from github     # Run from a specific source
zl run <package> -- --help         # Pass args to the binary
```

### Diagnostics & Analysis

```bash
zl doctor                          # Full system health check (DB, symlinks, libs, orphans)
zl why <package>                   # Show why a package is installed (dependency chain)
zl size                            # Disk usage per package
zl size <package>                  # Detailed breakdown with file sizes and dep costs
zl size --sort                     # Sort by size (largest first)
zl diff <package>                  # Show what would change if updated
zl audit                           # Check all packages for known CVEs (via OSV.dev)
zl audit <package>                 # Check a specific package
```

### History & Rollback

```bash
zl history list                    # Show install/remove/upgrade history
zl history rollback                # Undo the last operation
zl history rollback 3              # Undo the last 3 operations
```

### Cache Management

```bash
zl cache list                      # Show cached downloads and sizes
zl cache clean                     # Remove all cached files
zl cache dedup                     # Deduplicate shared libraries (hardlinks)
```

### Lockfile Export/Import

```bash
zl export                          # Export to zl-lock.json
zl export mypackages.json          # Export to custom file
zl import zl-lock.json             # Show packages to install from lockfile
```

### Shell Completions

```bash
zl completions bash                # Generate bash completions
zl completions zsh                 # Generate zsh completions
zl completions fish                # Generate fish completions
```

### Global Flags

```bash
zl -v ...              # Verbose output (show sync progress, package counts)
zl -vv ...             # Debug output (full trace)
zl -y ...              # Auto-confirm prompts
zl --root /custom/path ...  # Use a custom ZL root directory
zl --dry-run ...       # Show what would happen without making changes
zl --simulate ...      # Same as --dry-run
zl --skip-verify ...   # Skip checksum and GPG signature verification
```

## Architecture

### Directory Layout

ZL manages all packages under `~/.local/share/zl/`, and integrates with standard XDG user directories:

```
~/.local/share/zl/
  bin/          # Symlinks to executables (user adds to PATH)
  lib/          # Shared libraries (deduplicated across packages)
  share/        # Shared data files
  etc/          # Config files
  packages/     # Per-package directories (name-version/)
  cache/        # Download cache
  envs/         # Ephemeral/named environment roots

$XDG_DATA_HOME/ (default: ~/.local/share/)
  applications/ # .desktop files symlinked here → apps appear in launcher
  icons/        # Package icons symlinked here → icons appear in desktop
  zl.redb       # Package database
```

### Package Verification

ZL verifies package integrity before installation:

1. **SHA256 checksum** — mandatory when available. Mismatches cause immediate failure.
2. **GPG signature** — downloaded alongside the package when available (`.sig` files). Verified using the system `gpg` binary. Best-effort: skipped if gpg is not installed or no signature exists.

Use `--skip-verify` to bypass all verification (not recommended).

### Plugin System

Package sources are implemented as **plugins** behind the `SourcePlugin` trait. Each plugin handles search, resolve, download, and extraction for its source format. Currently implemented:

| Plugin | Source | Usage |
|--------|--------|-------|
| **pacman** | Arch Linux official repos (core, extra) | `zl install firefox --from pacman` |
| **aur** | Arch User Repository (live queries) | `zl install yay --from aur` |
| **apt** | Debian/Ubuntu APT repos | `zl install vim --from apt` |
| **github** | GitHub Releases | `zl install sharkdp/bat --from github` |

**AUR plugin** — uses AUR RPC API v5 for search and resolve, then builds with `git clone` + `makepkg`. Requires `base-devel` and `git` to be installed.

**APT plugin** — downloads and parses `Packages.gz` index, then downloads `.deb` files. Configure in `~/.config/zl/config.toml`:

```toml
[plugins.apt]
mirror     = "http://archive.ubuntu.com/ubuntu"   # or deb.debian.org/debian
suite      = "noble"                               # noble, bookworm, focal, jammy, etc.
components = ["main", "universe"]
arch       = "amd64"                               # auto-detected if omitted
```

Run `zl update --from apt` to sync the package index before installing.

**GitHub plugin** — fetches the latest release from any public GitHub repository. Smart asset selection: prefers musl+linux binaries and tar.gz format, skips Windows/macOS/deb/rpm assets. Supports tar.gz, tar.xz, tar.zst, zip, AppImage, and bare binary assets.

```toml
[plugins.github]
token = "ghp_..."   # Optional: GitHub token to avoid rate limiting
```

Planned:
- RPM (Fedora/RHEL)
- AppImage
- pip, npm, cargo

### Universal Distro Support (SystemProfile)

ZL auto-detects the host system at startup — no manual configuration needed. It discovers:

- **CPU architecture** — x86_64, aarch64, armv7, riscv64, i686, s390x, ppc64le
- **Dynamic linker** — detected by reading PT_INTERP from an existing system ELF (e.g., `/bin/sh`)
- **C library** — glibc or musl (detected from the interpreter name)
- **Library search paths** — from `ldconfig -p`, `ld.so.conf`, `LD_LIBRARY_PATH`, and layout-specific locations
- **Filesystem layout** — FHS, Merged /usr, NixOS, GNU Guix, Termux, GoboLinux

This means ZL works on any Linux distro without hardcoded assumptions: Arch, Ubuntu, Fedora, Alpine (musl), NixOS, Void, Gentoo, Clear Linux, Termux on Android, and more.

### Automatic Dependency Resolution

When you install a package, ZL automatically resolves and installs all dependencies:

```
$ zl install firefox
Syncing package database from Arch Linux (pacman)...
Resolving dependencies...
Checking for conflicts...
Verifying packages...

Dependencies to install (12):
  dbus-glib 0.112-3 (0.4 MB)
  gtk3 3.24.39-1 (23.1 MB)
  libxt 1.3.0-1 (0.3 MB)
  ...

Packages to install (1):
  firefox 120.0-1 (238.0 MB)

Total installed size: 285.4 MB

Proceed with installation? [Y/n]

Downloading 13 package(s)...
  [==========>                    ] 5/13 downloads
    gtk3 downloaded
    dbus-glib downloaded
  ...

[1/13] Installing dbus-glib...
[2/13] Installing gtk3...
...
[13/13] Installing firefox...

Installed 1 package(s) + 12 dependency(ies).
```

Dependencies are downloaded in parallel (up to 4 at a time) with progress bars and installed in correct dependency order. Virtual packages (e.g., `sh` provided by `bash`) are resolved automatically.

### Conflict Detection

Before installing, ZL checks for 5 types of conflicts:

1. **File ownership** — detects if any file would overwrite a file from another package
2. **Binary name** — detects if two packages provide the same executable name
3. **Library soname** — detects if two packages provide the same shared library
4. **Declared conflicts** — respects `conflicts` declarations from package metadata
5. **Version constraints** — detects incompatible version requirements (e.g., pkg A needs glibc>=2.34 but glibc 2.17 is installed)

### Atomic Transactions

Every install is wrapped in a transaction. If any package fails to install:
- All files, symlinks, and directories created during the install are removed
- All database entries are rolled back
- The system is left in its pre-install state

### Package Pinning

Pin packages to prevent them from being updated:

```bash
$ zl pin firefox
Pinned firefox-120.0 (will not be updated).

$ zl update
All packages are up to date.
1 pinned package(s) skipped.

$ zl unpin firefox
Unpinned firefox (updates allowed).
```

### Dry-Run Mode

Preview what any operation would do without making changes:

```bash
$ zl --dry-run install firefox
[DRY-RUN] Simulating install of firefox...
Syncing package database from Arch Linux (pacman)...
Resolving dependencies...
Checking for conflicts...

Packages to install (1):
  firefox 120.0-1 (238.0 MB)

[DRY-RUN] Would install 1 package(s). No changes made.
```

### Source Build Support

ZL can build packages from source when precompiled binaries aren't available. It auto-detects the build system:

- **Autotools** — `./configure && make && make install`
- **CMake** — `cmake -B build && cmake --build build`
- **Meson** — `meson setup build && ninja -C build`
- **Cargo** — `cargo build --release` (Rust projects)
- **Make** — simple Makefile projects

### Key Design Choices

- **Pure Rust, single binary** — no C dependencies, no dynamic linking required
- **Dynamic system detection** — all paths and interpreters auto-detected, never hardcoded
- **Parallel downloads** — up to 4 concurrent downloads with progress bars and exponential backoff retry
- **Package verification** — SHA256 checksums + GPG signatures verified before install
- **Atomic transactions** — all installs can be rolled back on failure
- **5-way conflict detection** — prevents broken installs before they happen
- **Multi-version packages** — install multiple versions side-by-side, switch between them
- **Ephemeral environments** — isolated shells where packages disappear on exit
- **ELF patching with `elb`** — pure-Rust patchelf alternative, sets interpreter and RUNPATH, parallel patching for multi-ELF packages
- **RUNPATH over RPATH** — modern standard, respects `LD_LIBRARY_PATH`
- **`redb` database** — pure-Rust embedded key-value store (ACID, no SQLite/C dependency)
- **`petgraph` dependency graph** — topological sort, cycle detection, orphan detection
- **Cross-source dependency resolution** — when a dep is not found in the primary source, queries all other sources and lets the user choose
- **Colored output** — uses `console` crate for colored output throughout (search, list, doctor, audit, diff, size)
- **CVE auditing** — checks installed packages against the OSV.dev vulnerability database
- **History & rollback** — all install/remove events are recorded; undo recent operations
- **Cache deduplication** — identical shared libraries are hardlinked to save disk space

## Configuration

Optional config file at `~/.config/zl/config.toml`:

```toml
[general]
root = "/custom/zl/root"   # Override default root directory
auto_confirm = false        # Auto-confirm prompts

[system]
# All fields are optional — auto-detection is used by default.
# interpreter = "/custom/path/ld-linux.so"  # Override dynamic linker
# extra_lib_dirs = ["/opt/mylibs"]          # Extra library search dirs
# extra_bin_dirs = ["/opt/mybin"]           # Extra binary search dirs
# layout = "nixos"                          # Override detected layout

[plugins.pacman]
enabled = true
mirrorlist = "/etc/pacman.d/mirrorlist"  # Custom mirrorlist path
arch = "x86_64"
repos = ["core", "extra"]
```

## Development

```bash
cargo build              # Build
cargo test               # Run all tests (209 tests: 92 bin + 117 lib)
cargo test <name>        # Run a single test
cargo clippy             # Lint
cargo fmt                # Format
```

### Development Workflow Rules

1. **Feature branches** — Never work directly on `main`. Always create a branch first.
2. **Branch naming** — Use prefixed names: `feat/xxx`, `fix/xxx`, `chore/xxx`, `refactor/xxx`, `docs/xxx`.
3. **Merge** — Only merge to `main` when everything works (tests pass, clippy clean, fmt clean). Delete the branch after merge.
4. **Atomic commits** — 1 commit = 1 concept. Better 3 small focused commits than 1 giant commit.
5. **Commit message format** — `type: clear title` + bullet points for details. Types: `feat`, `fix`, `chore`, `refactor`, `docs`.
6. **Documentation** — After every significant change, update `CLAUDE.md` (implementation state, module structure, test count), then `README.md` if user-facing features changed.

## License

GPL v3 — see [LICENSE](LICENSE) for details.
