use std::{fs::File, io::Read, path::Path, sync::Arc};

use winedroid_core::{DexIndex, parse_dex_index};
use zip::ZipArchive;

const MAX_DEX_SIZE: u64 = 256 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DexFieldReference {
    pub index: u16,
    pub descriptor: String,
    pub field_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DexMethodBody {
    pub dex_path: String,
    pub descriptor: String,
    pub access_flags: u32,
    pub registers_size: u16,
    pub ins_size: u16,
    pub outs_size: u16,
    pub tries_size: u16,
    pub instructions: Vec<u16>,
    pub field_table: Arc<[DexFieldReference]>,
}

pub fn find_method_in_dex(
    dex_path: &str,
    bytes: &[u8],
    descriptor: &str,
) -> Result<Option<DexMethodBody>, String> {
    let index = parse_dex_index(dex_path, bytes)?;
    find_method_in_index(dex_path, bytes, &index, descriptor)
}

pub fn find_method_in_apk(
    apk_path: &Path,
    descriptor: &str,
) -> Result<Option<DexMethodBody>, String> {
    let file = File::open(apk_path)
        .map_err(|error| format!("não foi possível abrir {}: {error}", apk_path.display()))?;
    let mut archive = ZipArchive::new(file)
        .map_err(|error| format!("APK inválido {}: {error}", apk_path.display()))?;

    let mut dex_names = Vec::new();
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|error| format!("erro lendo entrada ZIP #{index}: {error}"))?;
        let name = entry.name().to_owned();
        if is_dex_name(&name) {
            dex_names.push(name);
        }
    }
    dex_names.sort_by_key(|name| dex_number(name));

    for name in dex_names {
        let bytes = read_zip_entry(&mut archive, &name)?;
        if let Some(method) = find_method_in_dex(&name, &bytes, descriptor)? {
            return Ok(Some(method));
        }
    }

    Ok(None)
}

pub fn scan_apk_methods(apk_path: &Path, limit: usize) -> Result<Vec<DexMethodBody>, String> {
    let file = File::open(apk_path)
        .map_err(|error| format!("não foi possível abrir {}: {error}", apk_path.display()))?;
    let mut archive = ZipArchive::new(file)
        .map_err(|error| format!("APK inválido {}: {error}", apk_path.display()))?;

    let mut dex_names = Vec::new();
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|error| format!("erro lendo entrada ZIP #{index}: {error}"))?;
        let name = entry.name().to_owned();
        if is_dex_name(&name) {
            dex_names.push(name);
        }
    }
    dex_names.sort_by_key(|name| dex_number(name));

    let mut methods = Vec::new();
    for name in dex_names {
        let bytes = read_zip_entry(&mut archive, &name)?;
        let index = parse_dex_index(&name, &bytes)?;
        collect_zero_argument_methods(&name, &bytes, &index, limit, &mut methods)?;
        if methods.len() >= limit {
            break;
        }
    }

    methods.truncate(limit);
    Ok(methods)
}

fn find_method_in_index(
    dex_path: &str,
    bytes: &[u8],
    index: &DexIndex,
    descriptor: &str,
) -> Result<Option<DexMethodBody>, String> {
    let mut found = None;
    walk_methods(dex_path, bytes, index, |method| {
        if method.descriptor == descriptor {
            found = Some(method);
            return false;
        }
        true
    })?;
    Ok(found)
}

fn collect_zero_argument_methods(
    dex_path: &str,
    bytes: &[u8],
    index: &DexIndex,
    limit: usize,
    output: &mut Vec<DexMethodBody>,
) -> Result<(), String> {
    walk_methods(dex_path, bytes, index, |method| {
        if method.ins_size == 0 && !method.instructions.is_empty() {
            output.push(method);
        }
        output.len() < limit
    })
}

