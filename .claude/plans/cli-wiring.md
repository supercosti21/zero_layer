# Piano: Wiring CLI handlers

## Obiettivo
Collegare i comandi CLI (`install`, `remove`, `search`, `update`, `list`) alla logica reale, facendo passare il flusso attraverso tutti i moduli gia' implementati.

## Struttura

### 1. Bootstrap comune in `main.rs`
Prima di eseguire qualsiasi comando, main.rs deve:
- Caricare `ZlConfig` (config.toml o defaults)
- Creare `ZlPaths` (dalla config o default `~/.local/share/zl/`)
- Chiamare `paths.ensure_dirs()` per creare le directory
- Aprire `ZlDatabase`
- Creare `PluginRegistry` e registrare `PacmanPlugin`
- Inizializzare ogni plugin con la sua `PluginConfig` (settando `cache_dir`)

### 2. Creare file handler separati: `cli/install.rs`, `cli/remove.rs`, `cli/search.rs`, `cli/update.rs`, `cli/list.rs`
Ogni handler riceve il contesto condiviso (paths, db, registry) e gli args specifici.

### 3. `cli/install.rs` — Il flusso principale
```
fn handle_install(args, paths, db, registry) -> Result:
  1. Determinare quale plugin usare (args.from o "pacman" default)
  2. plugin.sync() — sincronizza database repo
  3. plugin.resolve(name, version) — trova il PackageCandidate
  4. Controllare se gia' installato nel DB
  5. plugin.download(candidate, cache_dir) — scarica .pkg.tar.zst
  6. plugin.extract(archive_path) — estrai in dir temporanea
  7. Creare PathMapping per questo pacchetto
  8. Per ogni ELF nella extracted.elf_files:
     - analysis::analyze(elf_path) — leggi metadati
     - patcher::patch_for_zl(elf_path, info, mapping) — patcha interpreter + RUNPATH
  9. Per ogni script nella extracted.script_files:
     - remapper::remap_shebang(script, mapping)
     - remapper::remap_text_file(script, mapping)
  10. Copiare file dalla dir temporanea alla dir pacchetto definitiva (packages/name-version/)
  11. Creare symlink in bin/ per gli eseguibili
  12. Creare symlink in lib/ per le shared libraries
  13. Costruire PackageNode e salvare nel DB
  14. Registrare file ownership e lib index nel DB
  15. Verificare con verifier (warning se fallisce, non errore fatale)
  16. Stampare riepilogo
```

### 4. `cli/remove.rs`
```
fn handle_remove(args, paths, db) -> Result:
  1. Cercare il pacchetto nel DB per nome
  2. Se non trovato, errore
  3. Leggere la lista file dal PackageNode
  4. Rimuovere i symlink da bin/ e lib/
  5. Rimuovere la directory del pacchetto (packages/name-version/)
  6. Rimuovere file ownership dal DB
  7. Rimuovere pacchetto dal DB
  8. Se --cascade: trovare orfani e rimuoverli ricorsivamente
  9. Stampare riepilogo
```

### 5. `cli/search.rs`
```
fn handle_search(args, paths, registry) -> Result:
  1. Determinare plugin (args.from o tutti)
  2. plugin.sync()
  3. plugin.search(query)
  4. Formattare e stampare risultati (nome, versione, descrizione, source)
```

### 6. `cli/update.rs`
```
fn handle_update(args, paths, db, registry) -> Result:
  1. Se args.package specificato: aggiorna solo quello
  2. Altrimenti: lista tutti i pacchetti dal DB
  3. Per ogni pacchetto: resolve versione piu' recente
  4. Se versione diversa: remove vecchio + install nuovo
  5. Stampare riepilogo
```

### 7. `cli/list.rs`
```
fn handle_list(db) -> Result:
  1. db.list_packages()
  2. Formattare e stampare (nome, versione, source, num file, data)
```

### 8. Aggiungere `PluginRegistry::all()` e `PluginRegistry::get_or_default()`
Per supportare la ricerca su tutti i plugin e il fallback al default.

## File da creare/modificare:
- **Creare**: `src/cli/install.rs`, `src/cli/remove.rs`, `src/cli/search.rs`, `src/cli/update.rs`, `src/cli/list.rs`
- **Modificare**: `src/cli/mod.rs` (aggiungere mod dichiarazioni)
- **Modificare**: `src/main.rs` (bootstrap + dispatch agli handler)
- **Modificare**: `src/plugin/mod.rs` (aggiungere metodi a PluginRegistry)

## Ordine di implementazione:
1. `PluginRegistry::all()` e context struct
2. `main.rs` bootstrap
3. `cli/list.rs` (piu' semplice, verifica che il wiring funzioni)
4. `cli/search.rs`
5. `cli/install.rs` (il piu' complesso)
6. `cli/remove.rs`
7. `cli/update.rs`
8. Compilare, testare, fixare
