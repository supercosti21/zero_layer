//! APT Packages index parser.
//!
//! The Packages file uses RFC 2822-like format: records separated by blank
//! lines, fields as `Key: Value`, continuation lines start with a space.

use std::collections::HashMap;

/// A parsed entry from a Packages index file
#[derive(Debug, Clone)]
pub struct AptEntry {
    pub name: String,
    pub version: String,
    pub arch: String,
    pub description: String,
    pub installed_size: u64, // in KiB
    pub filename: String,
    pub sha256: Option<String>,
    pub depends: Vec<String>,
    pub conflicts: Vec<String>,
    pub provides: Vec<String>,
}

/// Parse a decompressed Packages file into a list of entries
pub fn parse(content: &str) -> Vec<AptEntry> {
    let mut entries = Vec::new();
    let mut fields: HashMap<&str, String> = HashMap::new();
    let mut current_key: &str = "";

    for line in content.lines() {
        if line.is_empty() {
            // End of record
            if let Some(entry) = build_entry(&fields) {
                entries.push(entry);
            }
            fields.clear();
            current_key = "";
            continue;
        }

        if line.starts_with(' ') || line.starts_with('\t') {
            // Continuation line — append to current field (with newline separator)
            if !current_key.is_empty()
                && let Some(v) = fields.get_mut(current_key)
            {
                v.push('\n');
                v.push_str(line.trim_start());
            }
        } else if let Some((key, value)) = line.split_once(": ") {
            current_key = key;
            fields.insert(key, value.to_string());
        } else if let Some((key, _)) = line.split_once(':') {
            // Field with empty value
            current_key = key;
            fields.entry(key).or_default();
        }
    }

    // Handle final record if file doesn't end with blank line
    if !fields.is_empty()
        && let Some(entry) = build_entry(&fields)
    {
        entries.push(entry);
    }

    entries
}

fn build_entry(fields: &HashMap<&str, String>) -> Option<AptEntry> {
    let name = fields.get("Package")?.clone();
    let version = fields.get("Version")?.clone();
    let filename = fields.get("Filename")?.clone();

    Some(AptEntry {
        name,
        version,
        arch: fields.get("Architecture").cloned().unwrap_or_default(),
        description: fields
            .get("Description")
            .map(|d| d.lines().next().unwrap_or(d).to_string())
            .unwrap_or_default(),
        installed_size: fields
            .get("Installed-Size")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0),
        filename,
        sha256: fields.get("SHA256").cloned(),
        depends: parse_dep_list(fields.get("Depends").map(String::as_str).unwrap_or("")),
        conflicts: parse_dep_list(fields.get("Conflicts").map(String::as_str).unwrap_or("")),
        provides: parse_dep_list(fields.get("Provides").map(String::as_str).unwrap_or("")),
    })
}

/// Split a comma-separated dependency list, stripping version constraints.
/// "libc6 (>= 2.34), libacl1 (>= 2.2.23)" → ["libc6", "libacl1"]
pub fn parse_dep_list(raw: &str) -> Vec<String> {
    if raw.is_empty() {
        return Vec::new();
    }
    raw.split(',')
        .map(|dep| {
            // Strip alternatives (first option only for now)
            let dep = dep.split('|').next().unwrap_or(dep);
            // Strip version constraint in parentheses
            let dep = dep.split('(').next().unwrap_or(dep);
            dep.trim().to_string()
        })
        .filter(|s| !s.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
Package: vim
Version: 2:9.0.1234-1ubuntu1
Architecture: amd64
Installed-Size: 4567
Depends: vim-common (= 2:9.0.1234-1ubuntu1), libacl1 (>= 2.2.23)
Conflicts: vim-tiny
Provides: editor
Filename: pool/main/v/vim/vim_9.0.1234-1ubuntu1_amd64.deb
Size: 1234567
SHA256: abc123
Description: Vi IMproved - enhanced vi editor
 Vim is an almost compatible version of the UNIX editor Vi.

Package: nano
Version: 7.2-1
Architecture: amd64
Installed-Size: 1234
Filename: pool/main/n/nano/nano_7.2-1_amd64.deb
Size: 500000
Description: small, friendly text editor inspired by Pico

";

    #[test]
    fn test_parse_two_packages() {
        let entries = parse(SAMPLE);
        assert_eq!(entries.len(), 2);

        let vim = &entries[0];
        assert_eq!(vim.name, "vim");
        assert_eq!(vim.version, "2:9.0.1234-1ubuntu1");
        assert_eq!(vim.installed_size, 4567);
        assert_eq!(vim.sha256.as_deref(), Some("abc123"));
        assert_eq!(vim.depends, vec!["vim-common", "libacl1"]);
        assert_eq!(vim.conflicts, vec!["vim-tiny"]);
        assert_eq!(vim.provides, vec!["editor"]);
        assert_eq!(vim.description, "Vi IMproved - enhanced vi editor");

        let nano = &entries[1];
        assert_eq!(nano.name, "nano");
        assert_eq!(nano.depends, Vec::<String>::new());
    }

    #[test]
    fn test_parse_dep_list() {
        let deps = parse_dep_list("libc6 (>= 2.34), libacl1 (>= 2.2.23), libgpm2 | libgpm-dev");
        assert_eq!(deps, vec!["libc6", "libacl1", "libgpm2"]);
    }

    #[test]
    fn test_parse_empty() {
        assert!(parse("").is_empty());
    }
}
