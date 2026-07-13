use crate::model::DexInfo;

const DEX_HEADER_SIZE: usize = 0x70;
const DEX_ENDIAN_CONSTANT: u32 = 0x1234_5678;
const REVERSE_ENDIAN_CONSTANT: u32 = 0x7856_3412;
const NO_INDEX: u32 = 0xffff_ffff;
const SAMPLE_LIMIT: usize = 8;

const STRING_ID_ITEM_SIZE: usize = 4;
const TYPE_ID_ITEM_SIZE: usize = 4;
const PROTO_ID_ITEM_SIZE: usize = 12;
const FIELD_ID_ITEM_SIZE: usize = 8;
const METHOD_ID_ITEM_SIZE: usize = 8;
const CLASS_DEF_ITEM_SIZE: usize = 32;

#[derive(Debug, Clone)]
pub struct DexIndex {
    pub strings: Vec<String>,
    pub types: Vec<String>,
    pub protos: Vec<DexProto>,
    pub fields: Vec<DexField>,
    pub methods: Vec<DexMethod>,
    pub classes: Vec<DexClass>,
}

#[derive(Debug, Clone)]
pub struct DexProto {
    pub shorty: String,
    pub return_type: String,
    pub parameters: Vec<String>,
    pub descriptor: String,
}

#[derive(Debug, Clone)]
pub struct DexField {
    pub class: String,
    pub field_type: String,
    pub name: String,
    pub descriptor: String,
}

#[derive(Debug, Clone)]
pub struct DexMethod {
    pub class: String,
    pub name: String,
    pub proto_index: u16,
    pub descriptor: String,
}

#[derive(Debug, Clone)]
pub struct DexClass {
    pub descriptor: String,
    pub superclass: Option<String>,
    pub interfaces: Vec<String>,
    pub access_flags: u32,
    pub source_file: Option<String>,
    pub class_data_offset: u32,
}

#[derive(Debug, Clone)]
struct DexHeader {
    version: String,
    checksum_adler32: u32,
    signature_sha1: String,
    declared_file_size: u32,
    header_size: u32,
    endian_tag: u32,
    string_ids_size: u32,
    string_ids_off: u32,
    type_ids_size: u32,
    type_ids_off: u32,
    proto_ids_size: u32,
    proto_ids_off: u32,
    field_ids_size: u32,
    field_ids_off: u32,
    method_ids_size: u32,
    method_ids_off: u32,
    class_defs_size: u32,
    class_defs_off: u32,
    data_size: u32,
    warnings: Vec<String>,
}

pub fn inspect_dex(path: &str, bytes: &[u8], archive_file_size: u64) -> Result<DexInfo, String> {
    let header = parse_header(path, bytes, archive_file_size)?;

    if header.endian_tag == REVERSE_ENDIAN_CONSTANT {
        return Err(format!(
            "{path}: DEX em endian reverso ainda não pode ser indexado"
        ));
    }

    let logical_size = usize::try_from(header.declared_file_size)
        .map_err(|_| format!("{path}: tamanho lógico DEX não cabe em usize"))?;
    let logical_bytes = bytes
        .get(..logical_size)
        .ok_or_else(|| format!("{path}: arquivo menor que o tamanho declarado"))?;
    let index = parse_index(path, logical_bytes, &header)?;

    Ok(DexInfo {
        path: path.to_owned(),
        version: header.version,
        checksum_adler32: header.checksum_adler32,
        signature_sha1: header.signature_sha1,
        declared_file_size: header.declared_file_size,
        archive_file_size,
        header_size: header.header_size,
        endian_tag: header.endian_tag,
        string_ids: header.string_ids_size,
        type_ids: header.type_ids_size,
        proto_ids: header.proto_ids_size,
        field_ids: header.field_ids_size,
        method_ids: header.method_ids_size,
        class_defs: header.class_defs_size,
        data_size: header.data_size,
        parsed_strings: index.strings.len(),
        parsed_types: index.types.len(),
        parsed_protos: index.protos.len(),
        parsed_fields: index.fields.len(),
        parsed_methods: index.methods.len(),
        parsed_classes: index.classes.len(),
        class_samples: index
            .classes
            .iter()
            .take(SAMPLE_LIMIT)
            .map(|class| class.descriptor.clone())
            .collect(),
        method_samples: index
            .methods
            .iter()
            .take(SAMPLE_LIMIT)
            .map(|method| method.descriptor.clone())
            .collect(),
        warnings: header.warnings,
    })
}

