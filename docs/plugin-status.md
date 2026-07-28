# Plugin status

Live state of the 13 source plugins. Baseline sweep run on 2026-07-28
(`zl search jq --from <plugin>` against the real upstreams, plus installs where
noted); repair pass the same day. Update this file whenever a plugin's state
changes.

> **Sandbox note:** this repo's CI/agent environment reaches the network through
> an egress proxy that only allows a handful of hosts. `search.nixos.org` is
> reachable; the Fedora, openSUSE, Gentoo, Flathub and AppImage hosts are
> **blocked (403 at CONNECT)**. Fixes for the blocked sources are implemented
> from their documented protocols and unit-tested, but could not be exercised
> end-to-end here — they are marked "pending live verification" below.

## Working

| Plugin | Verified | Notes |
|--------|----------|-------|
| `pacman` | search + **install** | `zl install jq --from pacman` installs and runs, pulling glibc and 4 other deps |
| `aur` | search | 65 results for `jq`; install goes through makepkg, not exercised |
| `apt` | search | 70k packages cached, 176 results for `jq` |
| `github` | search + **install** | `zl install BurntSushi/ripgrep --from github` installs and runs |
| `snap` | search | 20 results for `jq` |
| `apk` | search | fixed 2026-07-28, 12 results for `jq`; install not yet exercised |
| `xbps` | search | parser implemented 2026-07-28, 8 results for `jq`; install not yet exercised |
| `nix` | search | **fixed 2026-07-28, verified live** — 30 results for `jq`. See below. |

### `nix` — fixed (index version + credentials)

Two stale values made every query return `401 Unauthorized`:

- the index name was pinned to `latest-43-<channel>`; the backend has since
  re-indexed and the current ElasticSearch mapping-schema version is **50**
  (`latest-50-<channel>`).
- the hard-coded `Authorization: Basic …` header carried an outdated password.

Both are now sourced from constants (`DEFAULT_INDEX_VERSION`, `SEARCH_USERNAME`,
`SEARCH_PASSWORD`) and the header is built with reqwest's `basic_auth`. The
index version is overridable via `[plugins.nix] index_version` so the next drift
is a config change, not a recompile. Verified live: `zl search jq --from nix`
returns 30 hits.

Relevant code: `src/plugin/nix/mod.rs`.

## Fixed in code — pending live verification

These upstreams are blocked by the sandbox egress policy, so the fixes below are
implemented from each source's documented protocol and covered by unit tests,
but a real `search`/`install` has not been run here. Re-run the sweep from an
unrestricted network to confirm.

### `dnf` and `zypper` — repomd.xml discovery (shared fix)

Both used to build a URL ending in `repodata/primary.xml.gz` and both got a 404:
that path does not exist in an RPM repository. The primary file is named after
its own checksum and is discovered by first fetching `repodata/repomd.xml` and
following its `<data type="primary"><location href=.../></data>` entry. The
primary is also zstd-compressed on modern Fedora, not gzip.

Fix: new shared `src/plugin/rpm/repomd.rs` that parses repomd.xml
(`parse_repomd` / `primary_href`) and decompresses the primary by the extension
on its href (`parse_primary_by_href` — handles `.zst`, `.gz`, `.xz`, plain).
Both `dnf` and `zypper` `sync()` now fetch repomd.xml → resolve the primary href
→ fetch and parse it. `DnfPlugin::DEFAULT_RELEASE` bumped from the EOL `40` to
`43` (overridable via `[plugins.dnf] release`).

Relevant code: `src/plugin/rpm/repomd.rs`, `src/plugin/dnf/mod.rs`,
`src/plugin/zypper/mod.rs`.

### `portage` — binhost path bumped to the 23.0 profile

`DEFAULT_BINHOST` pointed at `.../binpackages/17.1/x86-64`, a profile Gentoo
retired. Updated to `.../binpackages/23.0/x86-64` (the current default profile),
same index format. Overridable via `[plugins.portage] binhost`.

Relevant code: `src/plugin/portage/mod.rs`.

### `flatpak` — search is POST, not GET

The Flathub API v2 answered `405 Method Not Allowed` because the plugin did a
`GET /api/v2/search?q=…`. The v2 search endpoint is `POST /api/v2/search` with a
JSON body `{"query": "…"}`; the response shape (`hits[]` with `app_id`, `name`,
`summary`) already matched the structs. Switched to POST.

Relevant code: `src/plugin/flatpak/mod.rs`.

## Still broken

### `appimage` — feed.json no longer parses

`https://appimage.github.io/feed.json` downloads but fails to deserialize:
"error decoding response body". The feed's schema has drifted from the structs
in `src/plugin/appimage/mod.rs`. Fixing this needs the live feed to diff against
the structs, and the host is blocked by the sandbox egress policy — deferred
until it can be fetched. Fetch the feed, diff it against the structs, adjust.

## Not yet exercised

Search works but a real install has never been run for `aur`, `apt`, `apk`,
`xbps`, `snap` and `nix`. Worth doing once each is otherwise healthy — the
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
