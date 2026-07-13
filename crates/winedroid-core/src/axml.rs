use crate::model::{ManifestFormat, ManifestInfo};

const RES_STRING_POOL_TYPE: u16 = 0x0001;
const RES_XML_TYPE: u16 = 0x0003;
const RES_XML_START_NAMESPACE_TYPE: u16 = 0x0100;
const RES_XML_END_NAMESPACE_TYPE: u16 = 0x0101;
const RES_XML_START_ELEMENT_TYPE: u16 = 0x0102;
const RES_XML_END_ELEMENT_TYPE: u16 = 0x0103;
const RES_XML_CDATA_TYPE: u16 = 0x0104;
const RES_XML_RESOURCE_MAP_TYPE: u16 = 0x0180;

const CHUNK_HEADER_SIZE: usize = 8;
const STRING_POOL_HEADER_SIZE: usize = 28;
const XML_NODE_HEADER_SIZE: usize = 16;
const XML_ATTR_EXT_SIZE: usize = 20;
const XML_ATTRIBUTE_SIZE: usize = 20;
const NO_INDEX: u32 = 0xffff_ffff;
const UTF8_FLAG: u32 = 1 << 8;

const TYPE_REFERENCE: u8 = 0x01;
const TYPE_ATTRIBUTE: u8 = 0x02;
const TYPE_STRING: u8 = 0x03;
const TYPE_FLOAT: u8 = 0x04;
const TYPE_DIMENSION: u8 = 0x05;
const TYPE_FRACTION: u8 = 0x06;
const TYPE_INT_DEC: u8 = 0x10;
const TYPE_INT_HEX: u8 = 0x11;
const TYPE_INT_BOOLEAN: u8 = 0x12;
const TYPE_INT_COLOR_ARGB8: u8 = 0x1c;
const TYPE_INT_COLOR_RGB4: u8 = 0x1f;

#[derive(Debug)]
struct StringPool {
    strings: Vec<String>,
}

impl StringPool {
    fn get(&self, index: u32) -> Option<&str> {
        if index == NO_INDEX {
            return None;
        }

        self.strings.get(index as usize).map(String::as_str)
    }
}

#[derive(Debug)]
struct Attribute {
    name: String,
    value: String,
}

#[derive(Debug)]
struct StartElement {
    name: String,
    attributes: Vec<Attribute>,
}

#[derive(Debug)]
struct ComponentState {
    tag: String,
    name: String,
    has_main_action: bool,
    has_launcher_category: bool,
    inside_intent_filter: bool,
    launcher: bool,
}

pub fn inspect_manifest(bytes: &[u8]) -> ManifestInfo {
    let first_non_whitespace = bytes
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .unwrap_or(bytes.len());
    let visible = &bytes[first_non_whitespace..];

    if visible.starts_with(b"<?xml") || visible.starts_with(b"<manifest") {
        return empty_manifest(ManifestFormat::PlainXml, bytes.len());
    }

    if bytes.len() < CHUNK_HEADER_SIZE {
        return empty_manifest(ManifestFormat::Unknown, bytes.len());
    }

    let chunk_type = read_u16(bytes, 0);
    let header_size = read_u16(bytes, 2);
    let declared_size = read_u32(bytes, 4);

    let mut info = empty_manifest(
        if chunk_type == RES_XML_TYPE {
            ManifestFormat::AndroidBinaryXml
        } else {
            ManifestFormat::Unknown
        },
        bytes.len(),
    );
    info.chunk_type = Some(chunk_type);
    info.header_size = Some(header_size);
    info.declared_size = Some(declared_size);

    if chunk_type != RES_XML_TYPE {
        info.warnings
            .push(format!("tipo de chunk raiz inesperado: {chunk_type:#06x}"));
        return info;
    }

    if usize::from(header_size) < CHUNK_HEADER_SIZE {
        info.warnings.push(format!(
            "header_size do XML é menor que {CHUNK_HEADER_SIZE}: {header_size}"
        ));
        return info;
    }

    if declared_size < u32::from(header_size) {
        info.warnings.push(format!(
            "tamanho declarado {declared_size} é menor que o cabeçalho {header_size}"
        ));
        return info;
    }

    parse_binary_xml(bytes, &mut info);
    info
}

fn empty_manifest(format: ManifestFormat, bytes_sampled: usize) -> ManifestInfo {
    ManifestInfo {
        format,
        chunk_type: None,
        header_size: None,
        declared_size: None,
        bytes_sampled,
        package_name: None,
        version_code: None,
        version_name: None,
        min_sdk: None,
        target_sdk: None,
        application_name: None,
        launcher_activity: None,
        permissions: Vec::new(),
        activities: Vec::new(),
        warnings: Vec::new(),
    }
}