pub fn parse_dex_index(path: &str, bytes: &[u8]) -> Result<DexIndex, String> {
    let archive_size = u64::try_from(bytes.len())
        .map_err(|_| format!("{path}: tamanho do arquivo não cabe em u64"))?;
    let header = parse_header(path, bytes, archive_size)?;

    if header.endian_tag == REVERSE_ENDIAN_CONSTANT {
        return Err(format!(
            "{path}: DEX em endian reverso ainda não pode ser indexado"
        ));
    }

    let logical_size = usize::try_from(header.declared_file_size)
        .map_err(|_| format!("{path}: tamanho lógico DEX não cabe em usize"))?;
    let logical_bytes = bytes
        .get(..logical_size)
        .ok_or_else(|| format!("{path}: arquivo menor que o tamanho declarado"))?;

    parse_index(path, logical_bytes, &header)
}

fn parse_header(path: &str, bytes: &[u8], archive_file_size: u64) -> Result<DexHeader, String> {
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

    let checksum_adler32 = read_u32_at(bytes, 8, "checksum")?;
    let signature_sha1 = bytes[12..32]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let declared_file_size = read_u32_at(bytes, 32, "file_size")?;
    let header_size = read_u32_at(bytes, 36, "header_size")?;
    let endian_tag = read_u32_at(bytes, 40, "endian_tag")?;

    if declared_file_size < header_size {
        return Err(format!(
            "{path}: file_size {declared_file_size} é menor que header_size {header_size}"
        ));
    }

    if u64::from(declared_file_size) > archive_file_size {
        return Err(format!(
            "{path}: tamanho declarado ({declared_file_size}) excede o arquivo ({archive_file_size})"
        ));
    }

    let mut warnings = Vec::new();

    if u64::from(declared_file_size) != archive_file_size {
        warnings.push(format!(
            "tamanho declarado no DEX ({declared_file_size}) difere do tamanho no APK ({archive_file_size})"
        ));
    }

    if header_size < DEX_HEADER_SIZE as u32 {
        return Err(format!(
            "{path}: header_size {header_size:#x} é menor que {DEX_HEADER_SIZE:#x}"
        ));
    }

    if header_size != DEX_HEADER_SIZE as u32 {
        warnings.push(format!(
            "header_size diferente do formato clássico: {header_size:#x}"
        ));
    }

    match endian_tag {
        DEX_ENDIAN_CONSTANT => {}
        REVERSE_ENDIAN_CONSTANT => warnings
            .push("DEX em endian reverso detectado; a indexação ainda não é suportada".to_owned()),
        other => return Err(format!("{path}: endian_tag desconhecida: {other:#010x}")),
    }

    Ok(DexHeader {
        version,
        checksum_adler32,
        signature_sha1,
        declared_file_size,
        header_size,
        endian_tag,
        string_ids_size: read_u32_at(bytes, 56, "string_ids_size")?,
        string_ids_off: read_u32_at(bytes, 60, "string_ids_off")?,
        type_ids_size: read_u32_at(bytes, 64, "type_ids_size")?,
        type_ids_off: read_u32_at(bytes, 68, "type_ids_off")?,
        proto_ids_size: read_u32_at(bytes, 72, "proto_ids_size")?,
        proto_ids_off: read_u32_at(bytes, 76, "proto_ids_off")?,
        field_ids_size: read_u32_at(bytes, 80, "field_ids_size")?,
        field_ids_off: read_u32_at(bytes, 84, "field_ids_off")?,
        method_ids_size: read_u32_at(bytes, 88, "method_ids_size")?,
        method_ids_off: read_u32_at(bytes, 92, "method_ids_off")?,
        class_defs_size: read_u32_at(bytes, 96, "class_defs_size")?,
        class_defs_off: read_u32_at(bytes, 100, "class_defs_off")?,
        data_size: read_u32_at(bytes, 104, "data_size")?,
        warnings,
    })
}

