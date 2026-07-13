use crate::model::DexInfo;

const DEX_HEADER_SIZE: usize = 0x70;
const DEX_ENDIAN_CONSTANT: u32 = 0x1234_5678;
const REVERSE_ENDIAN_CONSTANT: u32 = 0x7856_3412;

pub fn inspect_dex(path: &str, bytes: &[u8], archive_file_size: u64) -> Result<DexInfo, String> {
    if bytes.len() < DEX_HEADER_SIZE {
        return Err(format!(
            "{path}: cabeçalho DEX incompleto: {} de {DEX_HEADER_SIZE} bytes",
            bytes.len()
        ));
    }

    if bytes.starts_with(b"cdex") {
        return Err(format!(
            "{path}: Compact DEX foi detectado, mas ainda não é suportado"
        ));
    }

    if !bytes.starts_with(b"dex\n") || bytes[7] != 0 {
        return Err(format!("{path}: magic DEX inválida"));
    }

    let version = std::str::from_utf8(&bytes[4..7])
        .map_err(|_| format!("{path}: versão DEX não é ASCII"))?
        .to_owned();

    if !version.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(format!("{path}: versão DEX inválida: {version:?}"));
    }

    let checksum_adler32 = read_u32(bytes, 8);
    let signature_sha1 = bytes[12..32]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let declared_file_size = read_u32(bytes, 32);
    let header_size = read_u32(bytes, 36);
    let endian_tag = read_u32(bytes, 40);

    let mut warnings = Vec::new();

    if u64::from(declared_file_size) != archive_file_size {
        warnings.push(format!(
            "tamanho declarado no DEX ({declared_file_size}) difere do tamanho no APK ({archive_file_size})"
        ));
    }

    if header_size != DEX_HEADER_SIZE as u32 {
        warnings.push(format!(
            "header_size inesperado: {header_size:#x}; esperado {DEX_HEADER_SIZE:#x}"
        ));
    }

    match endian_tag {
        DEX_ENDIAN_CONSTANT => {}
        REVERSE_ENDIAN_CONSTANT => warnings.push(
            "DEX em endian reverso detectado; a execução ainda não será suportada".to_owned(),
        ),
        other => warnings.push(format!("endian_tag desconhecida: {other:#010x}")),
    }

    Ok(DexInfo {
        path: path.to_owned(),
        version,
        checksum_adler32,
        signature_sha1,
        declared_file_size,
        archive_file_size,
        header_size,
        endian_tag,
        string_ids: read_u32(bytes, 56),
        type_ids: read_u32(bytes, 64),
        method_ids: read_u32(bytes, 88),
        class_defs: read_u32(bytes, 96),
        data_size: read_u32(bytes, 104),
        warnings,
    })
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
    fn parses_minimal_standard_dex_header() {
        let mut bytes = vec![0_u8; DEX_HEADER_SIZE];
        bytes[0..8].copy_from_slice(b"dex\n035\0");
        write_u32(&mut bytes, 32, DEX_HEADER_SIZE as u32);
        write_u32(&mut bytes, 36, DEX_HEADER_SIZE as u32);
        write_u32(&mut bytes, 40, DEX_ENDIAN_CONSTANT);
        write_u32(&mut bytes, 56, 7);
        write_u32(&mut bytes, 64, 3);
        write_u32(&mut bytes, 88, 2);
        write_u32(&mut bytes, 96, 1);

        let dex = inspect_dex("classes.dex", &bytes, DEX_HEADER_SIZE as u64)
            .expect("DEX de teste deveria ser válido");

        assert_eq!(dex.version, "035");
        assert_eq!(dex.string_ids, 7);
        assert_eq!(dex.class_defs, 1);
        assert!(dex.warnings.is_empty());
    }

    #[test]
    fn rejects_non_dex_data() {
        let bytes = vec![0_u8; DEX_HEADER_SIZE];
        assert!(inspect_dex("fake.dex", &bytes, DEX_HEADER_SIZE as u64).is_err());
    }

    fn write_u32(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
}