fn parse_binary_xml(bytes: &[u8], info: &mut ManifestInfo) {
    let declared_size = info.declared_size.unwrap_or(bytes.len() as u32) as usize;
    let document_end = declared_size.min(bytes.len());

    if declared_size > bytes.len() {
        info.warnings.push(format!(
            "manifesto truncado: declara {declared_size} bytes, mas apenas {} foram lidos",
            bytes.len()
        ));
    }

    let mut offset = usize::from(info.header_size.unwrap_or(CHUNK_HEADER_SIZE as u16));
    let mut string_pool: Option<StringPool> = None;
    let mut component: Option<ComponentState> = None;

    while offset < document_end {
        if document_end - offset < CHUNK_HEADER_SIZE {
            info.warnings
                .push(format!("chunk incompleto no offset {offset:#x}"));
            break;
        }

        let chunk_type = read_u16(bytes, offset);
        let header_size = usize::from(read_u16(bytes, offset + 2));
        let chunk_size = read_u32(bytes, offset + 4) as usize;

        if header_size < CHUNK_HEADER_SIZE || chunk_size < header_size {
            info.warnings.push(format!(
                "chunk inválido em {offset:#x}: tipo={chunk_type:#06x}, header={header_size}, size={chunk_size}"
            ));
            break;
        }

        let Some(chunk_end) = offset.checked_add(chunk_size) else {
            info.warnings
                .push(format!("overflow no tamanho do chunk em {offset:#x}"));
            break;
        };

        if chunk_end > document_end {
            info.warnings.push(format!(
                "chunk {chunk_type:#06x} ultrapassa o fim do manifesto: {chunk_end:#x} > {document_end:#x}"
            ));
            break;
        }

        match chunk_type {
            RES_STRING_POOL_TYPE => match parse_string_pool(bytes, offset, chunk_end) {
                Ok(pool) => string_pool = Some(pool),
                Err(error) => info.warnings.push(error),
            },
            RES_XML_START_ELEMENT_TYPE => {
                if let Some(pool) = string_pool.as_ref() {
                    match parse_start_element(bytes, offset, chunk_end, header_size, pool) {
                        Ok(element) => {
                            process_start_element(element, info, &mut component);
                        }
                        Err(error) => info.warnings.push(error),
                    }
                } else {
                    info.warnings.push(format!(
                        "elemento XML encontrado antes do string pool em {offset:#x}"
                    ));
                }
            }
            RES_XML_END_ELEMENT_TYPE => {
                if let Some(pool) = string_pool.as_ref() {
                    match parse_end_element_name(bytes, offset, chunk_end, header_size, pool) {
                        Ok(name) => process_end_element(&name, info, &mut component),
                        Err(error) => info.warnings.push(error),
                    }
                }
            }
            RES_XML_START_NAMESPACE_TYPE
            | RES_XML_END_NAMESPACE_TYPE
            | RES_XML_CDATA_TYPE
            | RES_XML_RESOURCE_MAP_TYPE => {}
            _ => info.warnings.push(format!(
                "chunk AXML desconhecido {chunk_type:#06x} em {offset:#x}; ignorado"
            )),
        }

        offset = chunk_end;
    }

    if let Some(component) = component.take() {
        finish_component(component, info);
    }
}

