# Dependency Resolution

This page explains how Zero Layer resolves, downloads, and installs package dependencies — including cross-source resolution, conflict detection, and edge cases.

---

## Table of Contents

- [Overview](#overview)
- [Resolution Algorithm](#resolution-algorithm)
- [Cross-Source Fallback](#cross-source-fallback)
- [Conflict Detection](#conflict-detection)
- [Edge Cases](#edge-cases)
- [Install Order](#install-order)

---

## Overview

When you install a package, it usually depends on other packages (shared libraries, tools, data files). ZL resolves all dependencies recursively before installing anything.

```bash
zl install firefox --from pacman
```

```
Resolving dependencies...

Dependencies to install (5):
  gtk3 3.24.39 (23.1 MB)
  dbus-glib 0.112 (0.4 MB)
  libxt 1.3.0 (0.3 MB)
  nss 3.95 (4.2 MB)
  nspr 4.35 (0.5 MB)

Packages to install (1):
  firefox 120.0 (238.0 MB)

Total installed size: 266.5 MB
```

---

## Resolution Algorithm

ZL uses depth-first recursive resolution:

```
resolve(package, source_plugin, registry, db):
    If package is already in the install plan → skip
    If package is already installed in ZL DB → skip
    If package is system-provided → skip

    candidate = source_plugin.resolve(package.name)

    For each dependency in candidate.dependencies:
        Strip version constraints (e.g., "glibc>=2.33" → "glibc")

        Try resolving in the same source:
            If found → resolve(dep, same_source, ...)  // recurse

        If not found in same source → cross-source fallback:
            Query ALL other enabled sources
            If found in 1 source → use it
            If found in multiple → prompt user to choose
            If not found anywhere → add to unresolvable list

    Add package to install plan (after its deps)
```

### System-provided packages

Before downloading a dependency, ZL checks if the system already provides it. A dependency is considered "system-provided" if:

1. A shared library matching the dependency name exists in the system's library directories (from `SystemProfile.lib_dirs`)
2. The package is already installed via the system package manager

This prevents ZL from downloading `libc6` on a Debian system that already has glibc installed.

### Version constraints

Dependencies often include version constraints like `glibc>=2.33` or `openssl>=3.0`. ZL handles these by:

1. Stripping the version constraint for the initial search
2. If a version is specified and a specific version is found, checking compatibility
3. If constraints can't be satisfied, adding to the unresolvable list (warning, not failure)

---

## Cross-Source Fallback

This is one of ZL's most powerful features. When a dependency isn't found in the primary source, ZL automatically queries all other enabled sources.

### Example scenario

You install a package from pacman that depends on `libfoo`. But `libfoo` isn't in the Arch repos — it's only available from APT or GitHub.

```
Resolving dependencies for firefox (from pacman)...

  Dependency 'libfoo' not found in pacman.
  Searching other sources...

  Found 'libfoo' in 2 source(s):
    1) libfoo 1.2 [apt/main]
    2) libfoo 1.3-r0 [apk/community]
    3) Skip (don't install this dependency)

  Select source for libfoo:
```

### How it works

1. Primary source returns "not found" for a dependency
2. ZL queries all other enabled plugins in parallel
3. Results are collected and presented to the user
4. The user picks which source to use (or skips)
5. Resolution continues with the chosen source for that dependency

This enables truly cross-distribution package management — you can mix packages from Arch, Debian, and GitHub in a single install.

---

## Conflict Detection

Before installing anything, ZL checks for 5 types of conflicts:

### 1. File Ownership

Detects when a new package would overwrite a file owned by an already-installed package.

```
Conflict: file ownership
  /usr/bin/vim is owned by vim-9.0 [pacman]
  firefox-120.0 would also install /usr/bin/vim
```

### 2. Binary Name

Detects when two packages provide executables with the same name.

```
Conflict: binary name
  'python' is already provided by python3-3.11 [pacman]
  python2-2.7 would also provide 'python'
```

### 3. Library Soname

Detects when two packages provide shared libraries with the same soname.

```
Conflict: library soname
  libssl.so.3 is already provided by openssl-3.0 [apt]
  openssl-3.1 would also provide libssl.so.3
```

### 4. Declared Conflicts

Respects `conflicts` declarations from package metadata.

```
Conflict: declared
  Package iptables conflicts with nftables (declared in metadata)
```

### 5. Version Constraints

Detects incompatible version requirements.

```
Conflict: version constraint
  Package A requires glibc>=2.34
  Package B requires glibc<2.33
  These requirements are incompatible.
```

### Conflict resolution

When conflicts are detected:
1. ZL shows all conflicts with details
2. Prompts the user to proceed or cancel
3. If `-y` flag is set, shows warnings but proceeds

---

## Edge Cases

### Circular dependencies

Some packages have circular dependencies (e.g., in Debian, `libc6` depends on `libgcc-s1` which depends on `libc6`).

ZL detects cycles using a visited set. When a cycle is detected:
- The dependency is skipped (it's assumed to be co-installed)
- A debug message is logged (`-vv` to see it)
- Resolution continues normally

### Virtual packages (provides)

Some dependencies are "virtual" — they're not real packages but capabilities. For example:
- `sh` is provided by `bash` (or `dash`, `zsh`)
- `awk` is provided by `gawk` (or `mawk`)
- `java-runtime` is provided by `openjdk`

ZL checks the `provides` field of packages when a dependency can't be found by name.

### Already installed packages

If a dependency is already installed in ZL's database, it's skipped. ZL doesn't re-download or re-install existing packages.

### Unresolvable dependencies

If a dependency can't be found in any source:
- It's added to an "unresolvable" list
- The install continues (doesn't fail)
- A warning is shown at the end:

```
Warning: 2 dependencies could not be resolved:
  - libcustom.so.1
  - libproprietary.so.2
The package may not work correctly.
```

This is by design — many packages have optional dependencies or dependencies that are provided by the kernel or base system.

---

## Install Order

After resolution, ZL determines the correct install order using topological sort:

```
Dependencies are installed before their dependents:

  1. nspr-4.35           (no deps in plan)
  2. nss-3.95            (depends on nspr)
  3. libxt-1.3.0         (no deps in plan)
  4. dbus-glib-0.112     (no deps in plan)
  5. gtk3-3.24.39        (depends on dbus-glib)
  6. firefox-120.0       (depends on all above)
```

The install plan (`InstallPlan`) contains:
- **packages**: ordered list of packages to install (deps first)
- **unresolvable**: list of dependency names that couldn't be resolved

Each entry tracks whether it's **explicit** (user requested) or **implicit** (pulled in as a dependency). This distinction is important for:
- `zl list --explicit` vs `zl list --deps`
- `zl remove --cascade` (only removes implicit packages)
- `zl why` (traces dependency chains)

---

## Dependency Graph

ZL maintains a persistent dependency graph using petgraph:

```
firefox ──depends──> gtk3 ──depends──> glib2
    │                  │
    └──depends──> nss ──depends──> nspr
```

This graph enables:
- **Topological sort** — correct install order
- **Cycle detection** — prevents infinite loops
- **Orphan detection** — find packages no longer needed
- **Reverse dependency lookup** — "what depends on X?"
- **Why queries** — trace the dependency chain for any package

---

## Next Steps

- [[ELF Binary Translation]] — How binaries are patched after resolution
- [[Architecture]] — Overall system architecture
- [[Commands Reference]] — The `install`, `remove --cascade`, and `why` commands
