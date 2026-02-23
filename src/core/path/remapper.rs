use std::path::Path;

use crate::error::ZlResult;

/// Rewrite hardcoded paths in text files (scripts, configs, pkg-config .pc files)
pub fn remap_text_file(path: &Path, mapping: &super::PathMapping) -> ZlResult<bool> {
    let content = std::fs::read_to_string(path)?;
    let mut modified = content.clone();

    // Sort by longest prefix first to avoid partial replacements
    let mut prefixes: Vec<_> = mapping.prefix_map.iter().collect();
    prefixes.sort_by(|a, b| b.0.len().cmp(&a.0.len()));

    for (from, to) in &prefixes {
        modified = modified.replace(from.as_str(), to.as_str());
    }

    if modified != content {
        std::fs::write(path, &modified)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Rewrite shebang lines in scripts
pub fn remap_shebang(path: &Path, mapping: &super::PathMapping) -> ZlResult<bool> {
    let content = std::fs::read_to_string(path)?;
    if !content.starts_with("#!") {
        return Ok(false);
    }

    let first_line_end = content.find('\n').unwrap_or(content.len());
    let shebang = &content[2..first_line_end]; // strip "#!"
    let shebang_trimmed = shebang.trim_start();

    let new_path = mapping.remap_path(shebang_trimmed);
    if new_path != shebang_trimmed {
        let new_shebang = format!("#!{}", new_path);
        let new_content = format!("{}{}", new_shebang, &content[first_line_end..]);
        std::fs::write(path, &new_content)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::path::PathMapping;
    use crate::system::SystemProfile;

    fn test_mapping() -> PathMapping {
        let profile = SystemProfile::detect();
        PathMapping::for_package(
            std::path::Path::new("/tmp/test-zl"),
            "test",
            "1.0",
            &profile,
        )
    }

    #[test]
    fn test_remap_text_file_replaces_paths() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("test.pc");
        std::fs::write(&file_path, "prefix=/usr/lib\nlibdir=/usr/lib/pkgconfig\n").unwrap();

        let mapping = test_mapping();
        let changed = remap_text_file(&file_path, &mapping).unwrap();
        assert!(changed);

        let content = std::fs::read_to_string(&file_path).unwrap();
        assert!(
            content.contains("/tmp/test-zl/lib"),
            "Expected remapped path, got: {}",
            content
        );
        assert!(!content.contains("/usr/lib"));
    }

    #[test]
    fn test_remap_text_file_no_change() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("plain.txt");
        std::fs::write(&file_path, "nothing to remap here\n").unwrap();

        let mapping = test_mapping();
        let changed = remap_text_file(&file_path, &mapping).unwrap();
        assert!(!changed);
    }

    #[test]
    fn test_remap_shebang_replaces() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("script.sh");
        std::fs::write(&file_path, "#!/usr/bin/env bash\necho hello\n").unwrap();

        let mapping = test_mapping();
        let changed = remap_shebang(&file_path, &mapping).unwrap();
        assert!(changed);

        let content = std::fs::read_to_string(&file_path).unwrap();
        let first_line = content.lines().next().unwrap_or("");
        // /usr/bin should be remapped to ZL bin dir
        assert!(
            !first_line.contains("/usr/bin"),
            "Expected /usr/bin to be remapped, got: {}",
            first_line
        );
        assert!(content.contains("echo hello"));
    }

    #[test]
    fn test_remap_shebang_no_shebang() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("noshebang.txt");
        std::fs::write(&file_path, "just a file\n").unwrap();

        let mapping = test_mapping();
        let changed = remap_shebang(&file_path, &mapping).unwrap();
        assert!(!changed);
    }
}