fn parse_index(path: &str, bytes: &[u8], header: &DexHeader) -> Result<DexIndex, String> {
    validate_table(
        path,
        bytes,
        "string_ids",
        header.string_ids_size,
        header.string_ids_off,
        STRING_ID_ITEM_SIZE,
    )?;
    validate_table(
        path,
        bytes,
        "type_ids",
        header.type_ids_size,
        header.type_ids_off,
        TYPE_ID_ITEM_SIZE,
    )?;
    validate_table(
        path,
        bytes,
        "proto_ids",
        header.proto_ids_size,
        header.proto_ids_off,
        PROTO_ID_ITEM_SIZE,
    )?;
    validate_table(
        path,
        bytes,
        "field_ids",
        header.field_ids_size,
        header.field_ids_off,
        FIELD_ID_ITEM_SIZE,
    )?;
    validate_table(
        path,
        bytes,
        "method_ids",
        header.method_ids_size,
        header.method_ids_off,
        METHOD_ID_ITEM_SIZE,
    )?;
    validate_table(
        path,
        bytes,
        "class_defs",
        header.class_defs_size,
        header.class_defs_off,
        CLASS_DEF_ITEM_SIZE,
    )?;

    let strings = parse_strings(path, bytes, header.string_ids_size, header.string_ids_off)?;
    let types = parse_types(
        path,
        bytes,
        header.type_ids_size,
        header.type_ids_off,
        &strings,
    )?;
    let protos = parse_protos(
        path,
        bytes,
        header.proto_ids_size,
        header.proto_ids_off,
        &strings,
        &types,
    )?;
    let fields = parse_fields(
        path,
        bytes,
        header.field_ids_size,
        header.field_ids_off,
        &strings,
        &types,
    )?;
    let methods = parse_methods(
        path,
        bytes,
        header.method_ids_size,
        header.method_ids_off,
        &strings,
        &types,
        &protos,
    )?;
    let classes = parse_classes(
        path,
        bytes,
        header.class_defs_size,
        header.class_defs_off,
        &strings,
        &types,
    )?;

    Ok(DexIndex {
        strings,
        types,
        protos,
        fields,
        methods,
        classes,
    })
}

fn parse_strings(
    path: &str,
    bytes: &[u8],
    count: u32,
    table_offset: u32,
) -> Result<Vec<String>, String> {
    let count = count_to_usize(path, count, "string_ids")?;
    let mut strings = Vec::with_capacity(count);

    for index in 0..count {
        let item_offset = table_item_offset(
            path,
            bytes,
            "string_id_item",
            table_offset,
            index,
            STRING_ID_ITEM_SIZE,
        )?;
        let data_offset = usize::try_from(read_u32_at(bytes, item_offset, "string_data_off")?)
            .map_err(|_| format!("{path}: string_data_off não cabe em usize"))?;
        let (declared_utf16_size, prefix_size) = read_uleb128(bytes, data_offset)
            .map_err(|error| format!("{path}: string #{index}: {error}"))?;
        let string_start = data_offset
            .checked_add(prefix_size)
            .ok_or_else(|| format!("{path}: overflow no início da string #{index}"))?;
        let tail = bytes
            .get(string_start..)
            .ok_or_else(|| format!("{path}: string #{index} aponta para fora do DEX"))?;
        let encoded_size = tail
            .iter()
            .position(|byte| *byte == 0)
            .ok_or_else(|| format!("{path}: string #{index} não possui terminador NUL"))?;
        let encoded = &tail[..encoded_size];
        let (decoded, actual_utf16_size) =
            decode_mutf8(encoded).map_err(|error| format!("{path}: string #{index}: {error}"))?;

        if actual_utf16_size != declared_utf16_size as usize {
            return Err(format!(
                "{path}: string #{index}: utf16_size declarado {declared_utf16_size}, decodificado {actual_utf16_size}"
            ));
        }

        strings.push(decoded);
    }

    Ok(strings)
}

fn parse_types(
    path: &str,
    bytes: &[u8],
    count: u32,
    table_offset: u32,
    strings: &[String],
) -> Result<Vec<String>, String> {
    let count = count_to_usize(path, count, "type_ids")?;
    let mut types = Vec::with_capacity(count);

    for index in 0..count {
        let item_offset = table_item_offset(
            path,
            bytes,
            "type_id_item",
            table_offset,
            index,
            TYPE_ID_ITEM_SIZE,
        )?;
        let descriptor_index = read_u32_at(bytes, item_offset, "descriptor_idx")?;
        let descriptor = get_index(path, strings, descriptor_index, "string", "type descriptor")?;
        types.push(descriptor.clone());
    }

    Ok(types)
}

