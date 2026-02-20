use crate::error::ZlResult;

/// A parsed pacman mirror entry
#[derive(Debug, Clone)]
pub struct Mirror {
    pub url: String,
    pub country: Option<String>,
}

/// Default Arch Linux mirrors (tier-1)
const DEFAULT_MIRRORS: &[&str] = &[
    "https://geo.mirror.pkgbuild.com",
    "https://mirror.rackspace.com/archlinux",
    "https://mirrors.kernel.org/archlinux",
];

/// Load mirrors from a mirrorlist file, or return defaults
pub fn load_mirrors(mirrorlist_path: Option<&str>) -> ZlResult<Vec<Mirror>> {
    if let Some(path) = mirrorlist_path {
        let content = std::fs::read_to_string(path)?;
        let mirrors = parse_mirrorlist(&content);
        if mirrors.is_empty() {
            tracing::warn!("No mirrors found in {}, using defaults", path);
            Ok(default_mirrors())
        } else {
            Ok(mirrors)
        }
    } else {
        Ok(default_mirrors())
    }
}

/// Parse a standard pacman mirrorlist file.
/// Format: `Server = https://mirror.example.com/$repo/os/$arch`
fn parse_mirrorlist(content: &str) -> Vec<Mirror> {
    let mut mirrors = Vec::new();
    let mut current_country = None;

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with("## ") {
            current_country = Some(trimmed.trim_start_matches("## ").to_string());
        } else if let Some(url_part) = trimmed.strip_prefix("Server = ") {
            // Strip the $repo/os/$arch suffix — we'll add it back when constructing URLs
            let base_url = url_part
                .replace("$repo/os/$arch", "")
                .trim_end_matches('/')
                .to_string();

            mirrors.push(Mirror {
                url: base_url,
                country: current_country.clone(),
            });
        }
        // Lines starting with # (commented-out servers) are skipped
    }

    mirrors
}

fn default_mirrors() -> Vec<Mirror> {
    DEFAULT_MIRRORS
        .iter()
        .map(|url| Mirror {
            url: url.to_string(),
            country: None,
        })
        .collect()
}

/// Construct the URL for a repository database file
/// e.g., https://mirror.example.com/core/os/x86_64/core.db
pub fn repo_db_url(mirror: &Mirror, repo: &str, arch: &str) -> String {
    format!("{}/{}/os/{}/{}.db", mirror.url, repo, arch, repo)
}

/// Construct the URL to download a specific package file
/// e.g., https://mirror.example.com/core/os/x86_64/linux-6.7.1-1-x86_64.pkg.tar.zst
pub fn package_url(mirror: &Mirror, repo: &str, arch: &str, filename: &str) -> String {
    format!("{}/{}/os/{}/{}", mirror.url, repo, arch, filename)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_mirrorlist() {
        let content = r#"
## Italy
Server = https://archmirror.it/repos/$repo/os/$arch
## Germany
Server = https://mirror.de.example.com/$repo/os/$arch
# Server = https://commented-out.example.com/$repo/os/$arch
"#;
        let mirrors = parse_mirrorlist(content);
        assert_eq!(mirrors.len(), 2);
        assert_eq!(mirrors[0].url, "https://archmirror.it/repos");
        assert_eq!(mirrors[0].country.as_deref(), Some("Italy"));
        assert_eq!(mirrors[1].country.as_deref(), Some("Germany"));
    }

    #[test]
    fn test_repo_db_url() {
        let mirror = Mirror {
            url: "https://geo.mirror.pkgbuild.com".into(),
            country: None,
        };
        assert_eq!(
            repo_db_url(&mirror, "core", "x86_64"),
            "https://geo.mirror.pkgbuild.com/core/os/x86_64/core.db"
        );
    }

    #[test]
    fn test_package_url() {
        let mirror = Mirror {
            url: "https://geo.mirror.pkgbuild.com".into(),
            country: None,
        };
        assert_eq!(
            package_url(&mirror, "extra", "x86_64", "firefox-120.0-1-x86_64.pkg.tar.zst"),
            "https://geo.mirror.pkgbuild.com/extra/os/x86_64/firefox-120.0-1-x86_64.pkg.tar.zst"
        );
    }
}
