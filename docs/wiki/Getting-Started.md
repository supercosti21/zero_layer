# Getting Started

This guide walks you through your first experience with Zero Layer — from the first-run wizard to installing your first package.

---

## First Run

When you run ZL for the first time, it detects your Linux distribution and launches an interactive setup wizard:

```
Welcome to Zero Layer! Let's configure your package sources.

Select which package sources to enable:
  (Use space to toggle, enter to confirm)

  [x] pacman
  [x] aur
  [ ] apt
  [ ] dnf
  [ ] zypper
  [ ] apk
  [ ] xbps
  [ ] portage
  [ ] nix
  [x] github
  [ ] flatpak
  [ ] snap
  [x] appimage

Enabled sources: pacman, aur, github, appimage
  You can change this anytime with: zl sources
```

ZL auto-suggests sources based on your distro:
- **Arch Linux** → pacman, aur, github, flatpak, appimage
- **Ubuntu/Debian** → apt, github, flatpak, snap, appimage
- **Fedora** → dnf, github, flatpak, appimage
- **openSUSE** → zypper, github, flatpak, appimage
- **Alpine** → apk, github, appimage
- **Void** → xbps, github, flatpak, appimage
- **Gentoo** → portage, github, flatpak, appimage
- **NixOS** → nix, github, flatpak, appimage

GitHub, Flatpak, and AppImage are always suggested as universal sources that work on any distro.

You can skip the wizard (press Enter with nothing selected) to enable all sources.

---

## Basic Workflow

### Searching for packages

Search across all your enabled sources at once:

```bash
zl search firefox
```

Output:
```
────── Arch Linux (3 results) ──────
  firefox                        120.0          Standalone web browser from mozilla.org
  firefox-esr                    102.0          Extended Support Release
  firefox-developer-edition      121.0b3        Developer Edition

────── GitHub (1 result) ──────
  Mozilla/Firefox                121.0          Standalone web browser

4 result(s) across 2 source(s).
```

Search a specific source:
```bash
zl search firefox --from apt
```

Search multiple sources:
```bash
zl search firefox --from pacman,apt,github
```

### Installing packages

**With a specific source:**
```bash
zl install firefox --from pacman
```

**Let ZL choose (interactive):**
```bash
zl install firefox
```

When multiple sources have the package, ZL shows an interactive menu:

```
Searching all sources for 'firefox'...

? Select package source:
> firefox 120.0 [pacman/extra]
  firefox 121.0 [apt/main]
  Mozilla/Firefox 121.0 [github]
```

**The full install flow:**
```
Syncing package database from pacman...
Resolving dependencies...
Checking for conflicts...

Dependencies to install (5):
  gtk3 3.24.39 (23.1 MB)
  dbus-glib 0.112 (0.4 MB)
  libxt 1.3.0 (0.3 MB)
  nss 3.95 (4.2 MB)
  nspr 4.35 (0.5 MB)

Packages to install (1):
  firefox 120.0 (238.0 MB)

Total installed size: 266.5 MB

Proceed with installation? [Y/n]

[1/4] Downloading 6 package(s)...
  [████████████████████] 6/6 complete
[2/4] Verifying packages...
[3/4] Installing & patching...
[4/4] Done!

Installed 1 package(s) + 5 dependency(ies).
```

### Removing packages

```bash
# Remove just the package
zl remove firefox

# Remove the package and any dependencies it pulled in that nothing else needs
zl remove firefox --cascade
```

With `--cascade`, ZL shows a preview before removing:

```
Package: firefox-120.0 (157 files)

Cascade will also remove:
  - dbus-glib-0.112
  - libxt-1.3.0

Keeping (needed by other packages):
  - gtk3-3.24.39 (needed by gnome-shell)
  - nss-3.95 (needed by thunderbird)

Remove this package and 2 orphaned dependencies? [Y/n]
```

### Updating packages

```bash
# Update all packages
zl update

# Update from a specific source
zl update --from pacman

# Preview what would be upgraded
zl upgrade --check

# Upgrade everything
zl upgrade
```

---

## Common Tasks

### Install from GitHub Releases

GitHub Releases use `owner/repo` format:

```bash
zl install sharkdp/bat --from github
zl install sharkdp/fd --from github
zl install BurntSushi/ripgrep --from github
zl install cli/cli --from github
```

ZL automatically picks the right binary for your architecture (prefers musl+linux, avoids Windows/macOS).

### Install a specific version

```bash
zl install python --version 3.11 --from pacman
```

### Preview without changes

Use `--dry-run` with any command to see what would happen:

```bash
zl --dry-run install firefox --from pacman
zl --dry-run remove firefox --cascade
zl --dry-run upgrade
```

### Run a package without installing

Download, patch, execute, then auto-cleanup:

```bash
zl run ripgrep -- --help
zl run sharkdp/bat --from github -- README.md
```

### Check system health

```bash
zl doctor
```

This checks database integrity, broken symlinks, missing libraries, orphaned packages, disk usage, and system profile.

---

## Tips

1. **Use `--from` to be explicit** — When you know which source you want, specify it. It's faster and avoids ambiguity.

2. **Sync before installing from cached sources** — Run `zl update --from pacman` (or apt, dnf, etc.) to refresh the package index.

3. **Use `--cascade` when removing** — This prevents orphaned dependencies from accumulating.

4. **Pin important packages** — `zl pin firefox` prevents accidental upgrades.

5. **Check for vulnerabilities** — Run `zl audit` periodically to check for known CVEs.

6. **Use environments for testing** — `zl env shell` creates an isolated environment. Install anything, then `exit` to clean up.

---

## Next Steps

- [[Package Sources]] — Deep dive into all 13 sources and their configuration
- [[Commands Reference]] — Complete reference for every command and flag
- [[Configuration]] — Customize ZL's behavior with `config.toml`