fn parse_protos(
    path: &str,
    bytes: &[u8],
    count: u32,
    table_offset: u32,
    strings: &[String],
    types: &[String],
) -> Result<Vec<DexProto>, String> {
    let count = count_to_usize(path, count, "proto_ids")?;
    let mut protos = Vec::with_capacity(count);

    for index in 0..count {
        let item_offset = table_item_offset(
            path,
            bytes,
            "proto_id_item",
            table_offset,
            index,
            PROTO_ID_ITEM_SIZE,
        )?;
        let shorty_index = read_u32_at(bytes, item_offset, "shorty_idx")?;
        let return_type_index = read_u32_at(bytes, item_offset + 4, "return_type_idx")?;
        let parameters_offset = read_u32_at(bytes, item_offset + 8, "parameters_off")?;
        let shorty = get_index(path, strings, shorty_index, "string", "shorty")?.clone();
        let return_type = get_index(
            path,
            types,
            return_type_index,
            "type",
            "prototype return type",
        )?
        .clone();
        let parameters = parse_type_list(path, bytes, parameters_offset, types)?;
        let descriptor = format!("({}){return_type}", parameters.concat());

        protos.push(DexProto {
            shorty,
            return_type,
            parameters,
            descriptor,
        });
    }

    Ok(protos)
}

fn parse_fields(
    path: &str,
    bytes: &[u8],
    count: u32,
    table_offset: u32,
    strings: &[String],
    types: &[String],
) -> Result<Vec<DexField>, String> {
    let count = count_to_usize(path, count, "field_ids")?;
    let mut fields = Vec::with_capacity(count);

    for index in 0..count {
        let item_offset = table_item_offset(
            path,
            bytes,
            "field_id_item",
            table_offset,
            index,
            FIELD_ID_ITEM_SIZE,
        )?;
        let class_index = u32::from(read_u16_at(bytes, item_offset, "field class_idx")?);
        let type_index = u32::from(read_u16_at(bytes, item_offset + 2, "field type_idx")?);
        let name_index = read_u32_at(bytes, item_offset + 4, "field name_idx")?;
        let class = get_index(path, types, class_index, "type", "field class")?.clone();
        let field_type = get_index(path, types, type_index, "type", "field type")?.clone();
        let name = get_index(path, strings, name_index, "string", "field name")?.clone();
        let descriptor = format!("{class}->{name}:{field_type}");

        fields.push(DexField {
            class,
            field_type,
            name,
            descriptor,
        });
    }

    Ok(fields)
}

fn parse_methods(
    path: &str,
    bytes: &[u8],
    count: u32,
    table_offset: u32,
    strings: &[String],
    types: &[String],
    protos: &[DexProto],
) -> Result<Vec<DexMethod>, String> {
    let count = count_to_usize(path, count, "method_ids")?;
    let mut methods = Vec::with_capacity(count);

    for index in 0..count {
        let item_offset = table_item_offset(
            path,
            bytes,
            "method_id_item",
            table_offset,
            index,
            METHOD_ID_ITEM_SIZE,
        )?;
        let class_index = u32::from(read_u16_at(bytes, item_offset, "method class_idx")?);
        let proto_index = read_u16_at(bytes, item_offset + 2, "method proto_idx")?;
        let name_index = read_u32_at(bytes, item_offset + 4, "method name_idx")?;
        let class = get_index(path, types, class_index, "type", "method class")?.clone();
        let proto = get_index(
            path,
            protos,
            u32::from(proto_index),
            "prototype",
            "method prototype",
        )?;
        let name = get_index(path, strings, name_index, "string", "method name")?.clone();
        let descriptor = format!("{class}->{name}{}", proto.descriptor);

        methods.push(DexMethod {
            class,
            name,
            proto_index,
            descriptor,
        });
    }

    Ok(methods)
}

