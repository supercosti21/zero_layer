# Zero Layer (ZL)

**Universal Linux package manager with native binary translation.**

Zero Layer installs packages from any source on any Linux distribution. It translates binaries at install time by patching ELF interpreters and library paths — no containers, no VMs, no isolation layers. Once installed, packages run with zero runtime overhead.

```bash
zl install firefox --from pacman      # Arch package on Ubuntu? Works.
zl install sharkdp/bat --from github  # GitHub release, patched and ready.
zl search vim                          # Search all 13 sources at once.
```

---

## Why Zero Layer?

Linux has dozens of package formats: `.deb`, `.rpm`, `.pkg.tar.zst`, `.apk`, `.snap`, `.flatpak`, `.AppImage`, Nix derivations... Each distro uses its own. If you want a package from another distro, you're out of luck — or you use containers, VMs, or compatibility layers that add overhead.

**Zero Layer solves this.** It downloads packages from any source, analyzes the ELF binaries inside, patches the dynamic linker and library paths to match your system, and installs them natively. The result is indistinguishable from a package installed by your distro's native package manager.

---

## Key Features

- **13 package sources** — pacman, AUR, APT, DNF, Zypper, APK, XBPS, Portage, Nix, GitHub Releases, Flatpak, Snap, AppImage
- **Works on any distro** — auto-detects arch, libc, dynamic linker, library paths, filesystem layout
- **Native binary translation** — ELF patching at install time, zero runtime overhead
- **Parallel everything** — downloads (4 threads), ELF patching (4 threads), source queries (all at once)
- **Atomic transactions** — any failure triggers full rollback
- **Dependency resolution** — recursive, cross-source, with conflict detection
- **Multi-version support** — install Python 3.11 and 3.12 side-by-side, switch between them
- **Ephemeral environments** — isolated shells where packages disappear on exit
- **Security** — SHA256 checksums + GPG signatures verified before install
- **CVE auditing** — check packages against OSV.dev vulnerability database
- **History & rollback** — undo any install, remove, or upgrade operation
- **Single binary, pure Rust** — no C dependencies, no dynamic linking required

---

## Quick Navigation

### Getting Started
- [[Installation]] — Download, build from source, post-install setup
- [[Getting Started]] — First run wizard, your first install, basic workflow

### User Guide
- [[Package Sources]] — All 13 sources explained, managing and configuring sources
- [[Commands Reference]] — Complete reference for every `zl` command
- [[Configuration]] — Full `config.toml` reference with all options

### Deep Dive
- [[Architecture]] — How ZL works internally, directory layout, data flow
- [[ELF Binary Translation]] — The core technology: how ELF patching works
- [[Dependency Resolution]] — How dependencies are resolved across sources
- [[Security and Verification]] — Checksums, GPG signatures, CVE auditing

### Development
- [[Plugin Development]] — How to create a new package source plugin
- [[Contributing]] — Development workflow, testing, code style
- [[Troubleshooting]] — Common issues and solutions

---

## Supported Sources

| Source | Platform | Format | Live/Cached |
|--------|----------|--------|-------------|
| **pacman** | Arch Linux | `.pkg.tar.zst` | Cached DB |
| **aur** | Arch User Repository | `.pkg.tar.zst` | Live API |
| **apt** | Debian, Ubuntu, Mint | `.deb` | Cached index |
| **dnf** | Fedora, RHEL, CentOS, Rocky, Alma | `.rpm` | Cached XML |
| **zypper** | openSUSE, SLES | `.rpm` | Cached XML |
| **apk** | Alpine Linux | `.apk` | Cached index |
| **xbps** | Void Linux | `.xbps` | Cached |
| **portage** | Gentoo | `.tbz2` / `.gpkg.tar` | Cached index |
| **nix** | NixOS / any distro with Nix | `.nar` | Live API |
| **github** | GitHub Releases | tar.gz, zip, binary | Live API |
| **flatpak** | Flathub | Flatpak bundle | Live API |
| **snap** | Snapcraft Store | `.snap` (SquashFS) | Live API |
| **appimage** | AppImageHub | `.AppImage` | Cached feed |

---

## Links

- [GitHub Repository](https://github.com/supercosti21/zero_layer)
- [Releases](https://github.com/supercosti21/zero_layer/releases)
- [Issues & Bug Reports](https://github.com/supercosti21/zero_layer/issues)
- [License (GPL v3)](https://github.com/supercosti21/zero_layer/blob/main/LICENSE)
