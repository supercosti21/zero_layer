# ELF Binary Translation

This page explains the core technology behind Zero Layer — how it takes a binary compiled for one Linux distribution and makes it run natively on another.

---

## Table of Contents

- [The Problem](#the-problem)
- [What is an ELF Binary?](#what-is-an-elf-binary)
- [What ZL Patches](#what-zl-patches)
- [How Patching Works](#how-patching-works)
- [Architecture Verification](#architecture-verification)
- [Library Resolution](#library-resolution)
- [Post-Install Verification](#post-install-verification)
- [Limitations](#limitations)

---

## The Problem

A binary compiled on Arch Linux might have:
- **Interpreter**: `/lib/ld-linux-x86-64.so.2` (glibc dynamic linker)
- **RPATH**: `/usr/lib` (Arch's library path)
- **Dependencies**: `libc.so.6`, `libssl.so.3`, `libz.so.1`

On Ubuntu, the dynamic linker is at the same path but library layouts differ. On Alpine (musl), the interpreter is completely different (`/lib/ld-musl-x86_64.so.1`). On NixOS, everything lives under `/nix/store/...`.

Without modification, the binary will fail to run because it can't find its dynamic linker or libraries.

**ZL's solution**: At install time, patch the binary to use the correct paths for the target system. After patching, the binary runs natively — no emulation, no containers, no overhead.

---

## What is an ELF Binary?

ELF (Executable and Linkable Format) is the standard binary format on Linux. Every compiled program and shared library is an ELF file.

Key sections relevant to ZL:

| Section | Description |
|---------|-------------|
| `PT_INTERP` | Path to the dynamic linker (e.g., `/lib64/ld-linux-x86-64.so.2`) |
| `DT_NEEDED` | List of shared libraries this binary needs (e.g., `libc.so.6`) |
| `DT_RPATH` | Legacy library search path (embedded in binary) |
| `DT_RUNPATH` | Modern library search path (embedded in binary) |
| `DT_SONAME` | The library's own name (for shared libraries) |
| `e_machine` | CPU architecture (EM_X86_64, EM_AARCH64, etc.) |

ZL uses the **goblin** crate to read these sections and the **elb** crate to modify them.

---

## What ZL Patches

For every ELF binary in a package, ZL modifies two things:

### 1. Dynamic Linker (PT_INTERP)

The dynamic linker (also called the "interpreter") is the first program that runs when you execute a binary. It loads all shared libraries before handing control to the program.

**Before patching:**
```
PT_INTERP: /lib/ld-linux-x86-64.so.2    (Arch's linker)
```

**After patching:**
```
PT_INTERP: /lib64/ld-linux-x86-64.so.2  (Ubuntu's linker, detected by ZL)
```

ZL detects the correct interpreter by reading `PT_INTERP` from an existing system binary (`/bin/sh`). This works on any Linux system.

### 2. Library Search Path (RUNPATH)

The RUNPATH tells the dynamic linker where to find shared libraries at runtime.

**Before patching:**
```
RUNPATH: (none, or /usr/lib)
```

**After patching:**
```
RUNPATH: $ORIGIN/../lib:~/.local/share/zl/lib
```

- `$ORIGIN/../lib` — looks in the package's own lib directory (relative to the binary)
- `~/.local/share/zl/lib` — looks in ZL's shared library directory

This two-path setup means:
1. Package-specific libraries are found first (in the package dir)
2. Shared libraries from other ZL packages are found second (in the global ZL lib dir)
3. System libraries are found last (via the default search path)

### Why RUNPATH, not RPATH?

ZL uses RUNPATH (not the older RPATH) because:

| | RPATH | RUNPATH |
|---|-------|---------|
| Search order | Before `LD_LIBRARY_PATH` | After `LD_LIBRARY_PATH` |
| User override | Cannot be overridden | Can be overridden by user |
| Standard | Legacy (deprecated) | Modern standard |

RUNPATH respects the user's `LD_LIBRARY_PATH`, giving users control over library resolution when needed.

---

## How Patching Works

### Step-by-step

1. **Detect**: Walk the extracted package and identify all ELF files using magic bytes (`\x7fELF`)
2. **Analyze**: Read each ELF with goblin to get current interpreter, NEEDED libs, RPATH/RUNPATH, SONAME, and machine type
3. **Verify arch**: Check `e_machine` matches the host (warn on mismatch)
4. **Patch interpreter**: Use `elb` to set `PT_INTERP` to the system's dynamic linker
5. **Patch RUNPATH**: Use `elb` to set `DT_RUNPATH` to `$ORIGIN/../lib:{zl_lib_dir}`

### Parallel patching

Packages with multiple ELF binaries (common — Firefox has dozens) are patched in parallel:

```
Package has 24 ELF files
  → Split into 4 chunks of 6
  → 4 threads patch simultaneously (thread::scope)
  → All done in ~25% of sequential time
```

### The elb crate

ZL uses `elb` — a pure-Rust patchelf alternative. It modifies ELF binaries in-place by:
1. Parsing the ELF header and program headers
2. Finding or creating the `.interp` section (for interpreter)
3. Finding or creating the `.dynamic` section entries (for RUNPATH)
4. Writing the modified binary back to disk

No external tools needed. No C dependencies.

---

## Architecture Verification

Before patching, ZL checks that the binary's architecture matches the host:

| ELF e_machine | Architecture |
|---------------|-------------|
| EM_X86_64 (62) | x86_64 |
| EM_AARCH64 (183) | aarch64 |
| EM_ARM (40) | armv7 |
| EM_386 (3) | i686 |
| EM_RISCV (243) | riscv64 |
| EM_S390 (22) | s390x |
| EM_PPC64 (21) | ppc64le |

If there's a mismatch (e.g., installing an ARM package on x86_64), ZL prints a warning:

```
Warning: ELF architecture mismatch
  Binary: aarch64 (EM_AARCH64)
  System: x86_64 (EM_X86_64)
  This package may not work on your system.
```

This is a warning, not an error — some packages contain multi-arch files or non-executable ELFs.

---

## Library Resolution

After patching, the dynamic linker searches for libraries in this order:

1. **RUNPATH** directories (set by ZL):
   - `$ORIGIN/../lib` — package's own libraries
   - `~/.local/share/zl/lib` — shared ZL libraries
2. **LD_LIBRARY_PATH** — user environment variable
3. **System default paths** — `/lib`, `/usr/lib`, etc. (from `ld.so.conf`)

### Library tracking

When ZL installs a package that provides shared libraries (`.so` files):

1. The library is placed in the package directory: `packages/<name>-<version>/usr/lib/libfoo.so.1`
2. A symlink is created in ZL's lib directory: `lib/libfoo.so.1 → ../packages/.../libfoo.so.1`
3. The library is registered in the `LIB_INDEX` database table with its soname
4. Other packages that need `libfoo.so.1` can find it via the symlink

### Cache deduplication

Multiple packages may ship identical copies of the same library. `zl cache dedup` scans all packages for duplicate `.so` files (matched by SHA256 hash) and replaces copies with hardlinks, saving disk space.

---

## Post-Install Verification

After installing and patching, ZL verifies that all ELF binaries can find their dependencies:

```
For each ELF binary in the package:
  For each DT_NEEDED library (e.g., libc.so.6, libssl.so.3):
    → Check ZL's LIB_INDEX database
    → Check ZL's lib/ directory
    → Check system lib dirs (from SystemProfile.lib_dirs)
    → If not found anywhere → warn user
```

Example warning:
```
Warning: firefox has unresolved shared libraries:
  libcustom.so.1 — not found in ZL DB or system lib dirs
  Hint: install the package that provides this library
```

This doesn't prevent installation — some libraries are optional or loaded at runtime. It's an informational check to help debug issues.

---

## Script Remapping

ZL doesn't only handle ELF binaries — it also remaps paths in scripts:

### Script detection

A file is detected as a script if:
- Its extension is `.sh`, `.bash`, `.py`, `.pl`, or `.rb`
- It starts with `#!` (shebang line)

### Path remapping

Scripts may contain hardcoded FHS paths that don't exist on the target system or need to point to ZL's directories:

```
Before: /usr/lib/firefox/firefox.sh contains "/usr/lib/firefox"
After:  Remapped to "~/.local/share/zl/packages/firefox-120.0/usr/lib/firefox"
```

The `PathMapping` module handles this translation using the `SystemProfile` to determine the correct target paths.

---

## Limitations

### What ZL can patch

- Dynamic linker path (PT_INTERP)
- Library search paths (RUNPATH)
- Script paths (text replacement)

### What ZL cannot patch

- **Statically linked binaries** — no dynamic linker, no RUNPATH. These usually work as-is since they include all dependencies.
- **Hardcoded absolute paths in compiled code** — if a binary has `/usr/share/myapp/data.db` compiled into it (not as RPATH/RUNPATH but as a regular string), ZL can't change that.
- **Kernel ABI differences** — ZL doesn't emulate syscalls. A binary compiled for a newer kernel may fail on an older one.
- **glibc version requirements** — A binary linked against glibc 2.38 won't work on a system with glibc 2.17. The symbols don't exist.
- **musl ↔ glibc incompatibility** — Binaries linked against glibc can't use musl's dynamic linker (and vice versa). ZL detects this and warns.

### Best practices

For maximum compatibility:
- Prefer musl-linked binaries (static, portable)
- Prefer packages from GitHub Releases (often ship static or musl builds)
- Use `-bin` variants from AUR when available
- Check architecture compatibility before installing

---

## Next Steps

- [[Dependency Resolution]] — How ZL resolves dependencies across sources
- [[Architecture]] — Overall system architecture
- [[Security and Verification]] — Checksum and signature verification