fn process_start_element(
    element: StartElement,
    info: &mut ManifestInfo,
    component: &mut Option<ComponentState>,
) {
    match element.name.as_str() {
        "manifest" => {
            info.package_name = attribute_value(&element.attributes, "package").map(str::to_owned);
            info.version_code =
                attribute_value(&element.attributes, "versionCode").map(str::to_owned);
            info.version_name =
                attribute_value(&element.attributes, "versionName").map(str::to_owned);
        }
        "uses-sdk" => {
            info.min_sdk = attribute_value(&element.attributes, "minSdkVersion").map(str::to_owned);
            info.target_sdk =
                attribute_value(&element.attributes, "targetSdkVersion").map(str::to_owned);
        }
        "uses-permission" | "uses-permission-sdk-23" | "uses-permission-sdk-m" => {
            if let Some(permission) = attribute_value(&element.attributes, "name")
                && !info.permissions.iter().any(|item| item == permission)
            {
                info.permissions.push(permission.to_owned());
            }
        }
        "application" => {
            info.application_name = attribute_value(&element.attributes, "name")
                .map(|name| normalize_class_name(info.package_name.as_deref(), name));
        }
        "activity" | "activity-alias" => {
            if let Some(previous) = component.take() {
                finish_component(previous, info);
            }

            if let Some(name) = attribute_value(&element.attributes, "name") {
                let normalized = normalize_class_name(info.package_name.as_deref(), name);
                if !info.activities.iter().any(|item| item == &normalized) {
                    info.activities.push(normalized.clone());
                }
                *component = Some(ComponentState {
                    tag: element.name,
                    name: normalized,
                    has_main_action: false,
                    has_launcher_category: false,
                    inside_intent_filter: false,
                    launcher: false,
                });
            }
        }
        "intent-filter" => {
            if let Some(component) = component.as_mut() {
                component.inside_intent_filter = true;
                component.has_main_action = false;
                component.has_launcher_category = false;
            }
        }
        "action" => {
            if let Some(component) = component.as_mut()
                && component.inside_intent_filter
                && attribute_value(&element.attributes, "name")
                    == Some("android.intent.action.MAIN")
            {
                component.has_main_action = true;
            }
        }
        "category" => {
            if let Some(component) = component.as_mut()
                && component.inside_intent_filter
                && attribute_value(&element.attributes, "name")
                    == Some("android.intent.category.LAUNCHER")
            {
                component.has_launcher_category = true;
            }
        }
        _ => {}
    }
}

fn process_end_element(
    name: &str,
    info: &mut ManifestInfo,
    component: &mut Option<ComponentState>,
) {
    match name {
        "intent-filter" => {
            if let Some(component) = component.as_mut() {
                if component.inside_intent_filter
                    && component.has_main_action
                    && component.has_launcher_category
                {
                    component.launcher = true;
                }
                component.inside_intent_filter = false;
            }
        }
        "activity" | "activity-alias" => {
            if component
                .as_ref()
                .is_some_and(|current| current.tag == name)
                && let Some(current) = component.take()
            {
                finish_component(current, info);
            }
        }
        _ => {}
    }
}

fn finish_component(component: ComponentState, info: &mut ManifestInfo) {
    if component.launcher && info.launcher_activity.is_none() {
        info.launcher_activity = Some(component.name);
    }
}

fn attribute_value<'a>(attributes: &'a [Attribute], name: &str) -> Option<&'a str> {
    attributes
        .iter()
        .find(|attribute| attribute.name == name)
        .map(|attribute| attribute.value.as_str())
}

fn normalize_class_name(package: Option<&str>, name: &str) -> String {
    if name.starts_with('.') {
        return package.map_or_else(
            || name.trim_start_matches('.').to_owned(),
            |package| format!("{package}{name}"),
        );
    }

    if name.contains('.') {
        return name.to_owned();
    }

    package.map_or_else(|| name.to_owned(), |package| format!("{package}.{name}"))
}

fn parse_string_pool(
    bytes: &[u8],
    chunk_start: usize,
    chunk_end: usize,
) -> Result<StringPool, String> {
    if chunk_end - chunk_start < STRING_POOL_HEADER_SIZE {
        return Err(format!(
            "string pool curto em {chunk_start:#x}: {} bytes",
            chunk_end - chunk_start
        ));
    }

    let header_size = usize::from(read_u16(bytes, chunk_start + 2));
    if header_size < STRING_POOL_HEADER_SIZE || chunk_start + header_size > chunk_end {
        return Err(format!(
            "header inválido do string pool em {chunk_start:#x}: {header_size}"
        ));
    }

    let string_count = read_u32(bytes, chunk_start + 8) as usize;
    let flags = read_u32(bytes, chunk_start + 16);
    let strings_start = read_u32(bytes, chunk_start + 20) as usize;
    let offsets_start = chunk_start + header_size;
    let Some(offsets_end) = string_count
        .checked_mul(4)
        .and_then(|size| offsets_start.checked_add(size))
    else {
        return Err("overflow calculando a tabela de offsets do string pool".to_owned());
    };

    if offsets_end > chunk_end {
        return Err(format!(
            "tabela de offsets do string pool ultrapassa o chunk em {chunk_start:#x}"
        ));
    }

    let Some(strings_base) = chunk_start.checked_add(strings_start) else {
        return Err("overflow calculando o início das strings".to_owned());
    };
    if strings_base > chunk_end {
        return Err(format!(
            "área de strings começa fora do chunk em {chunk_start:#x}"
        ));
    }

    let utf8 = flags & UTF8_FLAG != 0;
    let mut strings = Vec::with_capacity(string_count);

    for index in 0..string_count {
        let relative_offset = read_u32(bytes, offsets_start + index * 4) as usize;
        let Some(string_offset) = strings_base.checked_add(relative_offset) else {
            return Err(format!("overflow no offset da string {index}"));
        };
        if string_offset >= chunk_end {
            return Err(format!(
                "string {index} aponta para fora do pool: {string_offset:#x}"
            ));
        }

        let value = if utf8 {
            decode_utf8_string(bytes, string_offset, chunk_end)
        } else {
            decode_utf16_string(bytes, string_offset, chunk_end)
        }
        .map_err(|error| format!("string {index}: {error}"))?;
        strings.push(value);
    }

    Ok(StringPool { strings })
}

