# Security and Verification

This page documents how Zero Layer verifies package integrity, handles signatures, and audits for vulnerabilities.

---

## Table of Contents

- [Package Verification](#package-verification)
- [SHA256 Checksums](#sha256-checksums)
- [GPG Signatures](#gpg-signatures)
- [CVE Auditing](#cve-auditing)
- [Security Model](#security-model)
- [Skipping Verification](#skipping-verification)

---

## Package Verification

Every package goes through a two-step verification process before installation:

```
[1/4] Downloading 6 package(s)...
[2/4] Verifying packages...    ← This step
[3/4] Installing & patching...
[4/4] Done!
```

### Verification flow

```
For each downloaded package:
  1. SHA256 checksum (if available)
     → Match: continue
     → Mismatch: FAIL (abort install)
     → No checksum: warn, continue

  2. GPG signature (best-effort)
     → Download {url}.sig
     → If .sig exists: verify with system gpg
     → Valid: continue
     → Invalid: WARN (don't fail)
     → No .sig or no gpg: skip silently
```

---

## SHA256 Checksums

### When checksums are available

Most package sources provide checksums:

| Source | Checksum availability |
|--------|----------------------|
| pacman | SHA256 in database |
| aur | SHA256 from makepkg |
| apt | MD5/SHA256 in Packages index |
| dnf | SHA256 in primary.xml |
| zypper | SHA256 in primary.xml |
| apk | SHA256 in APKINDEX |
| portage | SHA256 in Packages index |
| github | No (checksums not in release API) |
| snap | SHA3-384 in channel map |
| flatpak | Managed by flatpak runtime |
| appimage | No standard checksum |

### Verification behavior

```
If checksum is in metadata:
  Calculate SHA256 of downloaded file
  Compare with expected checksum
  If match → pass
  If mismatch → abort with error:
    "Checksum mismatch for firefox-120.0.pkg.tar.zst
     Expected: abc123...
     Got:      def456...
     The file may be corrupted or tampered with."
```

Checksum mismatches are treated as **hard failures** — the install is aborted immediately. This prevents installing corrupted or tampered packages.

---

## GPG Signatures

### How it works

After downloading a package, ZL attempts to verify its GPG signature:

1. Tries to download `{package_url}.sig` from the same server
2. If the `.sig` file exists, runs `gpg --verify {sig_file} {package_file}`
3. Reports the result

### GPG is best-effort

GPG verification does **not** fail the install because:
- Not all sources provide signatures
- Users may not have the signing keys imported
- The `gpg` binary may not be installed

Instead, ZL reports the result:

```
# Signature valid
GPG: firefox-120.0.pkg.tar.zst — verified (Key: 0x1234ABCD)

# Signature invalid
WARNING: GPG signature verification failed for firefox-120.0.pkg.tar.zst
  This could indicate tampering. Proceed with caution.

# No signature available
GPG: No signature found for firefox-120.0.pkg.tar.zst (skipped)

# gpg not installed
GPG: gpg not found on system, skipping signature verification
```

### Importing signing keys

For Arch Linux packages, you may need to import the signing keys:

```bash
# Import Arch Linux master keys
sudo pacman-key --init
sudo pacman-key --populate archlinux

# Or import a specific key
gpg --recv-keys 0x1234ABCD
```

For AUR packages, the PKGBUILD may specify required keys. If verification fails, the build error will tell you which key to import.

---

## CVE Auditing

ZL can check installed packages for known vulnerabilities using the OSV.dev API.

### Usage

```bash
# Audit all installed packages
zl audit

# Audit a specific package
zl audit openssl
```

### How it works

1. ZL reads all installed packages from the database
2. For each package, queries the OSV.dev API with the package name and version
3. OSV.dev returns any known CVEs (Common Vulnerabilities and Exposures)
4. ZL displays the results with severity scores

### Output

```
Auditing 42 package(s) against OSV.dev...

  ! openssl-3.0.0 — 2 vulnerability(ies)
      CVE-2024-1234 [CVSS:8.5] Heap buffer overflow in function X
      CVE-2024-5678 [CVSS:7.2] Use-after-free in SSL handshake

  ! curl-8.1.0 — 1 vulnerability(ies)
      CVE-2024-9012 [CVSS:5.3] Information disclosure via redirect

! Found 3 vulnerability(ies) in 2 package(s).
  hint: update affected packages with `zl update <package>`
```

### OSV.dev

[OSV.dev](https://osv.dev) is an open-source vulnerability database maintained by Google. It aggregates data from multiple sources including:
- NVD (National Vulnerability Database)
- GitHub Security Advisories
- Linux kernel advisories
- Distribution-specific advisories

ZL queries it by package name and version to find applicable CVEs.

---

## Security Model

### What ZL trusts

1. **Package sources** — ZL trusts that the configured mirrors serve legitimate packages. This is the same trust model as your distro's package manager.
2. **HTTPS** — All API calls and downloads use HTTPS by default. HTTP mirrors are supported but not recommended.
3. **Checksums** — When available, checksums verify file integrity.
4. **System gpg** — GPG verification uses the system's gpg binary and keyring.

### What ZL does NOT do

1. **Sandboxing** — ZL does not sandbox installed packages. They run with your user permissions.
2. **Code review** — ZL does not analyze package contents for malicious code.
3. **Mandatory signatures** — GPG verification is best-effort, not mandatory.
4. **Reproducible builds** — ZL does not verify that packages match their source code.

### Recommendations

1. **Use trusted sources** — Only enable sources you trust. Don't add random third-party mirrors.
2. **Keep checksums enabled** — Don't use `--skip-verify` unless you have a specific reason.
3. **Audit regularly** — Run `zl audit` periodically to check for known vulnerabilities.
4. **Update promptly** — When `zl audit` finds issues, update the affected packages.
5. **Use GitHub tokens** — Adding a GitHub token prevents rate limiting and ensures you can always download.

---

## Skipping Verification

The `--skip-verify` flag disables both checksum and GPG verification:

```bash
zl --skip-verify install firefox --from pacman
```

**When to use this:**
- Offline installs from cached packages
- Development/testing environments
- When you've already verified the package externally

**When NOT to use this:**
- Production systems
- Untrusted networks
- Any time security matters

---

## Retry and Network Security

ZL uses exponential backoff for failed downloads:

```
Attempt 1: try download
  → Failed (network error)
Attempt 2: wait 1 second, try again
  → Failed (timeout)
Attempt 3: wait 2 seconds, try again
  → Failed (server error)
Attempt 4: wait 4 seconds, try again
  → Success (or final failure)
```

This prevents:
- Overwhelming servers during outages
- Wasting bandwidth on transient failures
- Hanging indefinitely on network issues

The timeout for each HTTP request is 30 seconds.

---

## Next Steps

- [[Architecture]] — How ZL handles security internally
- [[Configuration]] — Configure mirrors and tokens
- [[Troubleshooting]] — Common verification errors
