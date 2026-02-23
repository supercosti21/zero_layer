//! `zl audit` — check installed packages for known vulnerabilities (CVE).
//!
//! Uses the OSV.dev API (https://api.osv.dev/v1/query) which is free,
//! open-source, and covers multiple ecosystems.

use console::style;

use crate::core::db::ops::ZlDatabase;
use crate::error::{ZlError, ZlResult};

use super::AuditArgs;

const OSV_API: &str = "https://api.osv.dev/v1/query";

#[derive(serde::Serialize)]
struct OsvQuery {
    package: OsvPackage,
    version: String,
}

#[derive(serde::Serialize)]
struct OsvPackage {
    name: String,
    ecosystem: String,
}

#[derive(serde::Deserialize)]
struct OsvResponse {
    #[serde(default)]
    vulns: Vec<OsvVuln>,
}

#[derive(serde::Deserialize)]
struct OsvVuln {
    id: String,
    summary: Option<String>,
    #[serde(default)]
    severity: Vec<OsvSeverity>,
}

#[derive(serde::Deserialize)]
struct OsvSeverity {
    #[serde(rename = "type")]
    severity_type: String,
    score: String,
}

pub fn handle(args: AuditArgs, db: &ZlDatabase) -> ZlResult<()> {
    let packages = if let Some(ref name) = args.package {
        let pkg = db
            .get_package_by_name(name)?
            .ok_or_else(|| ZlError::PackageNotFound { name: name.clone() })?;
        vec![pkg]
    } else {
        db.list_packages()?
    };

    if packages.is_empty() {
        println!("No packages to audit.");
        return Ok(());
    }

    println!(
        "{} Auditing {} package(s) against OSV.dev...\n",
        style("🔍").bold(),
        packages.len()
    );

    let client = reqwest::blocking::Client::builder()
        .user_agent("zero-layer/0.1")
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .unwrap_or_default();

    let mut total_vulns = 0;
    let mut vulnerable_pkgs = 0;

    for pkg in &packages {
        let ecosystem = match pkg.id.source.as_str() {
            "pacman" | "aur" => "Arch Linux",
            "apt" => "Debian",
            "github" => "GitHub Actions", // best-effort mapping
            _ => "Linux",
        };

        let query = OsvQuery {
            package: OsvPackage {
                name: pkg.id.name.clone(),
                ecosystem: ecosystem.to_string(),
            },
            version: pkg.id.version.clone(),
        };

        match client.post(OSV_API).json(&query).send() {
            Ok(resp) if resp.status().is_success() => {
                if let Ok(osv_resp) = resp.json::<OsvResponse>()
                    && !osv_resp.vulns.is_empty()
                {
                    vulnerable_pkgs += 1;
                    println!(
                        "  {} {}-{} — {} vulnerability(ies)",
                        style("!").red().bold(),
                        pkg.id.name,
                        pkg.id.version,
                        osv_resp.vulns.len()
                    );

                    for vuln in &osv_resp.vulns {
                        total_vulns += 1;
                        let severity = vuln
                            .severity
                            .first()
                            .map(|s| format!(" [{}:{}]", s.severity_type, s.score))
                            .unwrap_or_default();
                        let summary = vuln.summary.as_deref().unwrap_or("No description");
                        println!(
                            "      {}{} — {}",
                            style(&vuln.id).yellow(),
                            severity,
                            summary
                        );
                    }
                    println!();
                }
            }
            Ok(_) => {
                tracing::debug!(
                    "OSV API returned non-success for {}-{}",
                    pkg.id.name,
                    pkg.id.version
                );
            }
            Err(e) => {
                tracing::debug!("OSV query failed for {}: {}", pkg.id.name, e);
            }
        }
    }

    // Summary
    if total_vulns == 0 {
        println!(
            "{} No known vulnerabilities found.",
            style("✓").green().bold()
        );
    } else {
        println!(
            "{} Found {} vulnerability(ies) in {} package(s).",
            style("!").red().bold(),
            total_vulns,
            vulnerable_pkgs
        );
        println!("  hint: update affected packages with `zl update <package>`");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_osv_query_serializes() {
        let q = OsvQuery {
            package: OsvPackage {
                name: "openssl".into(),
                ecosystem: "Arch Linux".into(),
            },
            version: "3.1.0".into(),
        };
        let json = serde_json::to_string(&q).unwrap();
        assert!(json.contains("openssl"));
        assert!(json.contains("Arch Linux"));
    }
}