fn parse_classes(
    path: &str,
    bytes: &[u8],
    count: u32,
    table_offset: u32,
    strings: &[String],
    types: &[String],
) -> Result<Vec<DexClass>, String> {
    let count = count_to_usize(path, count, "class_defs")?;
    let mut classes = Vec::with_capacity(count);

    for index in 0..count {
        let item_offset = table_item_offset(
            path,
            bytes,
            "class_def_item",
            table_offset,
            index,
            CLASS_DEF_ITEM_SIZE,
        )?;
        let class_index = read_u32_at(bytes, item_offset, "class_idx")?;
        let access_flags = read_u32_at(bytes, item_offset + 4, "class access_flags")?;
        let superclass_index = read_u32_at(bytes, item_offset + 8, "superclass_idx")?;
        let interfaces_offset = read_u32_at(bytes, item_offset + 12, "interfaces_off")?;
        let source_file_index = read_u32_at(bytes, item_offset + 16, "source_file_idx")?;
        let class_data_offset = read_u32_at(bytes, item_offset + 24, "class_data_off")?;
        let descriptor = get_index(path, types, class_index, "type", "class descriptor")?.clone();
        let superclass = if superclass_index == NO_INDEX {
            None
        } else {
            Some(get_index(path, types, superclass_index, "type", "class superclass")?.clone())
        };
        let interfaces = parse_type_list(path, bytes, interfaces_offset, types)?;
        let source_file = if source_file_index == NO_INDEX {
            None
        } else {
            Some(get_index(path, strings, source_file_index, "string", "source file")?.clone())
        };

        classes.push(DexClass {
            descriptor,
            superclass,
            interfaces,
            access_flags,
            source_file,
            class_data_offset,
        });
    }

    Ok(classes)
}

fn parse_type_list(
    path: &str,
    bytes: &[u8],
    list_offset: u32,
    types: &[String],
) -> Result<Vec<String>, String> {
    if list_offset == 0 {
        return Ok(Vec::new());
    }

    let offset = usize::try_from(list_offset)
        .map_err(|_| format!("{path}: type_list offset não cabe em usize"))?;
    let count = read_u32_at(bytes, offset, "type_list size")?;
    let count = count_to_usize(path, count, "type_list")?;
    let byte_size = count
        .checked_mul(2)
        .ok_or_else(|| format!("{path}: overflow no tamanho de type_list"))?;
    let list_start = offset
        .checked_add(4)
        .ok_or_else(|| format!("{path}: overflow no início de type_list"))?;
    checked_slice(bytes, list_start, byte_size, "type_list")?;

    let mut result = Vec::with_capacity(count);
    for index in 0..count {
        let item_offset = list_start
            .checked_add(index * 2)
            .ok_or_else(|| format!("{path}: overflow no item de type_list"))?;
        let type_index = u32::from(read_u16_at(bytes, item_offset, "type_idx")?);
        result.push(get_index(path, types, type_index, "type", "type_list item")?.clone());
    }

    Ok(result)
}

fn validate_table(
    path: &str,
    bytes: &[u8],
    name: &str,
    count: u32,
    offset: u32,
    item_size: usize,
) -> Result<(), String> {
    if count == 0 {
        return Ok(());
    }

    if offset == 0 {
        return Err(format!(
            "{path}: seção {name} possui itens, mas offset zero"
        ));
    }

    if !offset.is_multiple_of(4) {
        return Err(format!("{path}: seção {name} não está alinhada em 4 bytes"));
    }

    let count = count_to_usize(path, count, name)?;
    let offset = usize::try_from(offset)
        .map_err(|_| format!("{path}: offset da seção {name} não cabe em usize"))?;
    let byte_size = count
        .checked_mul(item_size)
        .ok_or_else(|| format!("{path}: overflow no tamanho da seção {name}"))?;
    checked_slice(bytes, offset, byte_size, name)?;
    Ok(())
}

fn table_item_offset(
    path: &str,
    bytes: &[u8],
    name: &str,
    table_offset: u32,
    index: usize,
    item_size: usize,
) -> Result<usize, String> {
    let table_offset = usize::try_from(table_offset)
        .map_err(|_| format!("{path}: offset de {name} não cabe em usize"))?;
    let relative = index
        .checked_mul(item_size)
        .ok_or_else(|| format!("{path}: overflow calculando índice de {name}"))?;
    let offset = table_offset
        .checked_add(relative)
        .ok_or_else(|| format!("{path}: overflow calculando offset de {name}"))?;
    checked_slice(bytes, offset, item_size, name)?;
    Ok(offset)
}

