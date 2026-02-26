# Installation

This guide covers all methods of installing Zero Layer on your system.

---

## System Requirements

- **OS**: Any Linux distribution (x86_64)
- **Kernel**: Linux 4.x or later
- **libc**: glibc 2.17+ or musl
- **Disk**: ~15 MB for the binary, plus space for installed packages

> **Note**: ZL is a single static binary. It has no runtime dependencies — no Python, no Node.js, no shared libraries needed.

---

## Option 1: Pre-built Binary (Recommended)

Download the latest release directly:

### System-wide install (requires sudo)

```bash
curl -Lo zl https://github.com/supercosti21/zero_layer/releases/latest/download/zl-x86_64-unknown-linux-gnu
chmod +x zl
sudo mv zl /usr/local/bin/
```

### User-local install (no sudo)

```bash
mkdir -p ~/.local/bin
curl -Lo ~/.local/bin/zl https://github.com/supercosti21/zero_layer/releases/latest/download/zl-x86_64-unknown-linux-gnu
chmod +x ~/.local/bin/zl
```

If you install to `~/.local/bin`, make sure it's in your PATH:

```bash
# Bash — add to ~/.bashrc
echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.bashrc
source ~/.bashrc

# Zsh — add to ~/.zshrc
echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.zshrc
source ~/.zshrc

# Fish
fish_add_path ~/.local/bin
```

---

## Option 2: Build from Source

Requires **Rust 1.85+** (edition 2024).

### Install Rust

If you don't have Rust installed:

```bash
# Recommended: via rustup (works on any distro)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env

# Arch Linux alternative
sudo pacman -S rust

# Fedora alternative
sudo dnf install rust cargo

# Ubuntu/Debian alternative
sudo apt install rustc cargo
```

> **Important**: Distribution packages may ship older Rust versions. ZL requires Rust 1.85+. Use `rustup` if your distro's Rust is too old. Check with `rustc --version`.

### Clone and Build

```bash
git clone https://github.com/supercosti21/zero_layer.git
cd zero_layer
cargo build --release
```

The binary will be at `target/release/zl`.

### Install the Binary

```bash
# System-wide
sudo cp target/release/zl /usr/local/bin/

# Or user-local
mkdir -p ~/.local/bin
cp target/release/zl ~/.local/bin/
```

---

## Option 3: Self-Update

If ZL is already installed, update to the latest version:

```bash
zl self-update
```

This downloads the latest release binary from GitHub and replaces the current binary in-place.

---

## Post-Install Setup

### 1. Add ZL's bin directory to PATH

ZL installs package executables to `~/.local/share/zl/bin/`. You need to add this to your PATH so installed packages are accessible:

```bash
# Bash — add to ~/.bashrc
echo 'export PATH="$HOME/.local/share/zl/bin:$PATH"' >> ~/.bashrc
source ~/.bashrc

# Zsh — add to ~/.zshrc
echo 'export PATH="$HOME/.local/share/zl/bin:$PATH"' >> ~/.zshrc
source ~/.zshrc

# Fish — run once
fish_add_path ~/.local/share/zl/bin
```

### 2. Enable shell completions

Tab completions make ZL much easier to use:

```bash
# Bash — add to ~/.bashrc
echo 'eval "$(zl completions bash)"' >> ~/.bashrc

# Zsh — add to ~/.zshrc
echo 'eval "$(zl completions zsh)"' >> ~/.zshrc

# Fish — run once
zl completions fish > ~/.config/fish/completions/zl.fish
```

### 3. Verify the installation

```bash
zl --version
zl --help
```

### 4. Sync package indexes

Some sources require syncing before you can search or install:

```bash
# Arch Linux repos (pacman)
zl update --from pacman

# Debian/Ubuntu repos (apt)
zl update --from apt

# Fedora repos (dnf)
zl update --from dnf

# openSUSE repos (zypper)
zl update --from zypper

# Alpine repos (apk)
zl update --from apk
```

Sources that use live APIs (GitHub, AUR, Nix, Flatpak, Snap, AppImage) don't need syncing.

---

## Verify Everything Works

```bash
# Search for a package
zl search ripgrep

# Install something
zl install ripgrep --from github

# Check it works
rg --version

# Clean up
zl remove ripgrep
```

---

## Uninstalling ZL

To completely remove Zero Layer:

```bash
# Remove the binary
sudo rm /usr/local/bin/zl
# or: rm ~/.local/bin/zl

# Remove all installed packages, database, and cache
rm -rf ~/.local/share/zl

# Remove configuration
rm -rf ~/.config/zl

# Remove PATH and completion lines from your shell config
# (manually edit ~/.bashrc, ~/.zshrc, or fish config)
```

---

## Next Steps

- [[Getting Started]] — First run wizard and basic workflow
- [[Package Sources]] — Configure which sources to use
- [[Commands Reference]] — Full command reference