fn decode_utf8_string(bytes: &[u8], offset: usize, end: usize) -> Result<String, String> {
    let (_, after_utf16_length) = decode_length8(bytes, offset, end)?;
    let (byte_length, data_start) = decode_length8(bytes, after_utf16_length, end)?;
    let Some(data_end) = data_start.checked_add(byte_length) else {
        return Err("overflow no comprimento UTF-8".to_owned());
    };
    if data_end > end {
        return Err("string UTF-8 ultrapassa o fim do pool".to_owned());
    }

    Ok(String::from_utf8_lossy(&bytes[data_start..data_end]).into_owned())
}

fn decode_length8(bytes: &[u8], offset: usize, end: usize) -> Result<(usize, usize), String> {
    if offset >= end {
        return Err("comprimento UTF-8 ausente".to_owned());
    }

    let first = bytes[offset];
    if first & 0x80 == 0 {
        return Ok((usize::from(first), offset + 1));
    }

    if offset + 1 >= end {
        return Err("comprimento UTF-8 de dois bytes incompleto".to_owned());
    }

    let length = (usize::from(first & 0x7f) << 8) | usize::from(bytes[offset + 1]);
    Ok((length, offset + 2))
}

fn decode_utf16_string(bytes: &[u8], offset: usize, end: usize) -> Result<String, String> {
    let (unit_count, data_start) = decode_length16(bytes, offset, end)?;
    let byte_count = unit_count
        .checked_mul(2)
        .ok_or_else(|| "overflow no comprimento UTF-16".to_owned())?;
    let data_end = data_start
        .checked_add(byte_count)
        .ok_or_else(|| "overflow no fim da string UTF-16".to_owned())?;
    if data_end > end {
        return Err("string UTF-16 ultrapassa o fim do pool".to_owned());
    }

    let mut units = Vec::with_capacity(unit_count);
    for chunk in bytes[data_start..data_end].chunks_exact(2) {
        units.push(u16::from_le_bytes([chunk[0], chunk[1]]));
    }
    Ok(String::from_utf16_lossy(&units))
}

fn decode_length16(bytes: &[u8], offset: usize, end: usize) -> Result<(usize, usize), String> {
    if offset + 2 > end {
        return Err("comprimento UTF-16 ausente".to_owned());
    }

    let first = read_u16(bytes, offset);
    if first & 0x8000 == 0 {
        return Ok((usize::from(first), offset + 2));
    }

    if offset + 4 > end {
        return Err("comprimento UTF-16 de quatro bytes incompleto".to_owned());
    }

    let second = read_u16(bytes, offset + 2);
    let length = (usize::from(first & 0x7fff) << 16) | usize::from(second);
    Ok((length, offset + 4))
}

fn parse_start_element(
    bytes: &[u8],
    chunk_start: usize,
    chunk_end: usize,
    header_size: usize,
    pool: &StringPool,
) -> Result<StartElement, String> {
    if header_size < XML_NODE_HEADER_SIZE {
        return Err(format!(
            "header de start element curto em {chunk_start:#x}: {header_size}"
        ));
    }

    let ext_start = chunk_start + header_size;
    if ext_start + XML_ATTR_EXT_SIZE > chunk_end {
        return Err(format!(
            "extensão de start element incompleta em {chunk_start:#x}"
        ));
    }

    let name_index = read_u32(bytes, ext_start + 4);
    let name = pool
        .get(name_index)
        .ok_or_else(|| format!("índice de nome inválido no elemento: {name_index}"))?
        .to_owned();
    let attribute_start = usize::from(read_u16(bytes, ext_start + 8));
    let attribute_size = usize::from(read_u16(bytes, ext_start + 10));
    let attribute_count = usize::from(read_u16(bytes, ext_start + 12));

    if attribute_size < XML_ATTRIBUTE_SIZE {
        return Err(format!(
            "attribute_size inválido em {chunk_start:#x}: {attribute_size}"
        ));
    }

    let attributes_start = ext_start
        .checked_add(attribute_start)
        .ok_or_else(|| "overflow no início dos atributos".to_owned())?;
    let attributes_end = attribute_count
        .checked_mul(attribute_size)
        .and_then(|size| attributes_start.checked_add(size))
        .ok_or_else(|| "overflow no tamanho dos atributos".to_owned())?;

    if attributes_end > chunk_end {
        return Err(format!(
            "atributos de {name} ultrapassam o chunk em {chunk_start:#x}"
        ));
    }

    let mut attributes = Vec::with_capacity(attribute_count);
    for index in 0..attribute_count {
        let offset = attributes_start + index * attribute_size;
        let name_index = read_u32(bytes, offset + 4);
        let raw_value_index = read_u32(bytes, offset + 8);
        let value_size = read_u16(bytes, offset + 12);
        let data_type = bytes[offset + 15];
        let data = read_u32(bytes, offset + 16);

        if value_size < 8 {
            return Err(format!(
                "Res_value curto no atributo {index} de {name}: {value_size}"
            ));
        }

        let attribute_name = pool
            .get(name_index)
            .ok_or_else(|| format!("índice de nome de atributo inválido: {name_index}"))?
            .to_owned();
        let value = decode_typed_value(pool, raw_value_index, data_type, data);
        attributes.push(Attribute {
            name: attribute_name,
            value,
        });
    }

    Ok(StartElement { name, attributes })
}

