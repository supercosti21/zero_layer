//! RPM extraction: parse RPM header, decompress payload, extract cpio archive.
//!
//! RPM file format:
//! 1. Lead (96 bytes, magic \xed\xab\xee\xdb)
//! 2. Signature header (header structure, aligned to 8 bytes)
//! 3. Main header (header structure)
//! 4. Payload (compressed cpio archive — gzip, xz, zstd, or bzip2)

use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use crate::error::{ZlError, ZlResult};

const RPM_MAGIC: [u8; 4] = [0xed, 0xab, 0xee, 0xdb];
const HEADER_MAGIC: [u8; 3] = [0x8e, 0xad, 0xe8];

/// Extract an RPM file to the given destination directory.
/// Returns the list of extracted file paths.
pub fn extract_rpm(rpm_path: &Path, dest: &Path) -> ZlResult<Vec<PathBuf>> {
    let file = std::fs::File::open(rpm_path)?;
    let mut reader = BufReader::new(file);

    // 1. Skip RPM lead (96 bytes)
    skip_lead(&mut reader)?;

    // 2. Skip signature header
    skip_header(&mut reader, true)?;

    // 3. Skip main header
    skip_header(&mut reader, false)?;

    // 4. Detect compression and extract cpio payload
    let mut magic_buf = [0u8; 6];
    reader.read_exact(&mut magic_buf)?;

    // Seek back so the decompressor can read the magic
    reader.seek(SeekFrom::Current(-6))?;

    let extracted = if magic_buf[0..2] == [0x1f, 0x8b] {
        // gzip
        let gz = flate2::read::GzDecoder::new(reader);
        extract_cpio(gz, dest)?
    } else if magic_buf[0..6] == [0xfd, b'7', b'z', b'X', b'Z', 0x00] {
        // xz
        let xz = xz2::read::XzDecoder::new(reader);
        extract_cpio(xz, dest)?
    } else if magic_buf[0..4] == [0x28, 0xb5, 0x2f, 0xfd] {
        // zstd
        let zst = zstd::stream::Decoder::new(reader)
            .map_err(|e| ZlError::Archive(format!("zstd error in RPM: {}", e)))?;
        extract_cpio(zst, dest)?
    } else if magic_buf[0..2] == *b"BZ" {
        // bzip2
        let bz = bzip2::read::BzDecoder::new(reader);
        extract_cpio(bz, dest)?
    } else {
        return Err(ZlError::Archive(format!(
            "Unknown RPM payload compression (magic: {:02x}{:02x}{:02x})",
            magic_buf[0], magic_buf[1], magic_buf[2]
        )));
    };

    Ok(extracted)
}

fn skip_lead<R: Read>(reader: &mut R) -> ZlResult<()> {
    let mut lead = [0u8; 96];
    reader.read_exact(&mut lead)?;
    if lead[0..4] != RPM_MAGIC {
        return Err(ZlError::Archive("Not an RPM file (bad lead magic)".into()));
    }
    Ok(())
}

fn skip_header<R: Read + Seek>(reader: &mut R, align: bool) -> ZlResult<()> {
    let mut magic = [0u8; 3];
    reader.read_exact(&mut magic)?;
    if magic != HEADER_MAGIC {
        return Err(ZlError::Archive("Bad RPM header magic".into()));
    }

    // Skip version (1 byte) + reserved (4 bytes)
    let mut skip = [0u8; 5];
    reader.read_exact(&mut skip)?;

    // nindex (4 bytes BE) + hsize (4 bytes BE)
    let mut counts = [0u8; 8];
    reader.read_exact(&mut counts)?;
    let nindex = u32::from_be_bytes([counts[0], counts[1], counts[2], counts[3]]) as u64;
    let hsize = u32::from_be_bytes([counts[4], counts[5], counts[6], counts[7]]) as u64;

    // Skip index entries (16 bytes each) + data store
    let skip_bytes = nindex * 16 + hsize;
    reader.seek(SeekFrom::Current(skip_bytes as i64))?;

    // Signature header is aligned to 8-byte boundary
    if align {
        let pos = reader.stream_position()?;
        let remainder = pos % 8;
        if remainder != 0 {
            reader.seek(SeekFrom::Current((8 - remainder) as i64))?;
        }
    }

    Ok(())
}

fn extract_cpio<R: Read>(reader: R, dest: &Path) -> ZlResult<Vec<PathBuf>> {
    let mut extracted = Vec::new();
    let mut remaining_reader = reader;

    loop {
        let cpio_reader = match cpio::NewcReader::new(remaining_reader) {
            Ok(r) => r,
            Err(_) => break, // No more entries
        };

        let name = cpio_reader.entry().name().to_string();
        let mode = cpio_reader.entry().mode();
        let is_dir = mode & 0o170000 == 0o040000;

        // cpio "TRAILER!!!" marks end of archive
        if cpio_reader.entry().is_trailer() {
            break;
        }

        // Strip leading "./" or "/"
        let clean_name = name
            .strip_prefix("./")
            .or_else(|| name.strip_prefix('/'))
            .unwrap_or(&name);

        if clean_name.is_empty() || clean_name == "." {
            remaining_reader = cpio_reader
                .finish()
                .map_err(|e| ZlError::Archive(format!("cpio finish error: {}", e)))?;
            continue;
        }

        let out_path = dest.join(clean_name);

        if is_dir {
            std::fs::create_dir_all(&out_path)?;
            remaining_reader = cpio_reader
                .finish()
                .map_err(|e| ZlError::Archive(format!("cpio finish error: {}", e)))?;
        } else {
            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut out_file = std::fs::File::create(&out_path)?;
            remaining_reader = cpio_reader
                .to_writer(&mut out_file)
                .map_err(|e| ZlError::Archive(format!("cpio write error: {}", e)))?;

            // Restore permissions
            let file_mode = mode & 0o7777;
            if file_mode != 0 {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&out_path, std::fs::Permissions::from_mode(file_mode))?;
            }

            extracted.push(out_path);
        }
    }

    Ok(extracted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rpm_magic_constant() {
        assert_eq!(RPM_MAGIC, [0xed, 0xab, 0xee, 0xdb]);
    }

    #[test]
    fn test_header_magic_constant() {
        assert_eq!(HEADER_MAGIC, [0x8e, 0xad, 0xe8]);
    }
}