fn walk_methods<F>(
    dex_path: &str,
    bytes: &[u8],
    index: &DexIndex,
    mut visitor: F,
) -> Result<(), String>
where
    F: FnMut(DexMethodBody) -> bool,
{
    let field_table = build_field_table(index)?;

    for class in &index.classes {
        if class.class_data_offset == 0 {
            continue;
        }

        let mut cursor = usize::try_from(class.class_data_offset)
            .map_err(|_| format!("{dex_path}: class_data_off não cabe em usize"))?;

        let static_fields = read_uleb_cursor(bytes, &mut cursor, "static_fields_size")?;
        let instance_fields = read_uleb_cursor(bytes, &mut cursor, "instance_fields_size")?;
        let direct_methods = read_uleb_cursor(bytes, &mut cursor, "direct_methods_size")?;
        let virtual_methods = read_uleb_cursor(bytes, &mut cursor, "virtual_methods_size")?;

        skip_fields(bytes, &mut cursor, static_fields)?;
        skip_fields(bytes, &mut cursor, instance_fields)?;

        if !walk_encoded_method_list(
            dex_path,
            bytes,
            &mut cursor,
            direct_methods,
            index,
            &field_table,
            &mut visitor,
        )? {
            return Ok(());
        }

        if !walk_encoded_method_list(
            dex_path,
            bytes,
            &mut cursor,
            virtual_methods,
            index,
            &field_table,
            &mut visitor,
        )? {
            return Ok(());
        }
    }

    Ok(())
}

fn skip_fields(bytes: &[u8], cursor: &mut usize, count: u32) -> Result<(), String> {
    for _ in 0..count {
        let _field_index_difference = read_uleb_cursor(bytes, cursor, "field_idx_diff")?;
        let _access_flags = read_uleb_cursor(bytes, cursor, "field access_flags")?;
    }
    Ok(())
}

fn walk_encoded_method_list<F>(
    dex_path: &str,
    bytes: &[u8],
    cursor: &mut usize,
    count: u32,
    index: &DexIndex,
    field_table: &Arc<[DexFieldReference]>,
    visitor: &mut F,
) -> Result<bool, String>
where
    F: FnMut(DexMethodBody) -> bool,
{
    let mut method_index = 0_u32;

    for item in 0..count {
        let difference = read_uleb_cursor(bytes, cursor, "method_idx_diff")?;
        method_index = method_index
            .checked_add(difference)
            .ok_or_else(|| format!("{dex_path}: overflow no method_idx do item {item}"))?;
        let access_flags = read_uleb_cursor(bytes, cursor, "method access_flags")?;
        let code_offset = read_uleb_cursor(bytes, cursor, "method code_off")?;

        let method_position = usize::try_from(method_index)
            .map_err(|_| format!("{dex_path}: method_idx não cabe em usize"))?;
        let method = index.methods.get(method_position).ok_or_else(|| {
            format!(
                "{dex_path}: method_idx #{method_position} fora da tabela de {} métodos",
                index.methods.len()
            )
        })?;

        if code_offset == 0 {
            continue;
        }

        let body = parse_code_item(
            dex_path,
            bytes,
            code_offset,
            method.descriptor.clone(),
            access_flags,
            Arc::clone(field_table),
        )?;

        if !visitor(body) {
            return Ok(false);
        }
    }

    Ok(true)
}

fn parse_code_item(
    dex_path: &str,
    bytes: &[u8],
    code_offset: u32,
    descriptor: String,
    access_flags: u32,
    field_table: Arc<[DexFieldReference]>,
) -> Result<DexMethodBody, String> {
    let offset = usize::try_from(code_offset)
        .map_err(|_| format!("{dex_path}: code_off não cabe em usize"))?;
    let header = checked_slice(bytes, offset, 16, "code_item header")?;

    let registers_size = u16::from_le_bytes([header[0], header[1]]);
    let ins_size = u16::from_le_bytes([header[2], header[3]]);
    let outs_size = u16::from_le_bytes([header[4], header[5]]);
    let tries_size = u16::from_le_bytes([header[6], header[7]]);
    let instruction_count = u32::from_le_bytes([header[12], header[13], header[14], header[15]]);
    let instruction_count = usize::try_from(instruction_count)
        .map_err(|_| format!("{dex_path}: insns_size não cabe em usize"))?;
    let instruction_bytes = instruction_count
        .checked_mul(2)
        .ok_or_else(|| format!("{dex_path}: overflow no tamanho do bytecode"))?;
    let instruction_offset = offset
        .checked_add(16)
        .ok_or_else(|| format!("{dex_path}: overflow no início do bytecode"))?;
    let raw = checked_slice(
        bytes,
        instruction_offset,
        instruction_bytes,
        "code_item instructions",
    )?;

    let mut instructions = Vec::with_capacity(instruction_count);
    for pair in raw.chunks_exact(2) {
        instructions.push(u16::from_le_bytes([pair[0], pair[1]]));
    }

    Ok(DexMethodBody {
        dex_path: dex_path.to_owned(),
        descriptor,
        access_flags,
        registers_size,
        ins_size,
        outs_size,
        tries_size,
        instructions,
        field_table,
    })
}

