use std::path::Path;
use std::process::Command;

use crate::error::{ZlError, ZlResult};

/// Run a command with environment variables and check for success
fn run_build_command(
    cmd: &str,
    args: &[&str],
    cwd: &Path,
    env: &[(String, String)],
) -> ZlResult<()> {
    tracing::debug!("Running: {} {}", cmd, args.join(" "));

    let mut command = Command::new(cmd);
    command.args(args).current_dir(cwd);

    for (key, value) in env {
        command.env(key, value);
    }

    let output = command.output().map_err(|e| ZlError::BuildFailed {
        package: cwd.to_string_lossy().into(),
        message: format!("Failed to run {}: {}", cmd, e),
    })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Err(ZlError::BuildFailed {
            package: cwd.to_string_lossy().into(),
            message: format!(
                "{} {} failed with exit code {}\nstdout: {}\nstderr: {}",
                cmd,
                args.join(" "),
                output.status,
                stdout.chars().take(500).collect::<String>(),
                stderr.chars().take(500).collect::<String>(),
            ),
        });
    }

    Ok(())
}

/// Build using autotools: ./configure && make && make install
pub fn build_autotools(
    source_dir: &Path,
    prefix: &Path,
    configure_flags: &[String],
    env: &[(String, String)],
) -> ZlResult<()> {
    // Run autoreconf if configure doesn't exist but configure.ac does
    if !source_dir.join("configure").exists() && source_dir.join("configure.ac").exists() {
        tracing::info!("Running autoreconf...");
        run_build_command("autoreconf", &["-fi"], source_dir, env)?;
    }

    // ./configure --prefix=...
    let prefix_flag = format!("--prefix={}", prefix.display());
    let mut args: Vec<&str> = vec![&prefix_flag];
    let flag_refs: Vec<&str> = configure_flags.iter().map(|s| s.as_str()).collect();
    args.extend(&flag_refs);

    tracing::info!("Configuring...");
    run_build_command("./configure", &args, source_dir, env)?;

    // make -j$(nproc)
    let nproc = num_cpus();
    let j_flag = format!("-j{}", nproc);
    tracing::info!("Building with {} threads...", nproc);
    run_build_command("make", &[&j_flag], source_dir, env)?;

    // make install
    tracing::info!("Installing...");
    run_build_command("make", &["install"], source_dir, env)?;

    Ok(())
}

/// Build using CMake
pub fn build_cmake(
    source_dir: &Path,
    prefix: &Path,
    extra_flags: &[String],
    env: &[(String, String)],
) -> ZlResult<()> {
    let build_dir = source_dir.join("_zl_build");
    std::fs::create_dir_all(&build_dir)?;

    // cmake -S . -B _zl_build -DCMAKE_INSTALL_PREFIX=...
    let prefix_flag = format!("-DCMAKE_INSTALL_PREFIX={}", prefix.display());
    let mut args = vec![
        "-S",
        source_dir.to_str().unwrap_or("."),
        "-B",
        build_dir.to_str().unwrap_or("_zl_build"),
        &prefix_flag,
    ];
    let flag_refs: Vec<&str> = extra_flags.iter().map(|s| s.as_str()).collect();
    args.extend(&flag_refs);

    tracing::info!("Configuring with CMake...");
    run_build_command("cmake", &args, source_dir, env)?;

    // cmake --build _zl_build -j$(nproc)
    let nproc = num_cpus();
    let j_flag = format!("-j{}", nproc);
    tracing::info!("Building with {} threads...", nproc);
    run_build_command(
        "cmake",
        &[
            "--build",
            build_dir.to_str().unwrap_or("_zl_build"),
            &j_flag,
        ],
        source_dir,
        env,
    )?;

    // cmake --install _zl_build
    tracing::info!("Installing...");
    run_build_command(
        "cmake",
        &["--install", build_dir.to_str().unwrap_or("_zl_build")],
        source_dir,
        env,
    )?;

    Ok(())
}

/// Build using Meson + Ninja
pub fn build_meson(
    source_dir: &Path,
    prefix: &Path,
    extra_flags: &[String],
    env: &[(String, String)],
) -> ZlResult<()> {
    let build_dir = source_dir.join("_zl_build");

    let prefix_str = prefix.to_string_lossy().to_string();
    let prefix_flag = format!("--prefix={}", prefix_str);
    let mut args: Vec<&str> = vec![
        "setup",
        build_dir.to_str().unwrap_or("_zl_build"),
        &prefix_flag,
    ];
    let flag_refs: Vec<&str> = extra_flags.iter().map(|s| s.as_str()).collect();
    args.extend(&flag_refs);

    tracing::info!("Configuring with Meson...");
    run_build_command("meson", &args, source_dir, env)?;

    // ninja -C build
    tracing::info!("Building...");
    run_build_command(
        "ninja",
        &["-C", build_dir.to_str().unwrap_or("_zl_build")],
        source_dir,
        env,
    )?;

    // ninja -C build install
    tracing::info!("Installing...");
    run_build_command(
        "ninja",
        &["-C", build_dir.to_str().unwrap_or("_zl_build"), "install"],
        source_dir,
        env,
    )?;

    Ok(())
}

/// Build using Cargo (Rust projects)
pub fn build_cargo(source_dir: &Path, prefix: &Path, env: &[(String, String)]) -> ZlResult<()> {
    tracing::info!("Building with Cargo...");
    run_build_command("cargo", &["build", "--release"], source_dir, env)?;

    // Copy binary to prefix/bin/
    let bin_dir = prefix.join("bin");
    std::fs::create_dir_all(&bin_dir)?;

    let target_dir = source_dir.join("target").join("release");
    if target_dir.is_dir() {
        for entry in std::fs::read_dir(&target_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() && is_executable(&path) {
                // Skip common non-binary files
                let name = entry.file_name().to_string_lossy().to_string();
                if name.ends_with(".d") || name.ends_with(".rlib") || name.contains('.') {
                    continue;
                }
                let dest = bin_dir.join(&name);
                std::fs::copy(&path, &dest)?;
                tracing::debug!("Installed binary: {}", name);
            }
        }
    }

    Ok(())
}

/// Build using a simple Makefile
pub fn build_make(source_dir: &Path, prefix: &Path, env: &[(String, String)]) -> ZlResult<()> {
    let nproc = num_cpus();
    let j_flag = format!("-j{}", nproc);
    let prefix_var = format!("PREFIX={}", prefix.display());

    tracing::info!("Building with Make ({} threads)...", nproc);
    run_build_command("make", &[&j_flag, &prefix_var], source_dir, env)?;

    tracing::info!("Installing...");
    run_build_command("make", &["install", &prefix_var], source_dir, env)?;

    Ok(())
}

/// Build using a custom script
pub fn build_script(
    script_path: &Path,
    source_dir: &Path,
    prefix: &Path,
    env: &[(String, String)],
) -> ZlResult<()> {
    let prefix_str = prefix.to_string_lossy().to_string();
    tracing::info!("Running build script...");
    run_build_command(
        "bash",
        &[script_path.to_str().unwrap_or("build.sh"), &prefix_str],
        source_dir,
        env,
    )
}

/// Get number of CPU cores for parallel builds
fn num_cpus() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
}

/// Check if a file is executable
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|m| m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}
