use crate::model::{ManifestFormat, ManifestInfo};

const RES_XML_TYPE: u16 = 0x0003;
const RES_CHUNK_HEADER_SIZE: usize = 8;

pub fn inspect_manifest(bytes: &[u8]) -> ManifestInfo {
    let first_non_whitespace = bytes
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .unwrap_or(bytes.len());

    let visible = &bytes[first_non_whitespace..];
    if visible.starts_with(b"<?xml") || visible.starts_with(b"<manifest") {
        return ManifestInfo {
            format: ManifestFormat::PlainXml,
            chunk_type: None,
            header_size: None,
            declared_size: None,
            bytes_sampled: bytes.len(),
        };
    }

    if bytes.len() >= RES_CHUNK_HEADER_SIZE {
        let chunk_type = read_u16(bytes, 0);
        let header_size = read_u16(bytes, 2);
        let declared_size = read_u32(bytes, 4);

        if chunk_type == RES_XML_TYPE
            && usize::from(header_size) >= RES_CHUNK_HEADER_SIZE
            && u32::from(header_size) <= declared_size
        {
            return ManifestInfo {
                format: ManifestFormat::AndroidBinaryXml,
                chunk_type: Some(chunk_type),
                header_size: Some(header_size),
                declared_size: Some(declared_size),
                bytes_sampled: bytes.len(),
            };
        }

        return ManifestInfo {
            format: ManifestFormat::Unknown,
            chunk_type: Some(chunk_type),
            header_size: Some(header_size),
            declared_size: Some(declared_size),
            bytes_sampled: bytes.len(),
        };
    }

    ManifestInfo {
        format: ManifestFormat::Unknown,
        chunk_type: None,
        header_size: None,
        declared_size: None,
        bytes_sampled: bytes.len(),
    }
}

fn read_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_android_binary_xml_header() {
        let bytes = [0x03, 0x00, 0x08, 0x00, 0x20, 0x00, 0x00, 0x00];
        let info = inspect_manifest(&bytes);

        assert!(matches!(info.format, ManifestFormat::AndroidBinaryXml));
        assert_eq!(info.declared_size, Some(32));
    }

    #[test]
    fn recognizes_plain_xml() {
        let info = inspect_manifest(b"\n  <manifest package=\"dev.winedroid.test\"/>");
        assert!(matches!(info.format, ManifestFormat::PlainXml));
    }

    #[test]
    fn rejects_short_input_without_panicking() {
        let info = inspect_manifest(&[0x03, 0x00]);
        assert!(matches!(info.format, ManifestFormat::Unknown));
    }
}
