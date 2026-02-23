# Commands Reference

Complete reference for every `zl` command, subcommand, flag, and option.

---

## Table of Contents

- [Global Flags](#global-flags)
- [install](#install)
- [remove](#remove)
- [search](#search)
- [update](#update)
- [upgrade](#upgrade)
- [list](#list)
- [info](#info)
- [run](#run)
- [sources](#sources)
- [switch](#switch)
- [pin / unpin](#pin--unpin)
- [env](#env)
- [doctor](#doctor)
- [why](#why)
- [size](#size)
- [diff](#diff)
- [audit](#audit)
- [history](#history)
- [cache](#cache)
- [export / import](#export--import)
- [completions](#completions)
- [self-update](#self-update)

---

## Global Flags

These flags work with any command:

| Flag | Short | Description |
|------|-------|-------------|
| `--verbose` | `-v` | Verbose output (info level). Use `-vv` for debug level. |
| `--yes` | `-y` | Auto-confirm all interactive prompts |
| `--root <path>` | | Use a custom ZL root directory (default: `~/.local/share/zl`) |
| `--dry-run` | | Show what would happen without making changes |
| `--simulate` | | Alias for `--dry-run` |
| `--skip-verify` | | Skip SHA256 checksum and GPG signature verification |
| `--help` | `-h` | Show help for any command |
| `--version` | `-V` | Show ZL version |

**Verbosity levels:**
- Default: warnings and errors only
- `-v`: info messages (sync progress, package counts)
- `-vv`: debug messages (full HTTP requests, file operations)

---

## install

Install a package and all its dependencies.

```
zl install <package> [options]
```

### Arguments

| Argument | Description |
|----------|-------------|
| `<package>` | Package name (or `owner/repo` for GitHub) |

### Options

| Flag | Description |
|------|-------------|
| `--from <source>` | Install from a specific source |
| `--version <ver>` | Install a specific version |

### Behavior

1. If `--from` is specified, queries only that source
2. If `--from` is omitted, queries all enabled sources in parallel
3. If multiple sources have the package, shows an interactive selection menu
4. Resolves all dependencies recursively (with cross-source fallback)
5. Checks for 5 types of conflicts before installing
6. Downloads all packages in parallel (4 threads) with progress bars
7. Verifies checksums and GPG signatures
8. Extracts, patches ELF binaries, remaps paths
9. Installs atomically — rolls back on any failure
10. Records the install in history

### Examples

```bash
zl install firefox                          # Interactive source selection
zl install firefox --from pacman            # From Arch repos
zl install vim --from apt                   # From Debian/Ubuntu repos
zl install gcc --from dnf                   # From Fedora repos
zl install sharkdp/bat --from github        # From GitHub Releases
zl install yay --from aur                   # Build from AUR
zl install firefox --version 120.0          # Specific version
zl -y install firefox --from pacman         # Skip confirmation
zl --dry-run install firefox --from pacman  # Preview only
```

### Output

```
[1/4] Downloading 6 package(s)...
  [████████████████████] 6/6 complete
[2/4] Verifying packages...
[3/4] Installing & patching...
[4/4] Done!

Installed 1 package(s) + 5 dependency(ies).
```

### Post-install checks

After installing, ZL checks that all ELF binaries can find their shared libraries. If any are missing, it prints a warning:

```
Warning: firefox has unresolved shared libraries:
  libcustom.so.1 — not found in ZL DB or system lib dirs
```

---

## remove

Remove an installed package.

```
zl remove <package> [options]
```

### Options

| Flag | Description |
|------|-------------|
| `--cascade` | Also remove orphaned dependencies |
| `--version <ver>` | Remove only a specific version |

### Behavior

**Without `--cascade`:**
- Removes the package files, symlinks, and database entries
- Leaves dependencies installed (even if orphaned)

**With `--cascade`:**
1. Shows preview of what will be removed
2. Identifies orphaned dependencies (not needed by any remaining package)
3. Lists dependencies that will be kept (needed by other packages)
4. Prompts for confirmation
5. Removes the package and all orphans

### Examples

```bash
zl remove firefox                      # Remove package only
zl remove firefox --cascade            # Remove package + orphaned deps
zl remove firefox --version 120.0     # Remove specific version
zl --dry-run remove firefox --cascade  # Preview cascade removal
```

### Output (with --cascade)

```
Package: firefox-120.0 (157 files)

Cascade will also remove:
  - dbus-glib-0.112
  - libxt-1.3.0

Keeping (needed by other packages):
  - gtk3-3.24.39 (needed by gnome-shell)

Remove this package and 2 orphaned dependencies? [Y/n]
```

---

## search

Search for packages across all enabled sources.

```
zl search <query> [options]
```

### Options

| Flag | Description |
|------|-------------|
| `--from <sources>` | Comma-separated list of sources to search |
| `--limit <num>` | Max results per source (default: 20) |
| `--sort <order>` | Sort order: `relevance` (default), `name`, `version` |
| `--exact` | Only show exact name matches |

### Relevance scoring

Each result is scored for relevance:

| Score | Match type |
|-------|-----------|
| 100 | Exact name match |
| 80 | Name starts with query |
| 60 | Name contains query |
| 30 | Description contains query |
| 10 | Matched by plugin but not by heuristics |

### Output

```
────── Arch Linux (3 results) ──────
  firefox                        120.0          Standalone web browser from mozilla.org
  firefox-esr                    102.0          Extended Support Release
  firefox-developer-edition      121.0b3        Developer Edition

────── GitHub (1 result) ──────
  Mozilla/Firefox                121.0          Standalone web browser

4 result(s) across 2 source(s).
Tip: use `zl search firefox --exact` for exact matches only.
```

- Exact matches are highlighted in green+bold
- Versions are shown in yellow
- Source headers are in cyan
- If results are truncated, a hint shows how to see more

### Examples

```bash
zl search ripgrep                         # Search all sources
zl search ripgrep --from github           # Search GitHub only
zl search vim --from pacman,apt           # Search two sources
zl search firefox --exact                 # Exact matches only
zl search python --sort name              # Sort alphabetically
zl search node --limit 50                 # Show up to 50 per source
```

---

## update

Sync package indexes and update installed packages.

```
zl update [package] [options]
```

### Options

| Flag | Description |
|------|-------------|
| `--from <source>` | Update only from a specific source |

### Behavior

1. Syncs all plugin databases (downloads latest indexes)
2. If a package name is given, updates only that package
3. If no package name, updates all explicitly installed packages
4. Skips pinned packages (with notification)
5. For each outdated package: removes old version, installs new version

### Examples

```bash
zl update                            # Update everything
zl update firefox                    # Update just firefox
zl update --from pacman              # Update only pacman packages
```

---

## upgrade

Check for and apply all available upgrades.

```
zl upgrade [options]
```

### Options

| Flag | Description |
|------|-------------|
| `--from <source>` | Only upgrade from a specific source |
| `--check` | Check-only mode (show upgrades, don't apply) |

### Output

```
Available upgrades (2):
  firefox 120.0 -> 121.0 (from pacman)
  bash 5.1 -> 5.2 (from pacman)

Total: 2 upgrade(s), 45.3 MB estimated

1 package(s) already up to date.
3 pinned package(s) skipped.

Proceed with upgrade? [Y/n]
```

### Examples

```bash
zl upgrade                           # Upgrade all
zl upgrade --check                   # Preview only
zl upgrade --from apt                # Upgrade APT packages only
```

---

## list

List installed packages.

```
zl list [options]
```

### Options

| Flag | Description |
|------|-------------|
| `--explicit` | Show only explicitly installed packages |
| `--deps` | Show only dependency packages |
| `--orphans` | Show orphaned dependencies |

### Output

```
Name                           Version          Source         Files Status
─────────────────────────────────────────────────────────────────────────
firefox                        120.0            pacman/extra     157 [explicit]
bash                           5.1              pacman/core       45 [explicit, pinned]
libfoo                         1.2              apt               23 [dep]

3 package(s) listed.
```

---

## info

Show detailed information about an installed package.

```
zl info <package>
```

### Output

```
Name:         firefox
Version:      120.0
Source:       pacman/extra
Status:       explicitly installed [PINNED]
Installed:    2024-02-23
Files:        157

Provides:
  libnss3 -> /home/user/.local/share/zl/packages/firefox-120.0/usr/lib/libnss3.so.1

Needs libs:   libc.so.6, libx11.so.6, libxrender.so.1
Depends on:   gtk3, libfoo>=1.2
Required by:  gnome-shell

Disk usage:   250.5 MB
```

---

## run

Download, patch, and execute a package without installing it. The temporary files are cleaned up on exit.

```
zl run <package> [options] [-- <args...>]
```

### Options

| Flag | Description |
|------|-------------|
| `--from <source>` | Use a specific source |
| `--version <ver>` | Use a specific version |

Arguments after `--` are passed to the executed binary.

### Behavior

1. Queries sources and downloads the package to a temp directory
2. Extracts and patches ELF binaries
3. Finds the main executable (exact name match, or first executable)
4. Runs it with combined `LD_LIBRARY_PATH` (package libs + ZL libs + system libs)
5. Exits with the same exit code as the program
6. Deletes the temp directory

### Examples

```bash
zl run ripgrep -- --help               # Run ripgrep with --help
zl run sharkdp/bat --from github -- README.md
zl run firefox --from pacman
```

---

## sources

Manage which package sources ZL uses.

```
zl sources <subcommand>
```

### Subcommands

| Subcommand | Description |
|------------|-------------|
| `list` | Show all sources and their status |
| `enable <names...>` | Enable specific sources |
| `disable <names...>` | Disable specific sources |
| `only <names...>` | Enable ONLY these sources, disable all others |
| `reset` | Remove source filter, enable all sources |

### Examples

```bash
zl sources list
zl sources enable pacman aur
zl sources disable snap flatpak
zl sources only pacman aur github
zl sources reset
```

See [[Package Sources]] for detailed documentation.

---

## switch

Activate a different version of a multi-version package.

```
zl switch <package> <version>
```

### Behavior

When you have multiple versions installed (e.g., Python 3.11 and 3.12), `switch` updates the symlinks in `bin/` and `lib/` to point to the chosen version.

### Examples

```bash
zl install python --version 3.11
zl install python --version 3.12
zl switch python 3.12                 # Activate 3.12
zl switch python 3.11                 # Switch back to 3.11
```

---

## pin / unpin

Prevent (or allow) a package from being updated.

```
zl pin <package>
zl unpin <package>
```

Pinned packages are skipped by `zl update` and `zl upgrade`. They show `[pinned]` in `zl list` output.

### Examples

```bash
zl pin firefox                         # Lock at current version
zl unpin firefox                       # Allow updates again
```

---

## env

Create and manage isolated package environments.

```
zl env <subcommand>
```

### Subcommands

| Subcommand | Description |
|------------|-------------|
| `shell` | Enter a temporary environment (deleted on exit) |
| `shell <name>` | Enter or create a named environment (persists) |
| `list` | List existing named environments |
| `delete <name>` | Delete a named environment |

### Behavior

Environments create an isolated ZL root directory. Inside the environment shell:
- `PATH` and `LD_LIBRARY_PATH` include the environment's `bin/` and `lib/`
- Packages installed with `zl --root $ZL_ENV_ROOT install <pkg>` go into the environment
- Temporary environments are deleted when you type `exit`
- Named environments persist across sessions

### Examples

```bash
zl env shell                           # Temporary (deleted on exit)
zl env shell myproject                 # Named (persists)
zl env list                            # List environments
zl env delete myproject                # Delete
```

---

## doctor

Run a full system health check.

```
zl doctor
```

### Checks performed

1. **Database integrity** — verifies all package records can be read
2. **Broken bin/ symlinks** — reports broken symlinks in the bin directory
3. **Broken lib/ symlinks** — reports broken symlinks in the lib directory
4. **Missing shared libraries** — checks if ELF binaries can find their dependencies
5. **Orphaned packages** — lists unused dependency packages
6. **Disk usage** — total package and cache size
7. **System profile** — architecture, libc, layout, interpreter, library directories

### Output

```
ZL Doctor — System Diagnostics

  Checking database... OK (42 packages)
  Checking bin/ symlinks... OK
  Checking lib/ symlinks... WARN (2 broken)
    -> ~/.local/share/zl/bin/missing-binary
    -> ~/.local/share/zl/bin/old-binary
  Checking shared library deps... OK
  Checking for orphans... INFO (1 orphaned)
    - libold-1.0
    hint: remove with `zl remove <pkg> --cascade` or reinstall as explicit
  Computing disk usage... 42 packages, 2.5 GB cache
  System: x86_64 glibc
  Layout: MergedUsr (10 lib dirs)
  Interpreter: /lib64/ld-linux-x86-64.so.2

~ 2 warning(s) found. Consider cleaning up.
```

---

## why

Show why a package is installed (trace the dependency chain).

```
zl why <package>
```

### Output examples

**Explicitly installed:**
```
firefox-120.0 was explicitly installed by the user.
```

**Installed as dependency:**
```
libfoo-1.2 is installed as a dependency.

  -> firefox-120.0 (explicitly installed)
```

**Orphan:**
```
libold-1.0 is installed as a dependency.

  No reverse dependency found — this may be an orphan.
  hint: remove it with `zl remove libold`
```

---

## size

Show disk usage per package or for a specific package.

```
zl size [package] [options]
```

### Options

| Flag | Description |
|------|-------------|
| `--sort` | Sort packages by size (largest first) |

### All packages

```
Package                        Version         Size      Files
─────────────────────────────────────────────────────────────
firefox                        120.0        250.5 MB       157
python3                        3.11         150.2 MB       892
bash                           5.1           12.3 MB        45
─────────────────────────────────────────────────────────────
Total                          3 packages   412.8 MB      1094
```

### Single package (detailed)

```bash
zl size firefox
```

```
firefox-120.0 — 250.5 MB (157 files)

  Largest files:
    157.2 MB  usr/lib/firefox/libxul.so
     42.1 MB  usr/lib/firefox/browser/omni.ja
    ... (top 20)

  Shared libraries provided:
      1.2 MB  libnss3.so.1

  Dependencies (3):
     12.3 MB  bash-5.1
    150.2 MB  python3-3.11
      5.1 MB  libfoo-1.2

  Total with deps: 417.9 MB
```

---

## diff

Show what would change if a package is updated.

```
zl diff <package>
```

### Output

```
firefox [pacman]

  Version: 120.0 -> 121.0

  New dependencies:
    + libfoo>=1.2

  Removed dependencies:
    - libold<=1.0

  Current size: 250.5 MB (157 files)
  New size: 252.1 MB (+1.6 MB)

  hint: run `zl update firefox` to apply this update
```

---

## audit

Check installed packages for known vulnerabilities using the OSV.dev API.

```
zl audit [package]
```

### Output (clean)

```
Auditing 42 package(s) against OSV.dev...

No known vulnerabilities found.
```

### Output (issues found)

```
Auditing 42 package(s) against OSV.dev...

  ! openssl-3.0.0 — 2 vulnerability(ies)
      CVE-2024-1234 [CVSS:8.5] Heap buffer overflow
      CVE-2024-5678 [CVSS:7.2] Use-after-free in SSL handshake

! Found 2 vulnerability(ies) in 1 package(s).
  hint: update affected packages with `zl update <package>`
```

---

## history

View and undo past operations.

```
zl history <subcommand>
```

### Subcommands

| Subcommand | Description |
|------------|-------------|
| `list` | Show operation history |
| `rollback [N]` | Undo the last N operations (default: 1) |

### History list

```
Date                 Action     Packages
──────────────────────────────────────────────────────
2024-02-23 14:32     Install    firefox-120.0, bash-5.1
2024-02-23 14:01     Upgrade    python3: 3.10 -> 3.11
2024-02-23 13:45     Remove     libold-1.0
```

### Rollback

```bash
zl history rollback                    # Undo last operation
zl history rollback 3                  # Undo last 3 operations
```

**Rollback behavior:**
- **Install**: Removes the installed packages
- **Remove**: Cannot restore (files are deleted). Shows reinstall hint.
- **Upgrade**: Cannot restore old version (not cached). Shows version hint.

---

## cache

Manage the download cache.

```
zl cache <subcommand>
```

### Subcommands

| Subcommand | Description |
|------------|-------------|
| `list` | Show cached downloads and sizes |
| `clean` | Delete all cached files |
| `dedup` | Deduplicate identical shared libraries using hardlinks |

### Examples

```bash
zl cache list                          # Show cache contents
zl cache clean                         # Free disk space
zl cache dedup                         # Deduplicate shared libraries
```

**Cache dedup** scans all installed packages for duplicate `.so` files (by SHA256 hash) and replaces duplicates with hardlinks, saving disk space.

---

## export / import

Export and import package lockfiles.

```
zl export [filename]
zl import <filename>
```

### Export

Exports all installed packages to a JSON lockfile:

```bash
zl export                              # Creates zl-lock.json
zl export mypackages.json              # Custom filename
```

### Import

Shows which packages from the lockfile need to be installed:

```bash
zl import zl-lock.json
```

```
Packages to install from lockfile:
  firefox 120.0 (from pacman)
  bash 5.1 (from pacman) [dep]

To install these packages, run:
  zl install firefox --from pacman --version 120.0
```

---

## completions

Generate shell completions.

```
zl completions <shell>
```

### Shells

- `bash`
- `zsh`
- `fish`

### Setup

```bash
# Bash — add to ~/.bashrc
eval "$(zl completions bash)"

# Zsh — add to ~/.zshrc
eval "$(zl completions zsh)"

# Fish — run once
zl completions fish > ~/.config/fish/completions/zl.fish
```

---

## self-update

Update ZL to the latest version.

```
zl self-update
```

Downloads the latest release binary from GitHub and replaces the current binary in-place.
