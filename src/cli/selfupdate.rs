use crate::error::{ZlError, ZlResult};

/// GitHub repository for ZL releases
const GITHUB_REPO: &str = "supercosti21/zero_layer";

/// Handle `zl self-update`: download and replace the current binary with the latest release.
pub fn handle() -> ZlResult<()> {
    println!("Checking for updates...");

    let current_version = env!("CARGO_PKG_VERSION");
    println!("Current version: {}", current_version);

    // Get the path of the currently running binary
    let current_exe = std::env::current_exe().map_err(|e| {
        ZlError::SelfUpdate(format!("Cannot determine current executable path: {}", e))
    })?;

    // Resolve symlinks to get the real path
    let real_exe = std::fs::canonicalize(&current_exe).unwrap_or(current_exe.clone());
    tracing::debug!("Current binary: {}", real_exe.display());

    // Check if we can write to the binary location
    if let Some(parent) = real_exe.parent() {
        let metadata = std::fs::metadata(parent).map_err(|e| {
            ZlError::SelfUpdate(format!(
                "Cannot access binary directory {}: {}",
                parent.display(),
                e
            ))
        })?;
        if metadata.permissions().readonly() {
            return Err(ZlError::SelfUpdate(format!(
                "Binary directory {} is not writable. Try running with appropriate permissions.",
                parent.display()
            )));
        }
    }

    // Detect architecture for the download
    let arch = std::env::consts::ARCH;
    let target = match arch {
        "x86_64" => "x86_64-unknown-linux-gnu",
        "aarch64" => "aarch64-unknown-linux-gnu",
        "arm" => "armv7-unknown-linux-gnueabihf",
        "riscv64" => "riscv64gc-unknown-linux-gnu",
        _ => {
            return Err(ZlError::SelfUpdate(format!(
                "Unsupported architecture for self-update: {}",
                arch
            )));
        }
    };

    // Fetch latest release info from GitHub API
    let api_url = format!(
        "https://api.github.com/repos/{}/releases/latest",
        GITHUB_REPO
    );

    let client = reqwest::blocking::Client::builder()
        .user_agent("zl-self-update")
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| ZlError::SelfUpdate(format!("HTTP client error: {}", e)))?;

    let response = client
        .get(&api_url)
        .send()
        .map_err(|e| ZlError::SelfUpdate(format!("Failed to check for updates: {}", e)))?;

    if !response.status().is_success() {
        let msg = if response.status().as_u16() == 404 {
            "No releases found on GitHub — check that the repository has published releases".to_string()
        } else {
            format!(
                "GitHub API returned status {}: check your internet connection or try again later",
                response.status()
            )
        };
        return Err(ZlError::SelfUpdate(msg));
    }

    let body_text = response
        .text()
        .map_err(|e| ZlError::SelfUpdate(format!("Failed to read response: {}", e)))?;
    let body: serde_json::Value = serde_json::from_str(&body_text)
        .map_err(|e| ZlError::SelfUpdate(format!("Failed to parse release info: {}", e)))?;

    let latest_version = body["tag_name"]
        .as_str()
        .ok_or_else(|| ZlError::SelfUpdate("No tag_name in release".into()))?
        .trim_start_matches('v');

    if latest_version == current_version {
        println!("Already at the latest version ({}).", current_version);
        return Ok(());
    }

    println!(
        "New version available: {} -> {}",
        current_version, latest_version
    );

    // Find the right asset for our architecture
    let asset_name = format!("zl-{}", target);
    let assets = body["assets"]
        .as_array()
        .ok_or_else(|| ZlError::SelfUpdate("No assets in release".into()))?;

    let download_url = assets
        .iter()
        .find(|a| {
            a["name"]
                .as_str()
                .map(|n: &str| n.contains(target) || n == asset_name)
                .unwrap_or(false)
        })
        .and_then(|a| a["browser_download_url"].as_str())
        .ok_or_else(|| {
            ZlError::SelfUpdate(format!(
                "No binary found for {} in release {}. Available assets: {}",
                target,
                latest_version,
                assets
                    .iter()
                    .filter_map(|a| a["name"].as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        })?;

    println!("Downloading {}...", download_url);

    let binary_bytes = client
        .get(download_url)
        .send()
        .map_err(|e| ZlError::SelfUpdate(format!("Download failed: {}", e)))?
        .bytes()
        .map_err(|e| ZlError::SelfUpdate(format!("Failed to read download: {}", e)))?;

    // Write to a temp file first, then atomically replace
    let tmp_path = real_exe.with_extension("update-tmp");
    std::fs::write(&tmp_path, &binary_bytes)
        .map_err(|e| ZlError::SelfUpdate(format!("Failed to write update file: {}", e)))?;

    // Make executable
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp_path, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| ZlError::SelfUpdate(format!("Failed to set permissions: {}", e)))?;
    }

    // Atomic rename (same filesystem)
    std::fs::rename(&tmp_path, &real_exe).map_err(|e| {
        // Clean up temp file on failure
        let _ = std::fs::remove_file(&tmp_path);
        ZlError::SelfUpdate(format!(
            "Failed to replace binary: {}. You may need to run with elevated permissions.",
            e
        ))
    })?;

    println!("Updated to {} successfully!", latest_version);
    println!("Restart zl to use the new version.");

    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_version_format() {
        let version = env!("CARGO_PKG_VERSION");
        assert!(!version.is_empty());
        // Should be semver-ish
        assert!(version.contains('.'));
    }
}
