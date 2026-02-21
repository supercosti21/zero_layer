use std::path::PathBuf;

use crate::config::ZlConfig;
use crate::error::{ZlError, ZlResult};
use crate::paths::ZlPaths;
use crate::system::SystemProfile;

use super::EnvCommand;

pub fn handle(
    cmd: EnvCommand,
    paths: &ZlPaths,
    _config: &ZlConfig,
    _profile: &SystemProfile,
) -> ZlResult<()> {
    match cmd {
        EnvCommand::Shell(args) => handle_shell(args.name, paths),
        EnvCommand::List => handle_list(paths),
        EnvCommand::Delete(args) => handle_delete(&args.name, paths),
    }
}

/// Enter an ephemeral shell environment.
/// If `name` is None, creates a temporary env that is deleted on exit.
/// If `name` is Some, creates/reuses a named env in ~/.local/share/zl/envs/<name>/.
fn handle_shell(name: Option<String>, paths: &ZlPaths) -> ZlResult<()> {
    let (env_root, is_temporary) = match name {
        Some(ref n) => {
            let env_dir = paths.envs.join(n);
            (env_dir, false)
        }
        None => {
            // Create a truly temporary directory under envs/
            let tmp_name = format!("tmp-{}", std::process::id());
            let env_dir = paths.envs.join(&tmp_name);
            (env_dir, true)
        }
    };

    // Setup the environment root (separate ZL directory structure)
    let env_paths = ZlPaths::new(Some(env_root.as_path()));
    env_paths.ensure_dirs()?;

    let env_name = name.as_deref().unwrap_or("temporary");
    println!("Entering ZL environment: {}", env_name);
    println!("  Root: {}", env_root.display());
    println!("  Packages installed here are isolated from your main ZL installation.");
    if is_temporary {
        println!("  This is a TEMPORARY environment — it will be deleted when you exit.");
    }
    println!();
    println!(
        "  Use `zl --root {} install <pkg>` to install packages in this environment.",
        env_root.display()
    );
    println!("  Type `exit` to leave the environment.");
    println!();

    // Detect the user's shell
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());

    // Build PATH: prepend the env's bin/ directory
    let current_path = std::env::var("PATH").unwrap_or_default();
    let env_path = format!("{}:{}", env_paths.bin.display(), current_path);

    // Build LD_LIBRARY_PATH: prepend the env's lib/ directory
    let current_ld_path = std::env::var("LD_LIBRARY_PATH").unwrap_or_default();
    let env_ld_path = if current_ld_path.is_empty() {
        env_paths.lib.display().to_string()
    } else {
        format!("{}:{}", env_paths.lib.display(), current_ld_path)
    };

    // Spawn a subshell with the modified environment
    let status = std::process::Command::new(&shell)
        .env("PATH", &env_path)
        .env("LD_LIBRARY_PATH", &env_ld_path)
        .env("ZL_ENV", env_name)
        .env("ZL_ENV_ROOT", &env_root)
        .env("PS1", format!("(zl:{}) \\u@\\h:\\w$ ", env_name))
        .status()
        .map_err(|e| ZlError::Environment(format!("Failed to spawn shell: {}", e)))?;

    // Shell exited
    let exit_code = status.code().unwrap_or(0);
    println!(
        "\nExited ZL environment: {} (exit code: {})",
        env_name, exit_code
    );

    // Clean up temporary environments
    if is_temporary {
        println!("Cleaning up temporary environment...");
        if env_root.exists() {
            std::fs::remove_dir_all(&env_root).map_err(|e| {
                ZlError::Environment(format!(
                    "Failed to clean up temporary env at {}: {}",
                    env_root.display(),
                    e
                ))
            })?;
        }
        println!("Temporary environment deleted.");
    }

    Ok(())
}

/// List all named environments
fn handle_list(paths: &ZlPaths) -> ZlResult<()> {
    if !paths.envs.is_dir() {
        println!("No environments found.");
        return Ok(());
    }

    let entries = std::fs::read_dir(&paths.envs)?;
    let mut envs: Vec<(String, u64)> = Vec::new();

    for entry in entries {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();

        // Skip temporary envs that weren't cleaned up
        if name.starts_with("tmp-") {
            continue;
        }

        if entry.file_type()?.is_dir() {
            let size = dir_size(&entry.path());
            envs.push((name, size));
        }
    }

    if envs.is_empty() {
        println!("No environments found.");
        return Ok(());
    }

    println!("ZL environments:");
    for (name, size) in &envs {
        println!("  {} ({:.1} MB)", name, *size as f64 / 1_000_000.0);
    }
    println!("\nUse `zl env shell <name>` to enter an environment.");
    println!("Use `zl env delete <name>` to remove one.");

    Ok(())
}

/// Delete a named environment
fn handle_delete(name: &str, paths: &ZlPaths) -> ZlResult<()> {
    let env_dir = paths.envs.join(name);

    if !env_dir.exists() {
        return Err(ZlError::Environment(format!(
            "Environment '{}' does not exist",
            name
        )));
    }

    let size = dir_size(&env_dir);

    println!(
        "Deleting environment '{}' ({:.1} MB)...",
        name,
        size as f64 / 1_000_000.0
    );

    std::fs::remove_dir_all(&env_dir).map_err(|e| {
        ZlError::Environment(format!("Failed to delete environment '{}': {}", name, e))
    })?;

    println!("Environment '{}' deleted.", name);
    Ok(())
}

/// Calculate total size of a directory tree
fn dir_size(path: &PathBuf) -> u64 {
    walkdir::WalkDir::new(path)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter_map(|e| e.metadata().ok())
        .map(|m| m.len())
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dir_size() {
        let tmp = tempfile::tempdir().unwrap();
        let file1 = tmp.path().join("file1.txt");
        let file2 = tmp.path().join("file2.txt");
        std::fs::write(&file1, "hello").unwrap();
        std::fs::write(&file2, "world!!").unwrap();

        let size = dir_size(&tmp.path().to_path_buf());
        assert_eq!(size, 12); // "hello" (5) + "world!!" (7)
    }
}