fn get_index<'a, T>(
    path: &str,
    values: &'a [T],
    index: u32,
    table_name: &str,
    purpose: &str,
) -> Result<&'a T, String> {
    let index = usize::try_from(index)
        .map_err(|_| format!("{path}: índice de {table_name} não cabe em usize"))?;
    values.get(index).ok_or_else(|| {
        format!(
            "{path}: {purpose} referencia {table_name} #{index}, mas existem {} itens",
            values.len()
        )
    })
}

fn count_to_usize(path: &str, count: u32, name: &str) -> Result<usize, String> {
    usize::try_from(count).map_err(|_| format!("{path}: contagem de {name} não cabe em usize"))
}

fn checked_slice<'a>(
    bytes: &'a [u8],
    offset: usize,
    size: usize,
    name: &str,
) -> Result<&'a [u8], String> {
    let end = offset
        .checked_add(size)
        .ok_or_else(|| format!("overflow calculando o fim de {name}"))?;
    bytes
        .get(offset..end)
        .ok_or_else(|| format!("{name} aponta para fora do DEX: {offset:#x}..{end:#x}"))
}

fn read_u16_at(bytes: &[u8], offset: usize, name: &str) -> Result<u16, String> {
    let data = checked_slice(bytes, offset, 2, name)?;
    Ok(u16::from_le_bytes([data[0], data[1]]))
}

fn read_u32_at(bytes: &[u8], offset: usize, name: &str) -> Result<u32, String> {
    let data = checked_slice(bytes, offset, 4, name)?;
    Ok(u32::from_le_bytes([data[0], data[1], data[2], data[3]]))
}

fn read_uleb128(bytes: &[u8], offset: usize) -> Result<(u32, usize), String> {
    let mut result = 0_u32;

    for index in 0..5 {
        let byte = *bytes
            .get(offset + index)
            .ok_or_else(|| "ULEB128 truncado".to_owned())?;
        result |= u32::from(byte & 0x7f) << (index * 7);

        if byte & 0x80 == 0 {
            if index == 4 && byte > 0x0f {
                return Err("ULEB128 excede 32 bits".to_owned());
            }
            return Ok((result, index + 1));
        }
    }

    Err("ULEB128 possui mais de 5 bytes".to_owned())
}

fn decode_mutf8(bytes: &[u8]) -> Result<(String, usize), String> {
    let mut utf16 = Vec::with_capacity(bytes.len());
    let mut cursor = 0;

    while cursor < bytes.len() {
        let first = bytes[cursor];

        match first {
            0x01..=0x7f => {
                utf16.push(u16::from(first));
                cursor += 1;
            }
            0xc0..=0xdf => {
                let second = continuation(bytes, cursor + 1)?;
                let value = (u16::from(first & 0x1f) << 6) | u16::from(second & 0x3f);
                utf16.push(value);
                cursor += 2;
            }
            0xe0..=0xef => {
                let second = continuation(bytes, cursor + 1)?;
                let third = continuation(bytes, cursor + 2)?;
                let value = (u16::from(first & 0x0f) << 12)
                    | (u16::from(second & 0x3f) << 6)
                    | u16::from(third & 0x3f);
                utf16.push(value);
                cursor += 3;
            }
            0 => return Err("NUL literal dentro dos dados MUTF-8".to_owned()),
            _ => {
                return Err(format!(
                    "byte inicial MUTF-8 inválido em {cursor}: {first:#04x}"
                ));
            }
        }
    }

    let utf16_size = utf16.len();
    Ok((String::from_utf16_lossy(&utf16), utf16_size))
}

