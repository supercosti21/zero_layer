# Piano: 9 nuovi plugin + sistema di filtraggio sorgenti

## Parte 1 — Sistema di filtraggio sorgenti

### Problema
Oggi ZL registra tutti i plugin e li interroga tutti in parallelo su `search`, `install`, `run`. Con 13+ plugin attivi, questo diventa lento e rumoroso. L'utente deve poter scegliere **quali sorgenti usare**.

### Soluzione: 3 livelli di controllo (dal più persistente al più puntuale)

#### Livello 1 — Config file (`~/.config/zl/config.toml`)

```toml
[general]
# Lista delle sorgenti abilitate. Se presente, SOLO queste vengono caricate.
# Se assente o vuota, tutte le sorgenti sono abilitate.
sources = ["pacman", "aur", "apt", "github"]
```

Ogni plugin mantiene anche il suo `enabled = true/false`:

```toml
[plugins.dnf]
enabled = false    # disabilitato anche se presente in sources
```

La logica: `sources` è la whitelist globale, `enabled` è l'override per plugin.

#### Livello 2 — Comando `zl sources`

Nuovo comando per gestire le sorgenti attive senza editare TOML a mano:

```bash
zl sources list              # mostra tutte le sorgenti (abilitate e disabilitate)
zl sources enable dnf apt    # abilita dnf e apt
zl sources disable snap nix  # disabilita snap e nix
zl sources only pacman aur   # imposta SOLO pacman e aur (disabilita tutto il resto)
zl sources reset             # torna al default (tutte abilitate)
```

Questi comandi modificano `config.toml` automaticamente.

#### Livello 3 — Flag CLI per comando (`--from`)

Già esiste `--from <source>` per un singolo plugin. Lo estendiamo:

```bash
zl search firefox --from pacman,apt      # cerca solo in pacman e apt
zl install firefox --from pacman          # installa solo da pacman (già funziona)
zl search firefox --from pacman,apt,aur   # cerca in 3 sorgenti
```

Cambiamento: `--from` accetta una lista separata da virgole anziché un singolo valore.

#### Livello bonus — First-run wizard

Al primo avvio (quando `config.toml` non esiste), ZL:
1. Rileva la distro corrente (`SystemProfile`)
2. Suggerisce le sorgenti più appropriate (es. su Arch → pacman+aur, su Ubuntu → apt)
3. Mostra un `dialoguer::MultiSelect` per scegliere quali attivare
4. Salva la scelta in `config.toml`

### Implementazione filtraggio

#### File da modificare

1. **`src/config.rs`**
   - Aggiungere campo `sources: Option<Vec<String>>` a `GeneralConfig`
   - Aggiungere metodo `ZlConfig::save()` per scrivere config su disco (necessario per `zl sources` e wizard)

2. **`src/main.rs`**
   - Dopo la registrazione di tutti i plugin, filtrare la registry in base a `config.general.sources`
   - Aggiungere logica first-run wizard prima del dispatch comandi
   - Nuovo metodo `PluginRegistry::retain(sources: &[String])` per filtrare

3. **`src/plugin/mod.rs`**
   - Aggiungere `PluginRegistry::retain(&mut self, names: &[String])` — rimuove plugin non in lista
   - Aggiungere `PluginRegistry::names(&self) -> Vec<&str>` — per elencare i plugin registrati

4. **`src/cli/mod.rs`**
   - Modificare `--from` da `Option<String>` a `Option<String>` ma parsare virgole nel handler
   - Aggiungere `SourcesCommand` enum (List, Enable, Disable, Only, Reset) e `SourcesArgs`

5. **`src/cli/search.rs`** e **`src/cli/install.rs`**
   - Adattare la logica `--from` per accettare lista di plugin (split su virgola)
   - `pick_source()` filtra i plugin in base alla lista

6. **`src/cli/sources.rs`** (nuovo file)
   - Handler per `zl sources list|enable|disable|only|reset`
   - Legge/scrive config.toml

---

## Parte 2 — I 9 nuovi plugin

Ogni plugin segue lo stesso pattern: `src/plugin/<name>/mod.rs` implementa `SourcePlugin`.

### 2.1 `dnf` — Fedora/RHEL/CentOS

- **Sorgente**: repository RPM via metalink/baseurl
- **Sync**: scarica e parsa `repodata/primary.xml.gz` (metadati RPM)
- **Search/Resolve**: query sull'indice locale
- **Download**: `.rpm` dal mirror
- **Extract**: RPM = cpio compresso. Usa `rpm2cpio` logic (header parsing + cpio + zstd/gzip/xz)
- **Config**: mirror, repos (fedora, updates), arch
- **Dipendenze crate**: nessuna nuova (xml parsing con `quick-xml` o manuale)

### 2.2 `zypper` — openSUSE/SLES

- **Sorgente**: repository RPM via OBS
- **Sync**: simile a dnf, `repodata/primary.xml.gz`
- **Extract**: stesso formato RPM di dnf — **condivide il modulo di estrazione RPM**
- **Config**: mirror (download.opensuse.org), repos, arch

### 2.3 `apk` — Alpine Linux

- **Sorgente**: repository Alpine
- **Sync**: scarica `APKINDEX.tar.gz`, parsa il formato chiave=valore
- **Download**: `.apk` (tar.gz con firma + dati)
- **Extract**: tar.gz standard (già supportato)
- **Config**: mirror (dl-cdn.alpinelinux.org), branch (v3.19), repos (main, community)
- **Nota**: pacchetti musl-based, ottimi per sistemi musl

### 2.4 `xbps` — Void Linux

