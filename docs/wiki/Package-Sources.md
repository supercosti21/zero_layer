# Package Sources

Zero Layer supports 13 package sources. Each source is a plugin that handles searching, resolving, downloading, and extracting packages from its respective ecosystem. This page documents every source in detail.

---

## Table of Contents

- [Source Management](#source-management)
- [Pacman (Arch Linux)](#pacman-arch-linux)
- [AUR (Arch User Repository)](#aur-arch-user-repository)
- [APT (Debian/Ubuntu)](#apt-debianubuntu)
- [DNF (Fedora/RHEL)](#dnf-fedorarhel)
- [Zypper (openSUSE/SLES)](#zypper-opensusesles)
- [APK (Alpine Linux)](#apk-alpine-linux)
- [XBPS (Void Linux)](#xbps-void-linux)
- [Portage (Gentoo)](#portage-gentoo)
- [Nix (nixpkgs)](#nix-nixpkgs)
- [GitHub Releases](#github-releases)
- [Flatpak (Flathub)](#flatpak-flathub)
- [Snap (Snapcraft)](#snap-snapcraft)
- [AppImage (AppImageHub)](#appimage-appimagehub)
- [Source Comparison Table](#source-comparison-table)

---

## Source Management

### First-run wizard

On first run, ZL detects your distro from `/etc/os-release` and suggests appropriate sources. You can toggle each source on or off with an interactive multi-select menu.

### The `zl sources` command

Manage which sources ZL uses at any time:

```bash
zl sources list                       # Show all sources and their status
zl sources enable pacman aur          # Enable specific sources
zl sources disable snap flatpak       # Disable specific sources
zl sources only pacman aur github     # Enable ONLY these, disable everything else
zl sources reset                      # Remove filter, enable all sources
```

**`zl sources list` output:**
```
Package Sources

  pacman        enabled  (loaded)
  aur           enabled  (loaded)
  apt           disabled
  github        enabled  (loaded)
  dnf           disabled
  zypper        disabled
  apk           disabled
  xbps          disabled
  portage       disabled
  nix           disabled
  flatpak       enabled  (loaded)
  snap          disabled
  appimage      enabled  (loaded)

  Active filter: pacman, aur, github, flatpak, appimage
```

- **enabled** (green) — ZL will query this source during search/install
- **disabled** (dim) — ZL will skip this source
- **(loaded)** — The plugin is initialized and ready (matches your system)

### Per-command source filtering

Override the global source list for a single command:

```bash
zl search vim --from pacman           # Search only pacman
zl search vim --from pacman,apt       # Search pacman and apt
zl install vim --from apt             # Install from apt specifically
```

The `--from` flag accepts comma-separated source names.

### Config file

Sources can also be configured in `~/.config/zl/config.toml`:

```toml
[general]
sources = ["pacman", "aur", "github", "flatpak", "appimage"]
```

When `sources` is set, only listed sources are loaded at startup. When omitted (or set to `null`), all sources are enabled.

Individual sources can also be disabled:

```toml
[plugins.snap]
enabled = false
```

---

## Pacman (Arch Linux)

**Plugin name:** `pacman`
**Package format:** `.pkg.tar.zst`, `.pkg.tar.xz`
**Database type:** Cached (synced via `zl update`)

### Overview

Installs packages from Arch Linux official repositories. Downloads and parses the pacman database files (`core.db`, `extra.db`) to search and resolve packages.

### Usage

```bash
zl update --from pacman              # Sync the database (required before first use)
zl search firefox --from pacman      # Search
zl install firefox --from pacman     # Install
```

### Configuration

```toml
[plugins.pacman]
mirrorlist = "/etc/pacman.d/mirrorlist"   # Path to mirrorlist (auto-detected)
arch = "x86_64"                            # Architecture (auto-detected)
repos = ["core", "extra"]                  # Repositories to use
```

### How it works

1. **Sync**: Downloads `{repo}.db` from the first working mirror. The DB is a tar.gz archive containing package metadata (desc files).
2. **Search**: Case-insensitive search across cached package names and descriptions.
3. **Resolve**: Exact name match first. If not found, checks `provides` fields for virtual packages (e.g., `sh` provided by `bash`).
4. **Download**: HTTP download of `.pkg.tar.zst` from the mirror.
5. **Extract**: Decompresses zstd, then extracts tar archive. Classifies files as ELF binaries, scripts, or data.

### Mirror detection

ZL reads mirrors from `/etc/pacman.d/mirrorlist` (or the configured path). It uses the first uncommented `Server = ...` line. If no mirrorlist is found, it falls back to `https://geo.mirror.pkgbuild.com`.

### Database entries

Each package in the DB provides:
- Name, version, description, architecture
- Dependencies (`depends`), optional deps (`optdepends`)
- Provides (virtual packages)
- Conflicts
- Installed size, compressed size
- Download URL (relative to mirror)

---

## AUR (Arch User Repository)

**Plugin name:** `aur`
**Package format:** `.pkg.tar.zst` (built from source)
**Database type:** Live API queries

### Overview

Installs packages from the Arch User Repository. Uses the AUR RPC API v5 for search and metadata, then clones the PKGBUILD and builds with `makepkg`.

### Usage

```bash
zl search yay --from aur              # Search the AUR
zl install yay --from aur             # Build and install from AUR
```

### Requirements

Building from AUR requires:
- `git` — for cloning PKGBUILDs
- `makepkg` — for building packages
- `base-devel` — build tools (gcc, make, etc.)

### Configuration

No specific configuration needed. The AUR plugin uses the public AUR RPC API.

### How it works

1. **Search**: Queries `https://aur.archlinux.org/rpc/v5/search/{query}?by=name-desc`. Automatically discovers binary variants (`-bin`, `-appimage`, `-prebuilt` suffixes) and tags them `[binary]`.
2. **Resolve**: Queries `https://aur.archlinux.org/rpc/v5/info/{name}` for exact package metadata.
3. **Download**: Runs `git clone --depth=1 https://aur.archlinux.org/{package_base}.git` into a temp directory, then `makepkg --syncdeps --force --noconfirm --noprogressbar` to build.
4. **Extract**: The built `.pkg.tar.zst` is extracted like a pacman package.

### Binary variant discovery

When you search for `firefox`, ZL also checks for:
- `firefox-bin` — precompiled binary
- `firefox-appimage` — AppImage wrapper
- `firefox-prebuilt` — prebuilt variant

These are tagged with `[binary]` in search results so you can choose a precompiled version instead of building from source.

### Common build errors

- **Missing base-devel**: Install `base-devel` group on Arch first
- **PGP key errors**: Import the required key with `gpg --recv-keys <KEYID>`
- **Missing dependencies**: AUR packages may depend on other AUR packages; ZL resolves these recursively

---

## APT (Debian/Ubuntu)

**Plugin name:** `apt`
**Package format:** `.deb` (ar archive containing tar)
**Database type:** Cached index (synced via `zl update`)

### Overview

Installs packages from Debian and Ubuntu repositories. Downloads and parses `Packages.gz` index files.

### Usage

```bash
zl update --from apt                   # Sync the package index (required)
zl search vim --from apt               # Search
zl install vim --from apt              # Install
```

### Configuration

```toml
[plugins.apt]
mirror = "http://archive.ubuntu.com/ubuntu"   # Mirror URL
suite = "noble"                                 # Release codename
components = ["main", "universe"]               # Repository components
arch = "amd64"                                  # Architecture (auto-detected)
```

**Common suites:**
- **Ubuntu**: `noble` (24.04), `jammy` (22.04), `focal` (20.04)
- **Debian**: `bookworm` (12), `bullseye` (11), `trixie` (13)

**Common mirrors:**
- Ubuntu: `http://archive.ubuntu.com/ubuntu`
- Debian: `http://deb.debian.org/debian`

### How it works

1. **Sync**: Downloads `{mirror}/dists/{suite}/{component}/binary-{arch}/Packages.gz` for each component. Parses the RFC822-style format into package entries.
2. **Search**: Case-insensitive search across cached names and descriptions.
3. **Resolve**: Exact package name lookup with optional version matching.
4. **Download**: Direct `.deb` download from the mirror.
5. **Extract**: `.deb` files are `ar` archives containing `data.tar.{gz,xz,zst}`. ZL extracts the ar, then the inner tar.

### Architecture mapping

| Host arch | APT arch |
|-----------|----------|
| x86_64 | amd64 |
| aarch64 | arm64 |
| arm | armhf |
| i686 | i386 |
| riscv64 | riscv64 |
| powerpc64 | ppc64el |
| s390x | s390x |

---

## DNF (Fedora/RHEL)

**Plugin name:** `dnf`
**Package format:** `.rpm`
**Database type:** Cached XML (synced via `zl update`)

### Overview

Installs packages from Fedora and RHEL-family repositories. Downloads and parses `primary.xml.gz` repodata.

### Usage

```bash
zl update --from dnf                   # Sync the repodata
zl search gcc --from dnf               # Search
zl install gcc --from dnf              # Install
```

### Configuration

```toml
[plugins.dnf]
mirror = "https://dl.fedoraproject.org/pub/fedora/linux"
release = "40"                          # Fedora release number
repos = ["fedora", "updates"]           # Repositories
arch = "x86_64"                         # Architecture (auto-detected)
```

### How it works

1. **Sync**: Downloads `primary.xml.gz` from each repo:
   - `fedora`: `{mirror}/releases/{release}/Everything/{arch}/os/repodata/primary.xml.gz`
   - `updates`: `{mirror}/updates/{release}/Everything/{arch}/repodata/primary.xml.gz`
2. **Search**: In-memory search of parsed `RpmEntry` list (name + summary).
3. **Resolve**: Exact name match with version matching.
4. **Download**: HTTP download of `.rpm` from the repo.
5. **Extract**: RPM extraction — skips lead (96 bytes) and headers, then extracts CPIO payload. Auto-detects compression (gzip, xz, zstd, bzip2).

### RPM extraction details

RPM files have this structure:
```
[Lead: 96 bytes] [Signature Header] [Main Header] [Compressed CPIO Payload]
```

The CPIO payload contains the actual files. ZL auto-detects the compression format from magic bytes:
- `1f 8b` → gzip
- `fd 37 7a 58 5a 00` → xz
- `28 b5 2f fd` → zstd
- `42 5a` → bzip2

---

## Zypper (openSUSE/SLES)

**Plugin name:** `zypper`
**Package format:** `.rpm`
**Database type:** Cached XML (synced via `zl update`)

### Overview

Installs packages from openSUSE and SLES repositories. Uses the same RPM extraction as DNF.

### Usage

```bash
zl update --from zypper                # Sync the repodata
zl search vim --from zypper            # Search
zl install vim --from zypper           # Install
```

### Configuration

```toml
[plugins.zypper]
mirror = "https://download.opensuse.org"
release = "tumbleweed"                  # "tumbleweed" or "15.5" for Leap
repos = ["oss", "update"]              # Repositories
arch = "x86_64"                         # Architecture (auto-detected)
```

### URL patterns

Different URL patterns for Tumbleweed vs Leap:

- **Tumbleweed**: `{mirror}/tumbleweed/repo/{repo}/repodata/primary.xml.gz`
- **Leap**: `{mirror}/distribution/leap/{release}/repo/{repo}/repodata/primary.xml.gz`

---

## APK (Alpine Linux)

**Plugin name:** `apk`
**Package format:** `.apk` (tar.gz)
**Database type:** Cached index (synced via `zl update`)

### Overview

Installs packages from Alpine Linux repositories. Parses the `APKINDEX` format.

### Usage

```bash
zl update --from apk                   # Sync the index
zl search curl --from apk              # Search
zl install curl --from apk             # Install
```

### Configuration

```toml
[plugins.apk]
mirror = "https://dl-cdn.alpinelinux.org/alpine"
branch = "v3.20"                        # Alpine branch
repos = ["main", "community"]           # Repositories
arch = "x86_64"                         # Architecture (auto-detected)
```

### How it works

1. **Sync**: Downloads `{mirror}/{branch}/{repo}/{arch}/APKINDEX.tar.gz`. Extracts the `APKINDEX` file from the tar.gz and parses the key-value format.
2. **Search**: Case-insensitive search in cached entries.
3. **Download**: `{mirror}/{branch}/{repo}/{arch}/{name}-{version}.apk`
4. **Extract**: APK files are tar.gz archives. ZL extracts the inner tar.

### APKINDEX format

The `APKINDEX` file uses a simple key-value format with entries separated by blank lines:

```
P:curl
V:8.5.0-r0
A:x86_64
T:URL retrieval utility and library
I:245760
D:ca-certificates libcurl
p:cmd:curl

P:vim
V:9.0.2100-r0
...
```

Key fields:
- `P:` — Package name
- `V:` — Version
- `A:` — Architecture
- `T:` — Description
- `I:` — Installed size (bytes)
- `D:` — Dependencies (space-separated)
- `p:` — Provides (space-separated)

---

## XBPS (Void Linux)

**Plugin name:** `xbps`
**Package format:** `.xbps` (tar.zst)
**Database type:** Cached

### Overview

Installs packages from Void Linux repositories.

### Usage

```bash
zl update --from xbps                  # Sync the repodata
zl search curl --from xbps             # Search
zl install curl --from xbps            # Install
```

### Configuration

```toml
[plugins.xbps]
mirror = "https://repo-default.voidlinux.org"
arch = "x86_64"                         # Architecture (auto-detected)
repos = ["current", "current/nonfree"]  # Repositories
```

### How it works

1. **Sync**: Downloads `{mirror}/{repo}/{arch}-repodata` for each repo.
2. **Extract**: `.xbps` files are tar archives compressed with zstd.

> **Note**: The repodata parser is currently simplified. XBPS repodata uses a binary plist format. Full plist parsing is planned for a future release.

---

## Portage (Gentoo)

**Plugin name:** `portage`
**Package format:** `.tbz2` (tar+bzip2) or `.gpkg.tar`
**Database type:** Cached index (synced via `zl update`)

### Overview

Installs binary packages from a Gentoo binhost. Parses the `Packages` index file.

### Usage

```bash
zl update --from portage               # Sync the index
zl search vim --from portage           # Search
zl install vim --from portage          # Install
```

### Configuration

```toml
[plugins.portage]
binhost = "https://distfiles.gentoo.org/releases/amd64/binpackages/17.1/x86-64"
arch = "amd64"                          # Architecture
```

### How it works

1. **Sync**: Downloads `{binhost}/Packages` and parses the key-value block format.
2. **Search**: Searches by name, CPV (category/package-version), and description.
3. **Extract**: Handles both old (`.tbz2`) and new (`.gpkg.tar`) Gentoo binary formats.

### Packages index format

```
CPV: dev-libs/openssl-3.0.12
DESC: Toolkit for SSL and TLS
SIZE: 4521984
PATH: dev-libs/openssl-3.0.12.tbz2
SHA256: abc123...
RDEPEND: sys-libs/zlib dev-libs/libffi
```

### CPV format

Gentoo uses `category/name-version` (CPV) format. ZL parses this to extract the package name and version separately.

---

## Nix (nixpkgs)

**Plugin name:** `nix`
**Package format:** `.nar` (Nix ARchive, optionally compressed with xz or zstd)
**Database type:** Live API queries

### Overview

Searches packages from nixpkgs using the search.nixos.org API. Downloads use the Nix binary cache.

### Usage

```bash
zl search ripgrep --from nix           # Search nixpkgs
```

### Configuration

```toml
[plugins.nix]
channel = "nixos-unstable"              # Channel (nixos-unstable, nixos-24.05, etc.)
cache_url = "https://cache.nixos.org"   # Binary cache URL
```

### How it works

1. **Search**: POST to `https://search.nixos.org/backend/latest-43-{channel}/_search` with ElasticSearch `multi_match` query. Searches across `package_pname`, `package_attr_name`, and `package_description`.
2. **Resolve**: Searches for exact name match.
3. **Download**: Currently limited — full NAR download from the binary cache requires narinfo resolution. ZL suggests using `nix profile install nixpkgs#{name}` as a workaround.

### NAR format

NAR (Nix ARchive) is Nix's deterministic archive format. ZL includes a complete NAR parser that handles:
- Regular files (with optional executable flag)
- Directories (recursive)
- Symlinks
- Compression: `.nar.xz`, `.nar.zst`, or raw `.nar`

NAR string encoding: 8-byte little-endian length + content + padding to 8-byte boundary.

> **Note**: Direct NAR download from cache.nixos.org is not yet fully implemented. The NAR parser itself is complete and functional.

---

## GitHub Releases

**Plugin name:** `github`
**Package format:** tar.gz, tar.xz, tar.zst, zip, AppImage, bare binary
**Database type:** Live API queries

### Overview

Installs packages from GitHub Releases. Supports any public repository that publishes release assets.

### Usage

```bash
zl search bat --from github                    # Search repos
zl install sharkdp/bat --from github           # Install (owner/repo format)
zl install BurntSushi/ripgrep --from github    # Another example
zl install cli/cli --from github               # GitHub CLI itself
```

### Configuration

```toml
[plugins.github]
token = "ghp_..."   # Optional: personal access token to avoid rate limiting
```

Without a token, GitHub allows 60 API requests per hour. With a token, 5000 per hour.

### How it works

1. **Search**: `GET /search/repositories?q={query}+language:any&sort=stars&per_page=10`. Returns the top 10 most-starred matching repos, then fetches the latest release for each.
2. **Resolve**: `GET /repos/{owner}/{repo}/releases/latest` (or `/tags/{tag}` for specific versions).
3. **Download**: Direct download of the selected release asset.
4. **Extract**: Auto-detects format from filename/content. Supports tar.gz, tar.xz, tar.zst, zip, AppImage, and bare binaries.

### Smart asset selection

When a release has multiple assets (common), ZL uses a scoring system:

**Excluded assets** (score = skip):
- Windows assets (`.exe`, `.msi`, `windows`, `win32`, `win64`)
- macOS assets (`darwin`, `macos`, `.dmg`)
- Package formats handled by other plugins (`.deb`, `.rpm`, `.apk`)
- Checksum files (`.sha256`, `.sha512`, `.md5`)
- Signature files (`.sig`, `.asc`)

**Scoring** (lower = better):
- Contains "linux" explicitly: -10
- musl build: -5
- tar.gz: -4
- tar.xz: -3
- tar.zst: -2
- zip: -1
- Bare binary: 0

This means ZL prefers: musl+linux+tar.gz > linux+tar.gz > bare binary > zip.

### Rate limiting

If you hit GitHub's rate limit (HTTP 429), ZL will tell you:
```
GitHub API rate limit exceeded. Add a personal access token to config:
  [plugins.github]
  token = "ghp_..."
```

---

## Flatpak (Flathub)

**Plugin name:** `flatpak`
**Package format:** Flatpak bundle (managed by flatpak runtime)
**Database type:** Live API queries

### Overview

Searches and installs Flatpak applications from Flathub.

### Usage

```bash
zl search firefox --from flatpak       # Search Flathub
zl install firefox --from flatpak      # Install
```

### Configuration

```toml
[plugins.flatpak]
remote = "flathub"                      # Flatpak remote name
```

### Requirements

- `flatpak` CLI must be installed on the system
- Flathub remote must be configured (`flatpak remote-add --if-not-exists flathub https://flathub.org/repo/flathub.flatpakrepo`)

### How it works

1. **Search**: `GET https://flathub.org/api/v2/search?q={query}`
2. **Resolve**: `GET https://flathub.org/api/v2/appstream/{app_id}` for app details.
3. **Install**: Delegates to `flatpak install --noninteractive {remote} {app_id}`.

> **Note**: Flatpak packages are managed by the Flatpak runtime, not directly by ZL's ELF patching system.

---

## Snap (Snapcraft)

**Plugin name:** `snap`
**Package format:** `.snap` (SquashFS image)
**Database type:** Live API queries

### Overview

Searches and installs Snap packages from the Snapcraft Store.

### Usage

```bash
zl search firefox --from snap          # Search the Snap Store
zl install firefox --from snap         # Install
```

### Configuration

```toml
[plugins.snap]
channel = "stable"                      # Channel: stable, candidate, beta, edge
```

### Requirements

- `unsquashfs` (from `squashfs-tools` package) — needed to extract `.snap` files

### How it works

1. **Search**: `GET https://api.snapcraft.io/v2/snaps/find?q={query}&fields=title,summary,publisher` with custom headers `Snap-Device-Series: 16` and `Snap-Device-Architecture: {arch}`.
2. **Resolve**: `GET https://api.snapcraft.io/v2/snaps/info/{name}` — returns channel map with versions for each channel/architecture combination.
3. **Download**: Direct download from the URL in the channel map.
4. **Extract**: Uses `unsquashfs` to extract the SquashFS image.

### Architecture mapping

| Host arch | Snap arch |
|-----------|-----------|
| x86_64 | amd64 |
| aarch64 | arm64 |
| arm | armhf |
| i686 | i386 |

---

## AppImage (AppImageHub)

**Plugin name:** `appimage`
**Package format:** `.AppImage` (self-contained executable)
**Database type:** Cached feed (synced via `zl update`)

### Overview

Installs AppImage applications from AppImageHub.

### Usage

```bash
zl update --from appimage              # Sync the feed
zl search firefox --from appimage      # Search
zl install firefox --from appimage     # Install
```

### Configuration

No specific configuration needed.

### How it works

1. **Sync**: Downloads `https://appimage.github.io/feed.json` — a JSON file listing all AppImage applications.
2. **Search**: In-memory search across app names and descriptions.
3. **Resolve**: Case-insensitive name matching.
4. **Download**: Direct download from the link in the feed. Sets executable permission (`chmod 0755`).
5. **Install**: Places the AppImage in the package's `usr/bin/` directory. AppImages are self-contained — they include all libraries and run anywhere.

AppImages don't need ELF patching — they are designed to be portable. ZL simply manages the download, symlink, and tracking.

---

## Source Comparison Table

| Source | Search | Sync Required | Archive Format | External Tools | API |
|--------|--------|--------------|----------------|----------------|-----|
| **pacman** | Cached | Yes | tar.zst/xz | None | Mirror |
| **aur** | Live | No | tar.zst | git, makepkg | RPC v5 |
| **apt** | Cached | Yes | .deb (ar+tar) | None | Mirror |
| **dnf** | Cached | Yes | .rpm (cpio) | None | Mirror |
| **zypper** | Cached | Yes | .rpm (cpio) | None | Mirror |
| **apk** | Cached | Yes | tar.gz | None | Mirror |
| **xbps** | Cached | Yes | tar.zst | None | Mirror |
| **portage** | Cached | Yes | tbz2/gpkg | None | Binhost |
| **nix** | Live | No | .nar | None | ElasticSearch |
| **github** | Live | No | Various | None | REST API |
| **flatpak** | Live | No | Flatpak | flatpak | REST API |
| **snap** | Live | No | SquashFS | unsquashfs | REST API |
| **appimage** | Cached | Yes | AppImage | None | JSON feed |

**Legend:**
- **Cached**: Database/index is downloaded locally for fast offline search
- **Live**: Queries the remote API on each search (requires internet)
- **Sync Required**: Must run `zl update --from <source>` before first use