fn continuation(bytes: &[u8], offset: usize) -> Result<u8, String> {
    let byte = *bytes
        .get(offset)
        .ok_or_else(|| "sequência MUTF-8 truncada".to_owned())?;

    if byte & 0xc0 != 0x80 {
        return Err(format!(
            "continuação MUTF-8 inválida em {offset}: {byte:#04x}"
        ));
    }

    Ok(byte)
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

        let dex = inspect_dex("classes.dex", &bytes, DEX_HEADER_SIZE as u64)
            .expect("DEX de teste deveria ser válido");

        assert_eq!(dex.version, "035");
        assert_eq!(dex.parsed_strings, 0);
        assert_eq!(dex.parsed_classes, 0);
        assert!(dex.warnings.is_empty());
    }

    #[test]
    fn parses_strings_types_and_class_defs() {
        let bytes = build_class_only_dex();
        let index = parse_dex_index("class-only.dex", &bytes)
            .expect("DEX estrutural de teste deveria ser válido");

        assert_eq!(index.strings.len(), 3);
        assert_eq!(index.types.len(), 2);
        assert_eq!(index.classes.len(), 1);
        assert_eq!(index.classes[0].descriptor, "Ldev/winedroid/Main;");
        assert_eq!(
            index.classes[0].superclass.as_deref(),
            Some("Ljava/lang/Object;")
        );
        assert_eq!(index.classes[0].source_file.as_deref(), Some("Main.java"));
    }

    #[test]
    fn decodes_modified_utf8_nul_and_surrogate_pair() {
        let encoded = [b'A', 0xc0, 0x80, 0xed, 0xa0, 0xbd, 0xed, 0xb8, 0x80];
        let (decoded, utf16_size) = decode_mutf8(&encoded).expect("MUTF-8 deveria decodificar");

        assert_eq!(decoded, "A\0😀");
        assert_eq!(utf16_size, 4);
    }

    #[test]
    fn reads_maximum_u32_uleb128() {
        let bytes = [0xff, 0xff, 0xff, 0xff, 0x0f];
        assert_eq!(read_uleb128(&bytes, 0).unwrap(), (u32::MAX, 5));
    }

    #[test]
    fn rejects_non_dex_data() {
        let bytes = vec![0_u8; DEX_HEADER_SIZE];
        assert!(inspect_dex("fake.dex", &bytes, DEX_HEADER_SIZE as u64).is_err());
    }

    fn build_class_only_dex() -> Vec<u8> {
        let strings = ["Ldev/winedroid/Main;", "Ljava/lang/Object;", "Main.java"];
        let string_ids_offset = DEX_HEADER_SIZE;
        let type_ids_offset = string_ids_offset + strings.len() * STRING_ID_ITEM_SIZE;
        let class_defs_offset = type_ids_offset + 2 * TYPE_ID_ITEM_SIZE;
        let data_offset = class_defs_offset + CLASS_DEF_ITEM_SIZE;
        let mut data = Vec::new();
        let mut string_offsets = Vec::new();

        for value in strings {
            string_offsets.push((data_offset + data.len()) as u32);
            write_uleb128(&mut data, value.encode_utf16().count() as u32);
            data.extend_from_slice(value.as_bytes());
            data.push(0);
        }

        let file_size = data_offset + data.len();
        let mut bytes = vec![0_u8; file_size];
        bytes[0..8].copy_from_slice(b"dex\n035\0");
        write_u32(&mut bytes, 32, file_size as u32);
        write_u32(&mut bytes, 36, DEX_HEADER_SIZE as u32);
        write_u32(&mut bytes, 40, DEX_ENDIAN_CONSTANT);
        write_u32(&mut bytes, 56, strings.len() as u32);
        write_u32(&mut bytes, 60, string_ids_offset as u32);
        write_u32(&mut bytes, 64, 2);
        write_u32(&mut bytes, 68, type_ids_offset as u32);
        write_u32(&mut bytes, 96, 1);
        write_u32(&mut bytes, 100, class_defs_offset as u32);
        write_u32(&mut bytes, 104, data.len() as u32);
        write_u32(&mut bytes, 108, data_offset as u32);

        for (index, string_offset) in string_offsets.iter().enumerate() {
            write_u32(
                &mut bytes,
                string_ids_offset + index * STRING_ID_ITEM_SIZE,
                *string_offset,
            );
        }

        write_u32(&mut bytes, type_ids_offset, 0);
        write_u32(&mut bytes, type_ids_offset + TYPE_ID_ITEM_SIZE, 1);

        write_u32(&mut bytes, class_defs_offset, 0);
        write_u32(&mut bytes, class_defs_offset + 4, 1);
        write_u32(&mut bytes, class_defs_offset + 8, 1);
        write_u32(&mut bytes, class_defs_offset + 16, 2);

        bytes[data_offset..].copy_from_slice(&data);
        bytes
    }

    fn write_uleb128(output: &mut Vec<u8>, mut value: u32) {
        loop {
            let mut byte = (value & 0x7f) as u8;
            value >>= 7;
            if value != 0 {
                byte |= 0x80;
            }
            output.push(byte);
            if value == 0 {
                break;
            }
        }
    }

    fn write_u32(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
}
