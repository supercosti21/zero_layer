//! NAR (Nix ARchive) extraction support.
//!
//! NAR is a simple deterministic archive format used by Nix.
//! Files are typically distributed as .nar.xz or .nar.zst.

use std::path::Path;

use crate::error::{ZlError, ZlResult};
use crate::plugin::{ExtractedPackage, PackageCandidate};

/// Extract a NAR archive (optionally compressed with xz or zstd).
pub fn extract_nar(archive_path: &Path) -> ZlResult<ExtractedPackage> {
    let extract_dir = tempfile::tempdir()?;
    let name = archive_path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase();

    // Decompress if needed, then parse NAR format
    if name.ends_with(".nar.xz") {
        let file = std::fs::File::open(archive_path)?;
        let xz = xz2::read::XzDecoder::new(file);
        extract_nar_stream(xz, extract_dir.path())?;
    } else if name.ends_with(".nar.zst") {
        let file = std::fs::File::open(archive_path)?;
        let zst = zstd::stream::Decoder::new(file)
            .map_err(|e| ZlError::Archive(format!("zstd error in NAR: {}", e)))?;
        extract_nar_stream(zst, extract_dir.path())?;
    } else if name.ends_with(".nar") {
        let file = std::fs::File::open(archive_path)?;
        extract_nar_stream(file, extract_dir.path())?;
    } else {
        return Err(ZlError::Archive(format!("Unknown NAR format: {}", name)));
    }

    classify_extracted(extract_dir, archive_path)
}

/// Parse a NAR stream and extract files to dest.
///
/// NAR format is a simple recursive structure:
/// - "nix-archive-1" header
/// - "(" node ")"
/// - node = "type" ("regular" | "directory" | "symlink") + contents
///
/// For now this is a simplified extractor that handles the common cases.
fn extract_nar_stream<R: std::io::Read>(mut reader: R, dest: &Path) -> ZlResult<()> {
    // Read and verify magic
    let magic = read_nar_string(&mut reader)?;
    if magic != "nix-archive-1" {
        return Err(ZlError::Archive(format!(
            "Invalid NAR magic: expected 'nix-archive-1', got '{}'",
            magic
        )));
    }

    // Parse root node
    extract_nar_node(&mut reader, dest)?;
    Ok(())
}

fn extract_nar_node<R: std::io::Read>(reader: &mut R, path: &Path) -> ZlResult<()> {
    let token = read_nar_string(reader)?;
    if token != "(" {
        return Err(ZlError::Archive(format!(
            "Expected '(' in NAR, got '{}'",
            token
        )));
    }

    let type_key = read_nar_string(reader)?;
    if type_key != "type" {
        return Err(ZlError::Archive(format!(
            "Expected 'type' in NAR, got '{}'",
            type_key
        )));
    }

    let node_type = read_nar_string(reader)?;
    match node_type.as_str() {
        "regular" => extract_nar_regular(reader, path)?,
        "directory" => extract_nar_directory(reader, path)?,
        "symlink" => extract_nar_symlink(reader, path)?,
        _ => {
            return Err(ZlError::Archive(format!(
                "Unknown NAR node type: '{}'",
                node_type
            )));
        }
    }

    Ok(())
}

fn extract_nar_regular<R: std::io::Read>(reader: &mut R, path: &Path) -> ZlResult<()> {
    let mut executable = false;

    loop {
        let token = read_nar_string(reader)?;
        match token.as_str() {
            "executable" => {
                executable = true;
                let _empty = read_nar_string(reader)?; // empty string
            }
            "contents" => {
                let size = read_nar_u64(reader)?;
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                let mut file = std::fs::File::create(path)?;
                // Copy exactly `size` bytes from reader to file
                let mut remaining = size;
                let mut buf = [0u8; 8192];
                while remaining > 0 {
                    let to_read = (remaining as usize).min(buf.len());
                    reader.read_exact(&mut buf[..to_read])?;
                    std::io::Write::write_all(&mut file, &buf[..to_read])?;
                    remaining -= to_read as u64;
                }
                // NAR pads to 8-byte boundary
                let padding = (8 - (size % 8)) % 8;
                if padding > 0 {
                    let mut pad = vec![0u8; padding as usize];
                    reader.read_exact(&mut pad)?;
                }
            }
            ")" => {
                if executable {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755));
                }
                return Ok(());
            }
            _ => {
                return Err(ZlError::Archive(format!(
                    "Unexpected token in NAR regular: '{}'",
                    token
                )));
            }
        }
    }
}

