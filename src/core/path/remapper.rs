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
    let shebang = &content[..first_line_end];

    let new_shebang = mapping.remap_path(shebang);
    if new_shebang != shebang {
        let new_content = format!("{}{}", new_shebang, &content[first_line_end..]);
        std::fs::write(path, &new_content)?;
        Ok(true)
    } else {
        Ok(false)
    }
}
