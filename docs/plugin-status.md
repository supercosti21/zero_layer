# Plugin status

Live state of the 13 source plugins, from an end-to-end sweep run on 2026-07-28
(`zl search jq --from <plugin>` against the real upstreams, plus installs where
noted). Update this file whenever a plugin's state changes.

## Working

| Plugin | Verified | Notes |
|--------|----------|-------|
| `pacman` | search + **install** | `zl install jq --from pacman` installs and runs, pulling glibc and 4 other deps |
| `aur` | search | 65 results for `jq`; install goes through makepkg, not exercised |
| `apt` | search | 70k packages cached, 176 results for `jq` |
| `github` | search + **install** | `zl install BurntSushi/ripgrep --from github` installs and runs |
| `snap` | search | 20 results for `jq` |
| `apk` | search | fixed this session, 12 results for `jq`; install not yet exercised |
| `xbps` | search | parser implemented this session, 8 results for `jq`; install not yet exercised |

## Still broken

Diagnosed but not fixed. Each entry records what was actually observed, so the
next session does not have to re-diagnose.

### `dnf` and `zypper` — no repomd.xml handling (shared fix)

Both build a URL ending in `repodata/primary.xml.gz` and both get a 404. That
path does not exist in **any** RPM repository and never has: the primary file is
named after its own checksum and must be discovered by first fetching
`repodata/repomd.xml` and following its `<data type="primary"><location href=...>`.

Verified against Fedora 43:

```
repodata/repomd.xml       -> 200
repodata/primary.xml.gz   -> 404
location href in repomd   -> repodata/<sha256>-primary.xml.zst
```

Two consequences beyond the URL:

- the primary file is **zstd**-compressed now, not gzip, so the sync path needs
  to pick the decompressor from the filename rather than assuming `.gz`
- `DnfPlugin::DEFAULT_RELEASE` is `"40"`, which is EOL and no longer on the
  mirror; Fedora 43 is current. Prefer resolving the current release, or at
  least bump the constant.

The fix belongs in the shared `src/plugin/rpm/` module (used by both plugins),
next to `repodata.rs`, as a `repomd.rs` that returns the primary file's href.

Relevant code: `src/plugin/dnf/mod.rs` (`primary_xml_url`, `sync`),
`src/plugin/zypper/mod.rs` (same shape), `src/plugin/rpm/repodata.rs`.

### `portage` — binhost path 404s

`https://distfiles.gentoo.org/releases/amd64/binpackages/17.1/x86-64/Packages`
returns 404. `DEFAULT_BINHOST` in `src/plugin/portage/mod.rs` points at a layout
Gentoo no longer serves. Needs the current binhost URL for the 23.0 profiles,
then a re-check of the `Packages` index format.

### `nix` — search API returns 401

`https://search.nixos.org/backend/latest-43-{channel}/_search` answers
`401 Unauthorized`. The Elasticsearch backend behind search.nixos.org requires
HTTP basic auth. Decide between sending the public read-only credentials the
web UI uses and switching to a different index source entirely.

Relevant code: `src/plugin/nix/mod.rs`.

### `flatpak` — wrong method or endpoint (405)

The Flathub API answers `405 Method Not Allowed`. The `/api/v2` root itself is
reachable (200), so this is a specific endpoint or verb mismatch — find the
current v2 endpoint for listing/searching apps and confirm whether it wants
POST rather than GET.

Relevant code: `src/plugin/flatpak/mod.rs`, `FLATHUB_API`.

### `appimage` — feed.json no longer parses

`https://appimage.github.io/feed.json` downloads but fails to deserialize:
"error decoding response body". The feed's schema has drifted from the structs
in `src/plugin/appimage/mod.rs`. Fetch the feed, diff it against the structs,
and adjust.

## Not yet exercised

Search works but a real install has never been run for `aur`, `apt`, `apk`,
`xbps` and `snap`. Worth doing once each of them is otherwise healthy — the
GitHub and pacman installs both uncovered bugs that search alone never showed.

## How the sweep was run

```bash
cargo build --release
export XDG_CONFIG_HOME=<sandbox>/config XDG_DATA_HOME=<sandbox>/data
printf '[general]\nsources = ["pacman","aur","apt","dnf","zypper","apk","xbps","portage","nix","flatpak","snap","appimage","github"]\n' \
  > "$XDG_CONFIG_HOME/zl/config.toml"
for p in pacman aur apt dnf zypper apk xbps portage nix flatpak snap appimage github; do
  ./target/release/zl search jq --from "$p" --limit 3
done
```

Running against a sandboxed `XDG_DATA_HOME` keeps test installs out of the real
`~/.local/share/zl`, and `-vv` surfaces the sync URLs, which is what made the
404s and the silent 0-package syncs visible.
