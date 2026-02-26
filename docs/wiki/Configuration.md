# Configuration

ZL stores its configuration at `~/.config/zl/config.toml`. This file is optional — ZL works out of the box with auto-detected defaults. The config is created automatically after the first-run wizard, or you can create it manually.

---

## Full Config Reference

```toml
# ─── General Settings ───────────────────────────────────────────────

[general]
# Override the ZL root directory (default: ~/.local/share/zl)
# root = "/custom/zl/root"

# Auto-confirm all prompts (same as -y flag)
# auto_confirm = false

# Source filter: only load these sources. Omit to enable all.
# sources = ["pacman", "aur", "github"]

# ─── System Overrides ───────────────────────────────────────────────

[system]
# All fields are optional. ZL auto-detects everything by default.

# Override the dynamic linker path
# interpreter = "/lib64/ld-linux-x86-64.so.2"

# Add extra directories to search for shared libraries
# extra_lib_dirs = ["/opt/mylibs", "/usr/local/lib"]

# Add extra directories to search for binaries
# extra_bin_dirs = ["/opt/mybin"]

# Override the detected filesystem layout
# Possible values: "fhs", "merged_usr", "nixos", "guix", "termux", "gobolinux"
# layout = "merged_usr"

# ─── Plugin Configuration ───────────────────────────────────────────

# Each plugin can be individually enabled/disabled and configured.
# All plugin fields are optional — defaults are used when omitted.

[plugins.pacman]
# enabled = true
# mirrorlist = "/etc/pacman.d/mirrorlist"
# arch = "x86_64"
# repos = ["core", "extra"]

[plugins.aur]
# enabled = true
# No additional configuration needed

[plugins.apt]
# enabled = true
# mirror = "http://archive.ubuntu.com/ubuntu"
# suite = "noble"
# components = ["main", "universe"]
# arch = "amd64"

[plugins.dnf]
# enabled = true
# mirror = "https://dl.fedoraproject.org/pub/fedora/linux"
# release = "40"
# repos = ["fedora", "updates"]
# arch = "x86_64"

[plugins.zypper]
# enabled = true
# mirror = "https://download.opensuse.org"
# release = "tumbleweed"
# repos = ["oss", "update"]
# arch = "x86_64"

[plugins.apk]
# enabled = true
# mirror = "https://dl-cdn.alpinelinux.org/alpine"
# branch = "v3.20"
# repos = ["main", "community"]
# arch = "x86_64"

[plugins.xbps]
# enabled = true
# mirror = "https://repo-default.voidlinux.org"
# arch = "x86_64"
# repos = ["current", "current/nonfree"]

[plugins.portage]
# enabled = true
# binhost = "https://distfiles.gentoo.org/releases/amd64/binpackages/17.1/x86-64"
# arch = "amd64"

[plugins.nix]
# enabled = true
# channel = "nixos-unstable"
# cache_url = "https://cache.nixos.org"

[plugins.github]
# enabled = true
# token = "ghp_..."    # Personal access token (avoids rate limiting)

[plugins.flatpak]
# enabled = true
# remote = "flathub"

[plugins.snap]
# enabled = true
# channel = "stable"

[plugins.appimage]
# enabled = true
# No additional configuration needed
```

---

## Section Details

### [general]

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `root` | string | `~/.local/share/zl` | ZL root directory for packages, DB, cache |
| `auto_confirm` | bool | `false` | Skip all confirmation prompts |
| `sources` | string[] | all | Whitelist of source names to load |

**Source filtering:**

When `sources` is set, only listed sources are loaded at startup:

```toml
[general]
sources = ["pacman", "aur", "github"]
```

When `sources` is omitted, all 13 sources are available. You can also manage this interactively with `zl sources`.

### [system]

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `interpreter` | string | auto-detected | Path to dynamic linker (ld-linux.so) |
| `extra_lib_dirs` | string[] | `[]` | Extra directories to search for shared libraries |
| `extra_bin_dirs` | string[] | `[]` | Extra directories to search for binaries |
| `layout` | string | auto-detected | Filesystem layout override |

**Interpreter detection:**

By default, ZL reads the `PT_INTERP` segment from `/bin/sh` to find the system's dynamic linker. This works on any Linux system. Override this only if your system has a non-standard setup.

**Filesystem layouts:**

| Layout | Description |
|--------|-------------|
| `fhs` | Traditional `/usr`, `/lib`, `/lib64` separation |
| `merged_usr` | `/bin` → `/usr/bin`, `/lib` → `/usr/lib` (modern default) |
| `nixos` | Nix store layout (`/nix/store/...`) |
| `guix` | GNU Guix layout |
| `termux` | Android Termux (`/data/data/com.termux/...`) |
| `gobolinux` | GoboLinux (`/Programs/...`) |

### [plugins.*]

Every plugin section supports:

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `enabled` | bool | `true` | Enable or disable this plugin |

Plus plugin-specific keys documented in [[Package Sources]].

---

## Config File Location

The config file lives at:

```
~/.config/zl/config.toml
```

Or more precisely: `$XDG_CONFIG_HOME/zl/config.toml`

If the file doesn't exist, ZL uses auto-detected defaults. The first-run wizard creates this file for you.

### Editing the config

The config is saved automatically by:
- The first-run wizard
- `zl sources enable/disable/only/reset`

You can also edit it manually with any text editor:

```bash
nano ~/.config/zl/config.toml
vim ~/.config/zl/config.toml
```

Changes take effect on the next `zl` invocation — there's no daemon to restart.

---

## Example Configurations

### Arch Linux user

```toml
[general]
sources = ["pacman", "aur", "github", "flatpak", "appimage"]

[plugins.pacman]
repos = ["core", "extra", "multilib"]
```

### Ubuntu user

```toml
[general]
sources = ["apt", "github", "flatpak", "snap", "appimage"]

[plugins.apt]
mirror = "http://archive.ubuntu.com/ubuntu"
suite = "noble"
components = ["main", "universe", "multiverse"]
```

### Fedora user

```toml
[general]
sources = ["dnf", "github", "flatpak", "appimage"]

[plugins.dnf]
release = "40"
repos = ["fedora", "updates"]
```

### Multi-distro (maximum sources)

```toml
# Enable everything — useful for cross-distro package hunting
[general]
# Don't set sources — all 13 are enabled by default

[plugins.github]
token = "ghp_your_token_here"
```

### Minimal (GitHub only)

```toml
[general]
sources = ["github"]

[plugins.github]
token = "ghp_your_token_here"
```

---

## Environment Variables

ZL respects these environment variables:

| Variable | Description |
|----------|-------------|
| `XDG_DATA_HOME` | Base directory for ZL data (default: `~/.local/share`) |
| `XDG_CONFIG_HOME` | Base directory for config (default: `~/.config`) |
| `ZL_ENV_ROOT` | Set inside `zl env shell` — the environment's root directory |
| `LD_LIBRARY_PATH` | ZL adds its lib directory here for executed binaries |

---

## Next Steps

- [[Package Sources]] — Detailed documentation for each source plugin
- [[Commands Reference]] — All commands and flags
- [[Architecture]] — How ZL uses the config internally