fn build_field_table(index: &DexIndex) -> Result<Arc<[DexFieldReference]>, String> {
    let mut fields = Vec::with_capacity(index.fields.len());

    for (position, field) in index.fields.iter().enumerate() {
        let field_index = u16::try_from(position)
            .map_err(|_| format!("field_id #{position} excede o índice Dalvik de 16 bits"))?;
        fields.push(DexFieldReference {
            index: field_index,
            descriptor: field.descriptor.clone(),
            field_type: field.field_type.clone(),
        });
    }

    Ok(Arc::from(fields))
}

fn read_zip_entry(archive: &mut ZipArchive<File>, name: &str) -> Result<Vec<u8>, String> {
    let mut entry = archive
        .by_name(name)
        .map_err(|error| format!("não foi possível abrir {name} no APK: {error}"))?;

    if entry.size() > MAX_DEX_SIZE {
        return Err(format!(
            "{name}: DEX excede o limite de {} MiB",
            MAX_DEX_SIZE / 1024 / 1024
        ));
    }

    let capacity =
        usize::try_from(entry.size()).map_err(|_| format!("{name}: tamanho não cabe em usize"))?;
    let mut bytes = Vec::with_capacity(capacity);
    entry
        .read_to_end(&mut bytes)
        .map_err(|error| format!("erro lendo {name}: {error}"))?;
    Ok(bytes)
}

fn is_dex_name(name: &str) -> bool {
    if name == "classes.dex" {
        return true;
    }

    name.strip_prefix("classes")
        .and_then(|tail| tail.strip_suffix(".dex"))
        .is_some_and(|number| {
            !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit())
        })
}

fn dex_number(name: &str) -> u32 {
    if name == "classes.dex" {
        return 1;
    }

    name.strip_prefix("classes")
        .and_then(|tail| tail.strip_suffix(".dex"))
        .and_then(|number| number.parse::<u32>().ok())
        .unwrap_or(u32::MAX)
}

fn read_uleb_cursor(bytes: &[u8], cursor: &mut usize, name: &str) -> Result<u32, String> {
    let (value, width) =
        read_uleb128(bytes, *cursor).map_err(|error| format!("{name}: {error}"))?;
    *cursor = (*cursor)
        .checked_add(width)
        .ok_or_else(|| format!("overflow avançando {name}"))?;
    Ok(value)
}

fn read_uleb128(bytes: &[u8], offset: usize) -> Result<(u32, usize), String> {
    let mut result = 0_u32;

    for index in 0..5 {
        let position = offset
            .checked_add(index)
            .ok_or_else(|| "overflow no ULEB128".to_owned())?;
        let byte = *bytes
            .get(position)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_multidex_names() {
        assert!(is_dex_name("classes.dex"));
        assert!(is_dex_name("classes2.dex"));
        assert!(is_dex_name("classes20.dex"));
        assert!(!is_dex_name("classesx.dex"));
        assert!(!is_dex_name("assets/classes.dex"));
    }

    #[test]
    fn sorts_multidex_numerically() {
        let mut names = vec![
            "classes10.dex".to_owned(),
            "classes2.dex".to_owned(),
            "classes.dex".to_owned(),
        ];
        names.sort_by_key(|name| dex_number(name));
        assert_eq!(names, ["classes.dex", "classes2.dex", "classes10.dex"]);
    }

    #[test]
    fn builds_resolved_field_table() {
        let index = DexIndex {
            strings: Vec::new(),
            types: Vec::new(),
            protos: Vec::new(),
            fields: vec![winedroid_core::DexField {
                class: "LTest;".to_owned(),
                field_type: "I".to_owned(),
                name: "counter".to_owned(),
                descriptor: "LTest;->counter:I".to_owned(),
            }],
            methods: Vec::new(),
            classes: Vec::new(),
        };

        let fields = build_field_table(&index).unwrap();
        assert_eq!(fields[0].index, 0);
        assert_eq!(fields[0].descriptor, "LTest;->counter:I");
        assert_eq!(fields[0].field_type, "I");
    }

    #[test]
    fn reads_maximum_uleb128() {
        assert_eq!(
            read_uleb128(&[0xff, 0xff, 0xff, 0xff, 0x0f], 0).unwrap(),
            (u32::MAX, 5)
        );
    }
}