fn parse_end_element_name(
    bytes: &[u8],
    chunk_start: usize,
    chunk_end: usize,
    header_size: usize,
    pool: &StringPool,
) -> Result<String, String> {
    if header_size < XML_NODE_HEADER_SIZE {
        return Err(format!(
            "header de end element curto em {chunk_start:#x}: {header_size}"
        ));
    }

    let ext_start = chunk_start + header_size;
    if ext_start + 8 > chunk_end {
        return Err(format!(
            "extensão de end element incompleta em {chunk_start:#x}"
        ));
    }

    let name_index = read_u32(bytes, ext_start + 4);
    pool.get(name_index)
        .map(str::to_owned)
        .ok_or_else(|| format!("índice de nome inválido no end element: {name_index}"))
}

fn decode_typed_value(pool: &StringPool, raw_value_index: u32, data_type: u8, data: u32) -> String {
    if let Some(raw) = pool.get(raw_value_index) {
        return raw.to_owned();
    }

    match data_type {
        TYPE_STRING => pool
            .get(data)
            .map_or_else(|| format!("<string:{data}>"), str::to_owned),
        TYPE_REFERENCE => format!("@0x{data:08x}"),
        TYPE_ATTRIBUTE => format!("?0x{data:08x}"),
        TYPE_FLOAT => f32::from_bits(data).to_string(),
        TYPE_DIMENSION => format!("<dimension:0x{data:08x}>"),
        TYPE_FRACTION => format!("<fraction:0x{data:08x}>"),
        TYPE_INT_DEC => data.to_string(),
        TYPE_INT_HEX => format!("0x{data:x}"),
        TYPE_INT_BOOLEAN => (data != 0).to_string(),
        TYPE_INT_COLOR_ARGB8..=TYPE_INT_COLOR_RGB4 => {
            format!("#{data:08x}")
        }
        _ => format!("<type:0x{data_type:02x},data:0x{data:08x}>"),
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

    #[test]
    fn decodes_short_and_long_utf8_lengths() {
        let short = [5_u8];
        assert_eq!(decode_length8(&short, 0, short.len()).unwrap(), (5, 1));

        let long = [0x81_u8, 0x02];
        assert_eq!(decode_length8(&long, 0, long.len()).unwrap(), (258, 2));
    }

    #[test]
    fn normalizes_android_component_names() {
        assert_eq!(
            normalize_class_name(Some("dev.winedroid"), ".MainActivity"),
            "dev.winedroid.MainActivity"
        );
        assert_eq!(
            normalize_class_name(Some("dev.winedroid"), "MainActivity"),
            "dev.winedroid.MainActivity"
        );
        assert_eq!(
            normalize_class_name(Some("dev.winedroid"), "other.app.Main"),
            "other.app.Main"
        );
    }

    #[test]
    fn decodes_typed_android_values() {
        let pool = StringPool {
            strings: vec!["hello".to_owned()],
        };

        assert_eq!(decode_typed_value(&pool, NO_INDEX, TYPE_STRING, 0), "hello");
        assert_eq!(
            decode_typed_value(&pool, NO_INDEX, TYPE_INT_BOOLEAN, 1),
            "true"
        );
        assert_eq!(decode_typed_value(&pool, NO_INDEX, TYPE_INT_DEC, 35), "35");
    }
}
