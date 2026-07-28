//! Parse RPM `repodata/repomd.xml` — the index that points at every metadata
//! file in a repository.
//!
//! A repository's primary metadata is **not** at a fixed path: it is named
//! after its own checksum (e.g. `repodata/<sha256>-primary.xml.zst`) and must
//! be discovered by first fetching `repodata/repomd.xml` and following the
//! `<data type="primary"><location href=.../></data>` entry. The compression
//! also varies — modern Fedora ships zstd, older repos gzip — so the extension
//! on the href is what decides the decompressor, never an assumption.

use std::io::Read;

use crate::error::{ZlError, ZlResult};

/// One `<data>` entry from repomd.xml (e.g. primary, filelists, other).
#[derive(Debug, Clone)]
pub struct RepoMdData {
    /// The `type` attribute: "primary", "filelists", "primary_db", …
    pub data_type: String,
    /// The `href` of the file, relative to the repository root.
    pub location_href: String,
}

/// Parse repomd.xml into its list of data entries.
pub fn parse_repomd<R: Read>(reader: R) -> ZlResult<Vec<RepoMdData>> {
    use quick_xml::events::Event;
    use quick_xml::reader::Reader;
    use std::io::BufReader;

    let mut xml = Reader::from_reader(BufReader::new(reader));
    let mut entries = Vec::new();
    let mut buf = Vec::new();

    let mut current_type: Option<String> = None;

    loop {
        match xml.read_event_into(&mut buf) {
            Ok(Event::Eof) => break,
            Ok(Event::Start(e)) if local_name(e.name().as_ref()) == "data" => {
                current_type = attr_value(&e, b"type");
            }
            Ok(Event::End(e)) if local_name(e.name().as_ref()) == "data" => {
                current_type = None;
            }
            // <location> is an empty (self-closing) element.
            Ok(Event::Empty(e)) | Ok(Event::Start(e))
                if local_name(e.name().as_ref()) == "location" =>
            {
                if let (Some(data_type), Some(href)) = (&current_type, attr_value(&e, b"href")) {
                    entries.push(RepoMdData {
                        data_type: data_type.clone(),
                        location_href: href,
                    });
                }
            }
            Err(e) => {
                return Err(ZlError::Plugin {
                    plugin: "rpm-repomd".into(),
                    message: format!("repomd.xml parse error: {}", e),
                });
            }
            _ => {}
        }
        buf.clear();
    }

    Ok(entries)
}

/// Return the `href` of the primary metadata file. Prefers the XML `primary`
/// over the sqlite `primary_db`, since this crate parses primary.xml.
pub fn primary_href(entries: &[RepoMdData]) -> Option<String> {
    entries
        .iter()
        .find(|d| d.data_type == "primary")
        .map(|d| d.location_href.clone())
}

/// Decompress raw metadata bytes according to the extension on its href and
/// parse the resulting primary.xml. Supports zstd (`.zst`), gzip (`.gz`),
/// xz (`.xz`) and uncompressed (`.xml`).
pub fn parse_primary_by_href(
    href: &str,
    bytes: Vec<u8>,
) -> ZlResult<Vec<super::repodata::RpmEntry>> {
    let cursor = std::io::Cursor::new(bytes);
    let ext = href.rsplit('.').next().unwrap_or("").to_lowercase();
    match ext.as_str() {
        "zst" | "zstd" => {
            let decoded = zstd::stream::decode_all(cursor).map_err(|e| ZlError::Plugin {
                plugin: "rpm-repomd".into(),
                message: format!("zstd decode failed for {}: {}", href, e),
            })?;
            super::repodata::parse_primary_xml(std::io::Cursor::new(decoded))
        }
        "gz" => super::repodata::parse_primary_xml(flate2::read::GzDecoder::new(cursor)),
        "xz" => super::repodata::parse_primary_xml(xz2::read::XzDecoder::new(cursor)),
        _ => super::repodata::parse_primary_xml(cursor),
    }
}

fn local_name(full: &[u8]) -> String {
    let s = std::str::from_utf8(full).unwrap_or("");
    s.rsplit(':').next().unwrap_or(s).to_string()
}

fn attr_value(e: &quick_xml::events::BytesStart, key: &[u8]) -> Option<String> {
    e.attributes()
        .flatten()
        .find(|a| a.key.as_ref() == key)
        .map(|a| String::from_utf8_lossy(&a.value).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<repomd xmlns="http://linux.duke.edu/metadata/repo"
        xmlns:rpm="http://linux.duke.edu/metadata/rpm">
  <revision>1720000000</revision>
  <data type="primary">
    <checksum type="sha256">deadbeef</checksum>
    <location href="repodata/deadbeef-primary.xml.zst"/>
    <size>12345</size>
  </data>
  <data type="filelists">
    <location href="repodata/cafef00d-filelists.xml.zst"/>
  </data>
  <data type="primary_db">
    <location href="repodata/12345-primary.sqlite.zst"/>
  </data>
</repomd>"#;

    #[test]
    fn test_parse_repomd_finds_all_data() {
        let entries = parse_repomd(SAMPLE.as_bytes()).unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].data_type, "primary");
        assert_eq!(
            entries[0].location_href,
            "repodata/deadbeef-primary.xml.zst"
        );
    }

    #[test]
    fn test_primary_href_prefers_xml_over_db() {
        let entries = parse_repomd(SAMPLE.as_bytes()).unwrap();
        let href = primary_href(&entries).unwrap();
        assert_eq!(href, "repodata/deadbeef-primary.xml.zst");
        assert!(!href.contains("sqlite"));
    }

    #[test]
    fn test_primary_href_none_when_absent() {
        let xml = r#"<repomd><data type="filelists"><location href="a.xml.gz"/></data></repomd>"#;
        let entries = parse_repomd(xml.as_bytes()).unwrap();
        assert!(primary_href(&entries).is_none());
    }

    #[test]
    fn test_parse_primary_by_href_plain_xml() {
        let xml = br#"<?xml version="1.0"?>
<metadata xmlns="http://linux.duke.edu/metadata/common" packages="1">
  <package type="rpm">
    <name>jq</name>
    <arch>x86_64</arch>
    <version epoch="0" ver="1.7.1" rel="1.fc43"/>
    <summary>Command-line JSON processor</summary>
    <location href="Packages/j/jq-1.7.1-1.fc43.x86_64.rpm"/>
  </package>
</metadata>"#;
        let entries = parse_primary_by_href("x-primary.xml", xml.to_vec()).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "jq");
    }

    #[test]
    fn test_parse_primary_by_href_zstd() {
        let xml = br#"<metadata xmlns="http://linux.duke.edu/metadata/common" packages="1">
  <package type="rpm"><name>bash</name><arch>x86_64</arch>
    <version epoch="0" ver="5.2" rel="1"/><summary>shell</summary>
    <location href="Packages/b/bash.rpm"/></package></metadata>"#;
        let compressed = zstd::stream::encode_all(std::io::Cursor::new(&xml[..]), 3).unwrap();
        let entries = parse_primary_by_href("x-primary.xml.zst", compressed).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "bash");
    }
}