- **Sorgente**: repository Void
- **Sync**: scarica `<arch>-repodata` (formato plist compresso zstd)
- **Download**: `.xbps` (tar.zst)
- **Extract**: tar + zstd (già supportato)
- **Config**: mirror (repo-default.voidlinux.org), arch

### 2.5 `portage` — Gentoo

- **Sorgente**: binhost Gentoo (pacchetti precompilati)
- **Sync**: scarica `Packages` index dal binhost
- **Download**: `.tbz2` o `.gpkg.tar` (Gentoo binary package)
- **Extract**: tar + bzip2 (già supportato) o tar per gpkg
- **Config**: binhost URL, arch (amd64)
- **Nota**: solo binhost, non compilazione da ebuild

### 2.6 `nix` — NixOS / qualsiasi distro

- **Sorgente**: cache.nixos.org (binary cache) + nixpkgs channel
- **Sync**: scarica store-paths.xz o usa l'API di ricerca Nix
- **Search**: usa `search.nixos.org` API (ElasticSearch)
- **Resolve**: query su `cache.nixos.org` per hash NAR
- **Download**: `.nar` o `.nar.xz` dal binary cache
- **Extract**: formato NAR (Nix ARchive) — serve un parser custom semplice
- **Config**: channel (nixos-unstable, nixos-24.05), cache URL

### 2.7 `flatpak` — Flathub

- **Sorgente**: Flathub AppStream API
- **Sync**: scarica appstream metadata da Flathub
- **Search**: query su appstream XML/JSON
- **Download**: `.flatpakref` o diretto da Flathub (bundle ostree)
- **Extract**: estrae i file dall'ostree bundle
- **Config**: remote URL (flathub.org)
- **Nota**: più complesso degli altri per il formato ostree

### 2.8 `snap` — Snapcraft

- **Sorgente**: Snapcraft Store API
- **Sync**: no-op (API live come AUR/GitHub)
- **Search/Resolve**: query su `api.snapcraft.io`
- **Download**: `.snap` (squashfs)
- **Extract**: squashfs — serve `unsquashfs` o parsing manuale
- **Config**: channel (stable, edge, beta)

### 2.9 `appimage` — AppImageHub

- **Sorgente**: AppImageHub API / GitHub releases con tag AppImage
- **Sync**: no-op (API live)
- **Search**: query su appimage.github.io feed o API
- **Download**: `.AppImage` binario
- **Extract**: AppImage = ELF + squashfs. Il plugin GitHub già gestisce AppImage parzialmente — possiamo estrarre quella logica e riusarla
- **Config**: nessuna speciale

---

## Parte 3 — Ordine di implementazione

### Fase 1: Infrastruttura filtraggio (prima di tutto)
1. Aggiungere `sources` a `GeneralConfig` + `ZlConfig::save()`
2. Aggiungere `PluginRegistry::retain()` + filtro in `main.rs`
3. Estendere `--from` per accettare lista virgolata
4. Implementare `zl sources` command
5. Implementare first-run wizard
6. Test unitari per tutto

### Fase 2: Plugin con formato condiviso RPM
7. Modulo condiviso `plugin/rpm/` per parsing ed estrazione RPM
8. Plugin `dnf` (usa modulo RPM)
9. Plugin `zypper` (usa modulo RPM)

### Fase 3: Plugin distro-native semplici
10. Plugin `apk` (Alpine) — formato semplice, tar.gz
11. Plugin `xbps` (Void) — tar.zst, già supportato
12. Plugin `portage` (Gentoo binhost) — tar.bz2, già supportato

### Fase 4: Plugin ecosistema universale
13. Plugin `nix` — richiede parser NAR
14. Plugin `flatpak` — richiede gestione ostree
15. Plugin `snap` — richiede unsquashfs
16. Plugin `appimage` — il più semplice, riusa logica GitHub

### Fase 5: Documentazione e polish
17. Aggiornare CLAUDE.md con tutti i nuovi plugin
18. Aggiornare README.md
19. Aggiornare GitHub metadata (topics)

---

## Crate aggiuntivi necessari

| Crate | Scopo | Usato da |
|-------|-------|----------|
| `quick-xml` | Parsing XML per repodata RPM | dnf, zypper |
| `cpio` | Estrazione archivi cpio (dentro RPM) | dnf, zypper |
| `plist` (o manuale) | Parsing plist per XBPS repodata | xbps |

I formati tar.gz, tar.zst, tar.bz2, tar.xz sono **già supportati** nel progetto.

---

## Struttura file finale

```
src/plugin/
├── mod.rs              # SourcePlugin, PluginRegistry (+ retain, names)
├── pacman/             # esistente
├── aur/                # esistente
├── apt/                # esistente
├── github/             # esistente
├── rpm/                # NUOVO — modulo condiviso per RPM
│   ├── mod.rs          # RepoData parsing, RPM extraction
│   └── extract.rs      # cpio + decompressione
├── dnf/                # NUOVO
│   └── mod.rs
├── zypper/             # NUOVO
│   └── mod.rs
├── apk/                # NUOVO (Alpine, non Android!)
│   └── mod.rs
├── xbps/               # NUOVO
│   └── mod.rs
├── portage/            # NUOVO
│   └── mod.rs
├── nix/                # NUOVO
│   ├── mod.rs
│   └── nar.rs          # Parser formato NAR
├── flatpak/            # NUOVO
│   └── mod.rs
├── snap/               # NUOVO
│   └── mod.rs
└── appimage/           # NUOVO
    └── mod.rs

src/cli/
├── sources.rs          # NUOVO — handler per `zl sources`
└── ...                 # modifiche a mod.rs, install.rs, search.rs
```