fn extract_nar_directory<R: std::io::Read>(reader: &mut R, path: &Path) -> ZlResult<()> {
    std::fs::create_dir_all(path)?;

    loop {
        let token = read_nar_string(reader)?;
        match token.as_str() {
            "entry" => {
                let paren = read_nar_string(reader)?;
                if paren != "(" {
                    return Err(ZlError::Archive("Expected '(' for entry".into()));
                }
                let name_key = read_nar_string(reader)?;
                if name_key != "name" {
                    return Err(ZlError::Archive("Expected 'name' in entry".into()));
                }
                let entry_name = read_nar_string(reader)?;
                let node_key = read_nar_string(reader)?;
                if node_key != "node" {
                    return Err(ZlError::Archive("Expected 'node' in entry".into()));
                }
                let child_path = path.join(&entry_name);
                extract_nar_node(reader, &child_path)?;
                let close = read_nar_string(reader)?;
                if close != ")" {
                    return Err(ZlError::Archive("Expected ')' closing entry".into()));
                }
            }
            ")" => return Ok(()),
            _ => {
                return Err(ZlError::Archive(format!(
                    "Unexpected token in NAR directory: '{}'",
                    token
                )));
            }
        }
    }
}

fn extract_nar_symlink<R: std::io::Read>(reader: &mut R, path: &Path) -> ZlResult<()> {
    let target_key = read_nar_string(reader)?;
    if target_key != "target" {
        return Err(ZlError::Archive("Expected 'target' in symlink".into()));
    }
    let target = read_nar_string(reader)?;
    let close = read_nar_string(reader)?;
    if close != ")" {
        return Err(ZlError::Archive("Expected ')' closing symlink".into()));
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let _ = std::os::unix::fs::symlink(target, path);
    Ok(())
}

/// Read a NAR string: 8-byte little-endian length + content + padding to 8 bytes.
fn read_nar_string<R: std::io::Read>(reader: &mut R) -> ZlResult<String> {
    let len = read_nar_u64(reader)?;
    let mut buf = vec![0u8; len as usize];
    reader
        .read_exact(&mut buf)
        .map_err(|e| ZlError::Archive(format!("NAR read error: {}", e)))?;
    let padding = (8 - (len % 8)) % 8;
    if padding > 0 {
        let mut pad = vec![0u8; padding as usize];
        reader
            .read_exact(&mut pad)
            .map_err(|e| ZlError::Archive(format!("NAR padding read error: {}", e)))?;
    }
    String::from_utf8(buf).map_err(|e| ZlError::Archive(format!("NAR string is not UTF-8: {}", e)))
}

fn read_nar_u64<R: std::io::Read>(reader: &mut R) -> ZlResult<u64> {
    let mut buf = [0u8; 8];
    reader
        .read_exact(&mut buf)
        .map_err(|e| ZlError::Archive(format!("NAR u64 read error: {}", e)))?;
    Ok(u64::from_le_bytes(buf))
}

fn classify_extracted(
    extract_dir: tempfile::TempDir,
    archive_path: &Path,
) -> ZlResult<ExtractedPackage> {
    use crate::core::elf::analysis;

    let mut files = Vec::new();
    let mut elf_files = Vec::new();
    let script_files = Vec::new();

    for entry in walkdir::WalkDir::new(extract_dir.path())
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path().to_path_buf();
        if analysis::is_elf_file(&path) {
            elf_files.push(path.clone());
        }
        files.push(path);
    }

    let fname = archive_path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    let metadata = PackageCandidate {
        name: fname,
        version: String::new(),
        description: String::new(),
        arch: std::env::consts::ARCH.to_string(),
        source: "nix".into(),
        dependencies: vec![],
        provides: vec![],
        conflicts: vec![],
        installed_size: 0,
        download_url: String::new(),
        checksum: None,
    };

    Ok(ExtractedPackage {
        extract_dir,
        metadata,
        files,
        elf_files,
        script_files,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nar_string_encoding() {
        // A NAR string "abc" would be: length=3 (LE u64) + "abc" + 5 bytes padding
        let mut data = Vec::new();
        data.extend_from_slice(&3u64.to_le_bytes()); // length = 3
        data.extend_from_slice(b"abc"); // content
        data.extend_from_slice(&[0u8; 5]); // padding to 8 bytes

        let s = read_nar_string(&mut data.as_slice()).unwrap();
        assert_eq!(s, "abc");
    }

    #[test]
    fn test_nar_string_aligned() {
        // A NAR string "test1234" — length 8, no padding needed
        let mut data = Vec::new();
        data.extend_from_slice(&8u64.to_le_bytes());
        data.extend_from_slice(b"test1234");

        let s = read_nar_string(&mut data.as_slice()).unwrap();
        assert_eq!(s, "test1234");
    }
}
