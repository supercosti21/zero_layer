# Troubleshooting

Common issues and their solutions when using Zero Layer.

---

## Table of Contents

- [Installation Issues](#installation-issues)
- [Package Install Issues](#package-install-issues)
- [Source-Specific Issues](#source-specific-issues)
- [Runtime Issues](#runtime-issues)
- [Database Issues](#database-issues)
- [Frequently Asked Questions](#frequently-asked-questions)

---

## Installation Issues

### ZL binary not found after install

**Symptom**: `zl: command not found`

**Solution**: Make sure the binary is in your PATH:

```bash
# Check where zl is
which zl
ls -la /usr/local/bin/zl
ls -la ~/.local/bin/zl

# If installed to ~/.local/bin, add to PATH:
echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.bashrc
source ~/.bashrc
```

### Installed packages not found

**Symptom**: You installed a package but the command isn't available.

**Solution**: Add ZL's bin directory to your PATH:

```bash
echo 'export PATH="$HOME/.local/share/zl/bin:$PATH"' >> ~/.bashrc
source ~/.bashrc
```

Verify with:
```bash
ls ~/.local/share/zl/bin/
```

### Permission denied during install

**Symptom**: `Permission denied` when installing ZL itself.

**Solution**: Either use sudo for system-wide install, or install to user directory:

```bash
# System-wide (requires sudo)
sudo mv zl /usr/local/bin/

# User-local (no sudo)
mkdir -p ~/.local/bin
mv zl ~/.local/bin/
```

ZL itself does NOT need root permissions. All packages are installed to `~/.local/share/zl/` in userspace.

---

## Package Install Issues

### "Package not found in any source"

**Possible causes:**
1. The package name is wrong
2. The source hasn't been synced yet
3. The source is disabled

**Solutions:**

```bash
# Search for the correct name
zl search <partial-name>

# Sync the source database
zl update --from pacman
zl update --from apt

# Check which sources are enabled
zl sources list

# Enable a source
zl sources enable apt
```

### Checksum mismatch

**Symptom**: `Checksum mismatch for package.pkg.tar.zst`

**Causes:**
- Corrupted download (network issue)
- Mirror is out of sync
- Stale local cache

**Solutions:**

```bash
# Clear cache and retry
zl cache clean
zl install <package> --from <source>

# If using pacman, try syncing first
zl update --from pacman

# Last resort: skip verification (NOT recommended)
zl --skip-verify install <package>
```

### Dependency resolution fails

**Symptom**: `Could not resolve dependency: libfoo`

**Solutions:**

```bash
# Enable more sources (cross-source resolution)
zl sources enable apt github

# Search for the dependency manually
zl search libfoo

# Install the dependency first
zl install libfoo --from apt

# Then retry the original install
zl install <package> --from pacman
```

### Conflict detected

**Symptom**: `Conflict: file ownership — /usr/bin/foo is owned by package-a`

**Solutions:**

```bash
# See what's conflicting
zl info package-a

# Remove the conflicting package first
zl remove package-a

# Then install
zl install package-b
```

### ELF architecture mismatch

**Symptom**: `Warning: ELF architecture mismatch (aarch64 vs x86_64)`

**Cause**: You're trying to install a package built for a different CPU architecture.

**Solution**: Make sure you're installing the correct architecture. For GitHub releases:

```bash
# Search for the right variant
zl search <package> --from github
# Look for x86_64 / amd64 / linux-amd64 variants
```

### Binary not found after install

**Symptom**: Package installed successfully but `command not found`.

**Solutions:**

```bash
# Check what files the package installed
ls ~/.local/share/zl/packages/<name>-<version>/

# Check if symlinks were created
ls -la ~/.local/share/zl/bin/

# Check PATH
echo $PATH | tr ':' '\n' | grep zl

# The binary might have a different name than expected
zl info <package>
```

---

## Source-Specific Issues

### Pacman: "Failed to sync database"

**Cause**: No mirrors configured or mirrors are down.

**Solutions:**

```bash
# Check if mirrorlist exists
cat /etc/pacman.d/mirrorlist

# Configure a mirror in ZL config
cat >> ~/.config/zl/config.toml << 'EOF'
[plugins.pacman]
mirrorlist = "/etc/pacman.d/mirrorlist"
EOF

# Or use a specific mirror
cat >> ~/.config/zl/config.toml << 'EOF'
[plugins.pacman]
mirror = "https://geo.mirror.pkgbuild.com"
EOF
```

### AUR: Build failed

**Common causes and solutions:**

```bash
# Missing build tools
sudo pacman -S base-devel git

# PGP key not imported
gpg --recv-keys <KEY_ID>    # Key ID from the error message

# Missing dependency
# The error will tell you what's missing. Install it first.
zl install <dependency> --from pacman
```

### APT: "No packages found"

**Cause**: Index not synced or wrong suite configured.

**Solutions:**

```bash
# Sync the index
zl update --from apt

# Check your config
cat ~/.config/zl/config.toml

# Verify the suite name (noble, bookworm, jammy, etc.)
cat /etc/os-release
```

### GitHub: Rate limit exceeded

**Symptom**: `HTTP 429` or `API rate limit exceeded`

**Solution**: Add a personal access token:

1. Go to https://github.com/settings/tokens
2. Create a token with no special permissions (public repo access is default)
3. Add to config:

```toml
[plugins.github]
token = "ghp_your_token_here"
```

Without a token: 60 requests/hour. With a token: 5000 requests/hour.

### DNF/Zypper: "Failed to download repodata"

**Cause**: Wrong release number or mirror URL.

**Solution**: Check and update config:

```bash
# For Fedora: check your release version
cat /etc/fedora-release

# Update config
cat >> ~/.config/zl/config.toml << 'EOF'
[plugins.dnf]
release = "40"
EOF
```

### Snap: "unsquashfs not found"

**Cause**: `squashfs-tools` package not installed.

**Solution**:

```bash
# Arch
sudo pacman -S squashfs-tools

# Ubuntu/Debian
sudo apt install squashfs-tools

# Fedora
sudo dnf install squashfs-tools
```

### Flatpak: "flatpak not found"

**Cause**: Flatpak runtime not installed.

**Solution**:

```bash
# Arch
sudo pacman -S flatpak

# Ubuntu/Debian
sudo apt install flatpak

# Fedora (usually pre-installed)
sudo dnf install flatpak

# Add Flathub remote
flatpak remote-add --if-not-exists flathub https://flathub.org/repo/flathub.flatpakrepo
```

---

## Runtime Issues

### "libfoo.so.1: cannot open shared object file"

**Cause**: A shared library is missing at runtime.

**Solutions:**

```bash
# Check what libraries are missing
zl doctor

# Search for the library
zl search libfoo

# Install it
zl install libfoo --from apt

# Or check if it's a system library
ldconfig -p | grep libfoo
```

### Installed program crashes

**Possible causes:**
1. glibc version too old (binary requires newer glibc)
2. Missing shared libraries
3. Hardcoded paths in the binary that don't exist

**Solutions:**

```bash
# Check system health
zl doctor

# Check library deps
ldd ~/.local/share/zl/packages/<name>-<version>/usr/bin/<binary>

# Try a different source (e.g., musl build from GitHub)
zl remove <package>
zl install <package> --from github
```

### Environment variables not set

Some packages need environment variables. Check if the package has any setup requirements:

```bash
# Some packages need XDG directories
export XDG_DATA_DIRS="$HOME/.local/share/zl/share:$XDG_DATA_DIRS"
```

---

## Database Issues

### "Database locked" or corruption

**Symptom**: `failed to open database` or `database is locked`

**Solutions:**

```bash
# Run doctor to check
zl doctor

# If the database is corrupted, you may need to remove and reinstall
# WARNING: This loses your package tracking
rm ~/.local/share/zl/zl.redb

# Then reinstall your packages
zl import zl-lock.json    # If you exported earlier
```

### Packages in DB but files missing

**Cause**: Files were manually deleted outside of ZL.

**Solution**:

```bash
# Doctor will detect this
zl doctor

# Remove the broken package from DB
zl remove <package>

# Reinstall
zl install <package>
```

---

## Frequently Asked Questions

### Does ZL need root/sudo?

**No.** ZL installs everything to `~/.local/share/zl/` in your home directory. It never touches system files or requires elevated privileges.

The only exception is if you want to install the `zl` binary itself to `/usr/local/bin/` — that requires sudo. But you can also install it to `~/.local/bin/` without sudo.

### Can ZL break my system package manager?

**No.** ZL manages its own directory (`~/.local/share/zl/`) completely independently of your system package manager. It doesn't modify `/usr`, `/lib`, or any system paths. Your system's `pacman`, `apt`, `dnf`, etc. are unaffected.

### Can I use ZL alongside my distro's package manager?

**Yes.** ZL is designed to complement your system package manager, not replace it. Use your system PM for core system packages, and ZL for packages from other distros or sources.

### Where does ZL install packages?

```
~/.local/share/zl/packages/<name>-<version>/
```

Executables are symlinked to `~/.local/share/zl/bin/` and shared libraries to `~/.local/share/zl/lib/`.

### How do I see what ZL has installed?

```bash
zl list                    # All packages
zl list --explicit         # Only user-requested packages
zl info <package>          # Detailed info for one package
zl size                    # Disk usage
```

### How do I completely remove everything?

```bash
# Remove all ZL data
rm -rf ~/.local/share/zl
rm -rf ~/.config/zl

# Remove the binary
rm ~/.local/bin/zl   # or /usr/local/bin/zl

# Remove PATH entries from your shell config
# (edit ~/.bashrc or ~/.zshrc manually)
```

### Can ZL install GUI applications?

**Yes.** ZL handles `.desktop` files and icons:
- Desktop entries are symlinked to `~/.local/share/applications/`
- Icons are symlinked to `~/.local/share/icons/`

This means GUI applications show up in your desktop launcher automatically.

### Why is my first search slow?

If the source uses a cached database (pacman, apt, dnf, etc.), you need to sync first:

```bash
zl update --from pacman    # Download the database
zl search firefox          # Now it's fast
```

Sources with live APIs (GitHub, AUR, Nix) don't need syncing but require internet for every search.

### How do I update ZL itself?

```bash
zl self-update
```

### How do I report a bug?

Open an issue at: https://github.com/supercosti21/zero_layer/issues

Include:
- ZL version (`zl --version`)
- Your distro and architecture
- The exact command you ran
- The full error output
- Output of `zl doctor`

---

## Next Steps

- [[Commands Reference]] — Full command reference
- [[Configuration]] — Configuration file reference
- [[Security and Verification]] — Security-related troubleshooting
