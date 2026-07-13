use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use winedroid_core::{DexIndex, parse_dex_index};
use zip::ZipArchive;

const MAX_DEX_SIZE: u64 = 256 * 1024 * 1024;
const ACCESS_STATIC: u32 = 0x0008;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootstrapMethod {
    pub dex_path: String,
    pub descriptor: String,
    pub access_flags: u32,
    pub registers_size: u16,
    pub ins_size: u16,
    pub outs_size: u16,
    pub instructions: Vec<u16>,
    pub methods: Arc<[String]>,
    pub fields: Arc<[String]>,
    pub field_types: Arc<[String]>,
    pub strings: Arc<[String]>,
    pub types: Arc<[String]>,
}

impl BootstrapMethod {
    #[must_use]
    pub fn demo() -> Self {
        Self {
            dex_path: "demo.dex".to_owned(),
            descriptor: "Ldev/winedroid/ObjectDemo;->answer()I".to_owned(),
            access_flags: ACCESS_STATIC,
            registers_size: 3,
            ins_size: 0,
            outs_size: 0,
            instructions: vec![
                0x0022, 0, // new-instance v0, type@0
                0x0113, 42, // const/16 v1, 42
                0x0159, 0, // iput v1, v0, field@0
                0x0252, 0,      // iget v2, v0, field@0
                0x020f, // return v2
            ],
            methods: Arc::from(Vec::<String>::new()),
            fields: Arc::from(vec!["Ldev/winedroid/ObjectDemo;->value:I".to_owned()]),
            field_types: Arc::from(vec!["I".to_owned()]),
            strings: Arc::from(Vec::<String>::new()),
            types: Arc::from(vec!["Ldev/winedroid/ObjectDemo;".to_owned()]),
        }
    }

    #[must_use]
    pub fn is_static(&self) -> bool {
        self.access_flags & ACCESS_STATIC != 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnsupportedInstruction {
    pub pc: usize,
    pub opcode: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootstrapReport {
    pub descriptor: String,
    pub registers_size: u16,
    pub ins_size: u16,
    pub instruction_count: usize,
    pub referenced_methods: Vec<String>,
    pub referenced_fields: Vec<String>,
    pub referenced_strings: Vec<String>,
    pub unsupported: Vec<UnsupportedInstruction>,
}

impl BootstrapReport {
    #[must_use]
    pub fn compilable(&self) -> bool {
        self.unsupported.is_empty()
    }
}

#[derive(Debug)]
pub enum BootstrapError {
    Apk(String),
    MethodNotFound(String),
    Truncated {
        pc: usize,
        opcode: u8,
    },
    Unsupported {
        pc: usize,
        opcode: u8,
    },
    InvalidRegister {
        pc: usize,
        register: usize,
        register_count: usize,
    },
    InvalidReference {
        pc: usize,
        kind: &'static str,
        index: usize,
        count: usize,
    },
    InvalidBranch {
        pc: usize,
        target: i64,
    },
    Io(std::io::Error),
    Clang {
        status: Option<i32>,
        stderr: String,
    },
}

impl fmt::Display for BootstrapError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Apk(error) => write!(formatter, "{error}"),
            Self::MethodNotFound(method) => write!(formatter, "método não encontrado: {method}"),
            Self::Truncated { pc, opcode } => {
                write!(formatter, "instrução truncada em pc={pc}: {opcode:#04x}")
            }
            Self::Unsupported { pc, opcode } => {
                write!(
                    formatter,
                    "opcode ainda não suportado em pc={pc}: {opcode:#04x}"
                )
            }
            Self::InvalidRegister {
                pc,
                register,
                register_count,
            } => write!(
                formatter,
                "pc={pc}: v{register} não existe; frame possui {register_count}"
            ),
            Self::InvalidReference {
                pc,
                kind,
                index,
                count,
            } => write!(
                formatter,
                "pc={pc}: {kind} #{index} fora da tabela com {count} itens"
            ),
            Self::InvalidBranch { pc, target } => {
                write!(formatter, "pc={pc}: destino de salto inválido: {target}")
            }
            Self::Io(error) => write!(formatter, "{error}"),
            Self::Clang { status, stderr } => {
                write!(formatter, "Clang falhou com status {status:?}:\n{stderr}")
            }
        }
    }
}

impl Error for BootstrapError {}

impl From<std::io::Error> for BootstrapError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

#[derive(Debug, Clone)]
pub struct BootstrapCompiler {
    clang: PathBuf,
}

impl Default for BootstrapCompiler {
    fn default() -> Self {
        Self {
            clang: PathBuf::from("clang"),
        }
    }
}

impl BootstrapCompiler {
    pub fn analyze(&self, method: &BootstrapMethod) -> Result<BootstrapReport, BootstrapError> {
        let starts = instruction_starts(&method.instructions)?;
        let mut referenced_methods = BTreeSet::new();
        let mut referenced_fields = BTreeSet::new();
        let mut referenced_strings = BTreeSet::new();
        let mut unsupported = Vec::new();

        for pc in starts {
            let unit = method.instructions[pc];
            let opcode = (unit & 0xff) as u8;

            if !is_supported_opcode(opcode, unit) {
                unsupported.push(UnsupportedInstruction { pc, opcode });
                continue;
            }

            match opcode {
                0x1a => {
                    let index = usize::from(read_unit(&method.instructions, pc + 1, pc, opcode)?);
                    referenced_strings
                        .insert(get_reference(&method.strings, pc, "string", index)?.to_owned());
                }
                0x1b => {
                    let index =
                        usize::try_from(read_u32(&method.instructions, pc + 1, pc, opcode)?)
                            .map_err(|_| BootstrapError::InvalidReference {
                                pc,
                                kind: "string",
                                index: usize::MAX,
                                count: method.strings.len(),
                            })?;
                    referenced_strings
                        .insert(get_reference(&method.strings, pc, "string", index)?.to_owned());
                }
                0x52..=0x6d => {
                    let index = usize::from(read_unit(&method.instructions, pc + 1, pc, opcode)?);
                    referenced_fields
                        .insert(get_reference(&method.fields, pc, "field", index)?.to_owned());
                }
                0x6e..=0x72 | 0x74..=0x78 => {
                    let index = usize::from(read_unit(&method.instructions, pc + 1, pc, opcode)?);
                    referenced_methods
                        .insert(get_reference(&method.methods, pc, "method", index)?.to_owned());
                }
                _ => {}
            }
        }

        Ok(BootstrapReport {
            descriptor: method.descriptor.clone(),
            registers_size: method.registers_size,
            ins_size: method.ins_size,
            instruction_count: method.instructions.len(),
            referenced_methods: referenced_methods.into_iter().collect(),
            referenced_fields: referenced_fields.into_iter().collect(),
            referenced_strings: referenced_strings.into_iter().collect(),
            unsupported,
        })
    }

    pub fn emit_c(&self, method: &BootstrapMethod) -> Result<String, BootstrapError> {
        let report = self.analyze(method)?;
        if let Some(first) = report.unsupported.first() {
            return Err(BootstrapError::Unsupported {
                pc: first.pc,
                opcode: first.opcode,
            });
        }

        let starts = instruction_starts(&method.instructions)?;
        let start_set: BTreeSet<usize> = starts.iter().copied().collect();
        let mut compact_fields = BTreeMap::new();
        let mut field_slots = Vec::new();

        for &pc in &starts {
            let opcode = (method.instructions[pc] & 0xff) as u8;
            if matches!(opcode, 0x52..=0x6d) {
                let original = usize::from(read_unit(&method.instructions, pc + 1, pc, opcode)?);
                if !compact_fields.contains_key(&original) {
                    let slot = compact_fields.len();
                    compact_fields.insert(original, slot);
                    field_slots.push(original);
                }
            }
        }

        let register_count = usize::from(method.registers_size).max(1);
        let method_count = method.methods.len().max(1);
        let field_count = field_slots.len().max(1);
        let mut source = String::new();

        source.push_str(
            "#include <stdint.h>\n#include <stdio.h>\n#include <stdlib.h>\n#include <string.h>\n\n",
        );
        source.push_str("typedef int64_t wd_value;\n");
        source.push_str(
            "typedef struct { int32_t object; uint32_t field; wd_value value; } wd_ifield;\n\n",
        );
        source.push_str("static int32_t wd_next_object = 1;\n");
        source.push_str("static __attribute__((unused)) wd_value wd_last_result = 0;\n");
        source.push_str("static __attribute__((unused)) wd_value wd_static_fields[");
        source.push_str(&field_count.to_string());
        source.push_str("] = {0};\n");
        source.push_str("static wd_ifield wd_instance_fields[4096];\n");
        source.push_str("static size_t wd_instance_field_count = 0;\n\n");

        emit_string_table(
            &mut source,
            "wd_method_names",
            &method.methods,
            method_count,
        );
        emit_string_table(
            &mut source,
            "wd_field_names",
            &method.fields,
            method.fields.len().max(1),
        );
        emit_string_table(
            &mut source,
            "wd_string_values",
            &method.strings,
            method.strings.len().max(1),
        );
        emit_string_table(
            &mut source,
            "wd_type_names",
            &method.types,
            method.types.len().max(1),
        );

        source.push_str(
            "static int32_t wd_new_object(uint32_t type_index) {\n\
             \tint32_t handle = wd_next_object++;\n\
             \tconst char *name = type_index < (sizeof(wd_type_names) / sizeof(wd_type_names[0])) ? wd_type_names[type_index] : \"<unknown-type>\";\n\
             \tfprintf(stderr, \"[WineDroid] new-instance #%d %s\\n\", handle, name);\n\
             \treturn handle;\n\
             }\n\n",
        );
        source.push_str(
            "static wd_value wd_iget(int32_t object, uint32_t field) {\n\
             \tfor (size_t i = wd_instance_field_count; i > 0; --i) {\n\
             \t\twd_ifield *entry = &wd_instance_fields[i - 1];\n\
             \t\tif (entry->object == object && entry->field == field) { return entry->value; }\n\
             \t}\n\
             \treturn 0;\n\
             }\n\n",
        );
        source.push_str(
            "static void wd_iput(int32_t object, uint32_t field, wd_value value) {\n\
             \tfor (size_t i = wd_instance_field_count; i > 0; --i) {\n\
             \t\twd_ifield *entry = &wd_instance_fields[i - 1];\n\
             \t\tif (entry->object == object && entry->field == field) { entry->value = value; return; }\n\
             \t}\n\
             \tif (wd_instance_field_count >= 4096) { fputs(\"WineDroid: instance field table full\\n\", stderr); exit(102); }\n\
             \twd_instance_fields[wd_instance_field_count++] = (wd_ifield){ object, field, value };\n\
             }\n\n",
        );
        source.push_str(
            "static __attribute__((unused)) wd_value wd_invoke(uint32_t method_index, uint32_t argc, const wd_value *args) {\n\
             \t(void)argc; (void)args;\n\
             \tconst char *name = method_index < (sizeof(wd_method_names) / sizeof(wd_method_names[0])) ? wd_method_names[method_index] : \"<unknown-method>\";\n\
             \tfprintf(stderr, \"[WineDroid] invoke %s\\n\", name);\n\
             \tif (strstr(name, \"isUserUnlocked()Z\") != NULL) { return 1; }\n\
             \tif (strstr(name, \"mkdir()Z\") != NULL) { return 1; }\n\
             \tif (strstr(name, \"exists()Z\") != NULL) { return 0; }\n\
             \tconst char *close = strrchr(name, ')');\n\
             \tif (close != NULL && (close[1] == 'L' || close[1] == '[')) { return wd_new_object(0); }\n\
             \treturn 0;\n\
             }\n\n",
        );
        source.push_str(
            "static __attribute__((unused)) void wd_throw(wd_value value) {\n\
             \tfprintf(stderr, \"[WineDroid] throw handle=%lld\\n\", (long long)value);\n\
             \texit(103);\n\
             }\n\n",
        );

        source.push_str("static wd_value winedroid_method(void) {\n");
        source.push_str(&format!("\twd_value v[{register_count}] = {{0}};\n"));

        if method.ins_size > 0 {
            let incoming_start = register_count.saturating_sub(usize::from(method.ins_size));
            if !method.is_static() {
                source.push_str(&format!(
                    "\tv[{incoming_start}] = wd_new_object(0); /* this */\n"
                ));
            }
        }

        source.push_str("\tgoto L0;\n");

        for (position, &pc) in starts.iter().enumerate() {
            let next = starts.get(position + 1).copied();
            let statement = lower_instruction(
                method,
                pc,
                next,
                &start_set,
                &compact_fields,
                register_count,
            )?;
            source.push_str(&format!("L{pc}:\n{statement}"));
        }

        source.push_str("}\n\n");
        source.push_str(
            "int main(void) {\n\
             \twd_value result = winedroid_method();\n\
             \tprintf(\"%lld\\n\", (long long)result);\n\
             \treturn 0;\n\
             }\n",
        );

        Ok(source)
    }

    pub fn compile(
        &self,
        method: &BootstrapMethod,
        output: &Path,
        emit_c: Option<&Path>,
    ) -> Result<(), BootstrapError> {
        let source = self.emit_c(method)?;

        if let Some(parent) = output.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
        }

        if let Some(path) = emit_c {
            if let Some(parent) = path.parent()
                && !parent.as_os_str().is_empty()
            {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(path, &source)?;
        }

        let temporary = temporary_c_path();
        std::fs::write(&temporary, &source)?;

        let result = Command::new(&self.clang)
            .args([
                "-std=c11", "-O2", "-fPIE", "-pie", "-Wall", "-Wextra", "-Werror", "-o",
            ])
            .arg(output)
            .arg(&temporary)
            .output();

        let _ = std::fs::remove_file(&temporary);
        let result = result?;

        if !result.status.success() {
            return Err(BootstrapError::Clang {
                status: result.status.code(),
                stderr: String::from_utf8_lossy(&result.stderr).into_owned(),
            });
        }

        Ok(())
    }
}

pub fn find_bootstrap_method_in_apk(
    apk_path: &Path,
    descriptor: &str,
) -> Result<Option<BootstrapMethod>, BootstrapError> {
    let file = File::open(apk_path)?;
    let mut archive = ZipArchive::new(file)
        .map_err(|error| BootstrapError::Apk(format!("APK inválido: {error}")))?;
    let mut dex_names = Vec::new();

    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|error| BootstrapError::Apk(format!("entrada ZIP #{index}: {error}")))?;
        let name = entry.name().to_owned();
        if is_dex_name(&name) {
            dex_names.push(name);
        }
    }

    dex_names.sort_by_key(|name| dex_number(name));

    for name in dex_names {
        let bytes = read_zip_entry(&mut archive, &name)?;
        let index = parse_dex_index(&name, &bytes).map_err(BootstrapError::Apk)?;
        if let Some(method) = find_method_in_index(&name, &bytes, &index, descriptor)? {
            return Ok(Some(method));
        }
    }

    Ok(None)
}

fn find_method_in_index(
    dex_path: &str,
    bytes: &[u8],
    index: &DexIndex,
    descriptor: &str,
) -> Result<Option<BootstrapMethod>, BootstrapError> {
    let methods: Arc<[String]> = Arc::from(
        index
            .methods
            .iter()
            .map(|method| method.descriptor.clone())
            .collect::<Vec<_>>(),
    );
    let fields: Arc<[String]> = Arc::from(
        index
            .fields
            .iter()
            .map(|field| field.descriptor.clone())
            .collect::<Vec<_>>(),
    );
    let field_types: Arc<[String]> = Arc::from(
        index
            .fields
            .iter()
            .map(|field| field.field_type.clone())
            .collect::<Vec<_>>(),
    );
    let strings: Arc<[String]> = Arc::from(index.strings.clone());
    let types: Arc<[String]> = Arc::from(index.types.clone());

    for class in &index.classes {
        if class.class_data_offset == 0 {
            continue;
        }

        let mut cursor = usize::try_from(class.class_data_offset)
            .map_err(|_| BootstrapError::Apk("class_data_off inválido".to_owned()))?;
        let static_fields = read_uleb_cursor(bytes, &mut cursor)?;
        let instance_fields = read_uleb_cursor(bytes, &mut cursor)?;
        let direct_methods = read_uleb_cursor(bytes, &mut cursor)?;
        let virtual_methods = read_uleb_cursor(bytes, &mut cursor)?;

        skip_encoded_fields(bytes, &mut cursor, static_fields)?;
        skip_encoded_fields(bytes, &mut cursor, instance_fields)?;

        for count in [direct_methods, virtual_methods] {
            let mut method_index = 0_u32;

            for _ in 0..count {
                method_index = method_index
                    .checked_add(read_uleb_cursor(bytes, &mut cursor)?)
                    .ok_or_else(|| BootstrapError::Apk("method_idx overflow".to_owned()))?;
                let access_flags = read_uleb_cursor(bytes, &mut cursor)?;
                let code_offset = read_uleb_cursor(bytes, &mut cursor)?;
                let method_position = usize::try_from(method_index)
                    .map_err(|_| BootstrapError::Apk("method_idx inválido".to_owned()))?;
                let method = index.methods.get(method_position).ok_or_else(|| {
                    BootstrapError::Apk(format!("method_idx #{method_position} inexistente"))
                })?;

                if method.descriptor != descriptor || code_offset == 0 {
                    continue;
                }

                return Ok(Some(parse_code_item(
                    dex_path,
                    bytes,
                    code_offset,
                    method.descriptor.clone(),
                    access_flags,
                    Arc::clone(&methods),
                    Arc::clone(&fields),
                    Arc::clone(&field_types),
                    Arc::clone(&strings),
                    Arc::clone(&types),
                )?));
            }
        }
    }

    Ok(None)
}

#[allow(clippy::too_many_arguments)]
fn parse_code_item(
    dex_path: &str,
    bytes: &[u8],
    code_offset: u32,
    descriptor: String,
    access_flags: u32,
    methods: Arc<[String]>,
    fields: Arc<[String]>,
    field_types: Arc<[String]>,
    strings: Arc<[String]>,
    types: Arc<[String]>,
) -> Result<BootstrapMethod, BootstrapError> {
    let offset = usize::try_from(code_offset)
        .map_err(|_| BootstrapError::Apk("code_off inválido".to_owned()))?;
    let header = checked_slice(bytes, offset, 16)?;

    let registers_size = u16::from_le_bytes([header[0], header[1]]);
    let ins_size = u16::from_le_bytes([header[2], header[3]]);
    let outs_size = u16::from_le_bytes([header[4], header[5]]);
    let instruction_count = u32::from_le_bytes([header[12], header[13], header[14], header[15]]);
    let instruction_count = usize::try_from(instruction_count)
        .map_err(|_| BootstrapError::Apk("insns_size inválido".to_owned()))?;
    let byte_count = instruction_count
        .checked_mul(2)
        .ok_or_else(|| BootstrapError::Apk("bytecode muito grande".to_owned()))?;
    let start = offset
        .checked_add(16)
        .ok_or_else(|| BootstrapError::Apk("offset de bytecode inválido".to_owned()))?;
    let raw = checked_slice(bytes, start, byte_count)?;
    let instructions = raw
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();

    Ok(BootstrapMethod {
        dex_path: dex_path.to_owned(),
        descriptor,
        access_flags,
        registers_size,
        ins_size,
        outs_size,
        instructions,
        methods,
        fields,
        field_types,
        strings,
        types,
    })
}

fn lower_instruction(
    method: &BootstrapMethod,
    pc: usize,
    next: Option<usize>,
    starts: &BTreeSet<usize>,
    compact_fields: &BTreeMap<usize, usize>,
    register_count: usize,
) -> Result<String, BootstrapError> {
    let unit = method.instructions[pc];
    let opcode = (unit & 0xff) as u8;
    let fallthrough = |statement: String| -> Result<String, BootstrapError> {
        let next = next.ok_or(BootstrapError::Unsupported { pc, opcode })?;
        Ok(format!("\t{statement}\n\tgoto L{next};\n"))
    };

    match opcode {
        0x00 => {
            if unit != 0 {
                return Err(BootstrapError::Unsupported { pc, opcode });
            }
            fallthrough("/* nop */".to_owned())
        }
        0x01 | 0x04 | 0x07 => {
            let destination = usize::from((unit >> 8) & 0x0f);
            let source = usize::from((unit >> 12) & 0x0f);
            validate_register(pc, destination, register_count)?;
            validate_register(pc, source, register_count)?;
            fallthrough(format!("v[{destination}] = v[{source}];"))
        }
        0x02 | 0x05 | 0x08 => {
            let destination = usize::from(unit >> 8);
            let source = usize::from(read_unit(&method.instructions, pc + 1, pc, opcode)?);
            validate_register(pc, destination, register_count)?;
            validate_register(pc, source, register_count)?;
            fallthrough(format!("v[{destination}] = v[{source}];"))
        }
        0x03 | 0x06 | 0x09 => {
            let destination = usize::from(read_unit(&method.instructions, pc + 1, pc, opcode)?);
            let source = usize::from(read_unit(&method.instructions, pc + 2, pc, opcode)?);
            validate_register(pc, destination, register_count)?;
            validate_register(pc, source, register_count)?;
            fallthrough(format!("v[{destination}] = v[{source}];"))
        }
        0x0a..=0x0d => {
            let destination = usize::from(unit >> 8);
            validate_register(pc, destination, register_count)?;
            fallthrough(format!("v[{destination}] = wd_last_result;"))
        }
        0x0e => Ok("\treturn 0;\n".to_owned()),
        0x0f..=0x11 => {
            let source = usize::from(unit >> 8);
            validate_register(pc, source, register_count)?;
            Ok(format!("\treturn v[{source}];\n"))
        }
        0x12 => {
            let destination = usize::from((unit >> 8) & 0x0f);
            let nibble = ((unit >> 12) & 0x0f) as u8;
            let literal = i64::from(((nibble << 4) as i8) >> 4);
            validate_register(pc, destination, register_count)?;
            fallthrough(format!("v[{destination}] = INT64_C({literal});"))
        }
        0x13 => {
            let destination = usize::from(unit >> 8);
            let literal = i64::from(read_unit(&method.instructions, pc + 1, pc, opcode)? as i16);
            validate_register(pc, destination, register_count)?;
            fallthrough(format!("v[{destination}] = INT64_C({literal});"))
        }
        0x14 => {
            let destination = usize::from(unit >> 8);
            let literal = i64::from(read_i32(&method.instructions, pc + 1, pc, opcode)?);
            validate_register(pc, destination, register_count)?;
            fallthrough(format!("v[{destination}] = INT64_C({literal});"))
        }
        0x15 => {
            let destination = usize::from(unit >> 8);
            let high = i64::from(read_unit(&method.instructions, pc + 1, pc, opcode)? as i16);
            validate_register(pc, destination, register_count)?;
            fallthrough(format!("v[{destination}] = INT64_C({});", high << 16))
        }
        0x16 => {
            let destination = usize::from(unit >> 8);
            let literal = i64::from(read_unit(&method.instructions, pc + 1, pc, opcode)? as i16);
            validate_register(pc, destination, register_count)?;
            fallthrough(format!("v[{destination}] = INT64_C({literal});"))
        }
        0x17 => {
            let destination = usize::from(unit >> 8);
            let literal = i64::from(read_i32(&method.instructions, pc + 1, pc, opcode)?);
            validate_register(pc, destination, register_count)?;
            fallthrough(format!("v[{destination}] = INT64_C({literal});"))
        }
        0x18 => {
            let destination = usize::from(unit >> 8);
            let literal = read_i64(&method.instructions, pc + 1, pc, opcode)?;
            validate_register(pc, destination, register_count)?;
            fallthrough(format!("v[{destination}] = INT64_C({literal});"))
        }
        0x19 => {
            let destination = usize::from(unit >> 8);
            let high = i64::from(read_unit(&method.instructions, pc + 1, pc, opcode)? as i16);
            validate_register(pc, destination, register_count)?;
            fallthrough(format!("v[{destination}] = INT64_C({});", high << 48))
        }
        0x1a => {
            let destination = usize::from(unit >> 8);
            let index = usize::from(read_unit(&method.instructions, pc + 1, pc, opcode)?);
            validate_register(pc, destination, register_count)?;
            get_reference(&method.strings, pc, "string", index)?;
            fallthrough(format!(
                "v[{destination}] = wd_new_object(0); /* string@{index} */"
            ))
        }
        0x1b => {
            let destination = usize::from(unit >> 8);
            let index = usize::try_from(read_u32(&method.instructions, pc + 1, pc, opcode)?)
                .map_err(|_| BootstrapError::InvalidReference {
                    pc,
                    kind: "string",
                    index: usize::MAX,
                    count: method.strings.len(),
                })?;
            validate_register(pc, destination, register_count)?;
            get_reference(&method.strings, pc, "string", index)?;
            fallthrough(format!(
                "v[{destination}] = wd_new_object(0); /* string@{index} */"
            ))
        }
        0x1c => {
            let destination = usize::from(unit >> 8);
            let index = usize::from(read_unit(&method.instructions, pc + 1, pc, opcode)?);
            validate_register(pc, destination, register_count)?;
            get_reference(&method.types, pc, "type", index)?;
            fallthrough(format!("v[{destination}] = wd_new_object({index});"))
        }
        0x1d..=0x1f => fallthrough("/* monitor/check-cast compatibility stub */".to_owned()),
        0x20 => {
            let destination = usize::from((unit >> 8) & 0x0f);
            let source = usize::from((unit >> 12) & 0x0f);
            validate_register(pc, destination, register_count)?;
            validate_register(pc, source, register_count)?;
            fallthrough(format!("v[{destination}] = v[{source}] != 0;"))
        }
        0x21 => {
            let destination = usize::from((unit >> 8) & 0x0f);
            let source = usize::from((unit >> 12) & 0x0f);
            validate_register(pc, destination, register_count)?;
            validate_register(pc, source, register_count)?;
            fallthrough(format!(
                "v[{destination}] = 0; /* array-length handle v{source} */"
            ))
        }
        0x22 => {
            let destination = usize::from(unit >> 8);
            let type_index = usize::from(read_unit(&method.instructions, pc + 1, pc, opcode)?);
            validate_register(pc, destination, register_count)?;
            get_reference(&method.types, pc, "type", type_index)?;
            fallthrough(format!("v[{destination}] = wd_new_object({type_index});"))
        }
        0x23 => {
            let destination = usize::from((unit >> 8) & 0x0f);
            let size_register = usize::from((unit >> 12) & 0x0f);
            validate_register(pc, destination, register_count)?;
            validate_register(pc, size_register, register_count)?;
            fallthrough(format!(
                "v[{destination}] = wd_new_object(0); /* new-array */"
            ))
        }
        0x24 | 0x25 => {
            fallthrough("wd_last_result = wd_new_object(0); /* filled-new-array */".to_owned())
        }
        0x27 => {
            let source = usize::from(unit >> 8);
            validate_register(pc, source, register_count)?;
            Ok(format!("\twd_throw(v[{source}]);\n\treturn 0;\n"))
        }
        0x28 => {
            let offset = i64::from((unit >> 8) as u8 as i8);
            let target = branch_target(pc, offset, starts)?;
            Ok(format!("\tgoto L{target};\n"))
        }
        0x29 => {
            let offset = i64::from(read_unit(&method.instructions, pc + 1, pc, opcode)? as i16);
            let target = branch_target(pc, offset, starts)?;
            Ok(format!("\tgoto L{target};\n"))
        }
        0x2a => {
            let offset = i64::from(read_i32(&method.instructions, pc + 1, pc, opcode)?);
            let target = branch_target(pc, offset, starts)?;
            Ok(format!("\tgoto L{target};\n"))
        }
        0x2d..=0x31 => {
            let destination = usize::from(unit >> 8);
            let operands = read_unit(&method.instructions, pc + 1, pc, opcode)?;
            let left = usize::from(operands & 0xff);
            let right = usize::from(operands >> 8);
            validate_register(pc, destination, register_count)?;
            validate_register(pc, left, register_count)?;
            validate_register(pc, right, register_count)?;
            fallthrough(format!(
                "v[{destination}] = (v[{left}] > v[{right}]) - (v[{left}] < v[{right}]);"
            ))
        }
        0x32..=0x37 => {
            let left = usize::from((unit >> 8) & 0x0f);
            let right = usize::from((unit >> 12) & 0x0f);
            validate_register(pc, left, register_count)?;
            validate_register(pc, right, register_count)?;
            let offset = i64::from(read_unit(&method.instructions, pc + 1, pc, opcode)? as i16);
            let target = branch_target(pc, offset, starts)?;
            let next = next.ok_or(BootstrapError::Unsupported { pc, opcode })?;
            let operator = ["==", "!=", "<", ">=", ">", "<="][usize::from(opcode - 0x32)];
            Ok(format!(
                "\tif (v[{left}] {operator} v[{right}]) goto L{target};\n\tgoto L{next};\n"
            ))
        }
        0x38..=0x3d => {
            let source = usize::from(unit >> 8);
            validate_register(pc, source, register_count)?;
            let offset = i64::from(read_unit(&method.instructions, pc + 1, pc, opcode)? as i16);
            let target = branch_target(pc, offset, starts)?;
            let next = next.ok_or(BootstrapError::Unsupported { pc, opcode })?;
            let operator = ["==", "!=", "<", ">=", ">", "<="][usize::from(opcode - 0x38)];
            Ok(format!(
                "\tif (v[{source}] {operator} 0) goto L{target};\n\tgoto L{next};\n"
            ))
        }
        0x44..=0x51 => {
            let first = usize::from(unit >> 8);
            let operands = read_unit(&method.instructions, pc + 1, pc, opcode)?;
            let array = usize::from(operands & 0xff);
            let index = usize::from(operands >> 8);
            validate_register(pc, first, register_count)?;
            validate_register(pc, array, register_count)?;
            validate_register(pc, index, register_count)?;
            if opcode <= 0x4a {
                fallthrough(format!("v[{first}] = 0; /* aget */"))
            } else {
                fallthrough(format!("/* aput v{first} */"))
            }
        }
        0x52..=0x5f => {
            let value_register = usize::from((unit >> 8) & 0x0f);
            let object_register = usize::from((unit >> 12) & 0x0f);
            validate_register(pc, value_register, register_count)?;
            validate_register(pc, object_register, register_count)?;
            let field_index = usize::from(read_unit(&method.instructions, pc + 1, pc, opcode)?);
            get_reference(&method.fields, pc, "field", field_index)?;
            let compact =
                *compact_fields
                    .get(&field_index)
                    .ok_or(BootstrapError::InvalidReference {
                        pc,
                        kind: "compact field",
                        index: field_index,
                        count: compact_fields.len(),
                    })?;
            if opcode <= 0x58 {
                fallthrough(format!(
                    "v[{value_register}] = wd_iget((int32_t)v[{object_register}], {compact});"
                ))
            } else {
                fallthrough(format!(
                    "wd_iput((int32_t)v[{object_register}], {compact}, v[{value_register}]);"
                ))
            }
        }
        0x60..=0x6d => {
            let value_register = usize::from(unit >> 8);
            validate_register(pc, value_register, register_count)?;
            let field_index = usize::from(read_unit(&method.instructions, pc + 1, pc, opcode)?);
            get_reference(&method.fields, pc, "field", field_index)?;
            let compact =
                *compact_fields
                    .get(&field_index)
                    .ok_or(BootstrapError::InvalidReference {
                        pc,
                        kind: "compact field",
                        index: field_index,
                        count: compact_fields.len(),
                    })?;
            if opcode <= 0x66 {
                let special =
                    method.fields[field_index].contains("Landroid/os/Build$VERSION;->SDK_INT:I");
                if special {
                    fallthrough(format!("v[{value_register}] = 35;"))
                } else {
                    fallthrough(format!(
                        "v[{value_register}] = wd_static_fields[{compact}];"
                    ))
                }
            } else {
                fallthrough(format!(
                    "wd_static_fields[{compact}] = v[{value_register}];"
                ))
            }
        }
        0x6e..=0x72 => {
            let count = usize::from((unit >> 12) & 0x0f);
            let g = usize::from((unit >> 8) & 0x0f);
            let method_index = usize::from(read_unit(&method.instructions, pc + 1, pc, opcode)?);
            get_reference(&method.methods, pc, "method", method_index)?;
            let packed = read_unit(&method.instructions, pc + 2, pc, opcode)?;
            let mut registers = [
                usize::from(packed & 0x0f),
                usize::from((packed >> 4) & 0x0f),
                usize::from((packed >> 8) & 0x0f),
                usize::from((packed >> 12) & 0x0f),
                g,
            ];
            for register in registers.iter().take(count) {
                validate_register(pc, *register, register_count)?;
            }
            let args = registers
                .iter_mut()
                .take(count)
                .map(|register| format!("v[{register}]"))
                .collect::<Vec<_>>()
                .join(", ");
            let args_array = if count == 0 {
                "NULL".to_owned()
            } else {
                format!("(wd_value[]){{{args}}}")
            };
            fallthrough(format!(
                "wd_last_result = wd_invoke({method_index}, {count}, {args_array});"
            ))
        }
        0x74..=0x78 => {
            let count = usize::from(unit >> 8);
            let method_index = usize::from(read_unit(&method.instructions, pc + 1, pc, opcode)?);
            let first = usize::from(read_unit(&method.instructions, pc + 2, pc, opcode)?);
            get_reference(&method.methods, pc, "method", method_index)?;
            for register in first..first.saturating_add(count) {
                validate_register(pc, register, register_count)?;
            }
            let args = (first..first.saturating_add(count))
                .map(|register| format!("v[{register}]"))
                .collect::<Vec<_>>()
                .join(", ");
            let args_array = if count == 0 {
                "NULL".to_owned()
            } else {
                format!("(wd_value[]){{{args}}}")
            };
            fallthrough(format!(
                "wd_last_result = wd_invoke({method_index}, {count}, {args_array});"
            ))
        }
        0x7b..=0x8f => {
            let destination = usize::from((unit >> 8) & 0x0f);
            let source = usize::from((unit >> 12) & 0x0f);
            validate_register(pc, destination, register_count)?;
            validate_register(pc, source, register_count)?;
            fallthrough(format!(
                "v[{destination}] = v[{source}]; /* unary/conversion */"
            ))
        }
        0x90..=0xaf => {
            let destination = usize::from(unit >> 8);
            let operands = read_unit(&method.instructions, pc + 1, pc, opcode)?;
            let left = usize::from(operands & 0xff);
            let right = usize::from(operands >> 8);
            validate_register(pc, destination, register_count)?;
            validate_register(pc, left, register_count)?;
            validate_register(pc, right, register_count)?;
            let expression = binary_expression(opcode, left, right);
            fallthrough(format!("v[{destination}] = {expression};"))
        }
        0xb0..=0xcf => {
            let destination = usize::from((unit >> 8) & 0x0f);
            let right = usize::from((unit >> 12) & 0x0f);
            validate_register(pc, destination, register_count)?;
            validate_register(pc, right, register_count)?;
            let expression = binary_expression(opcode - 0x20, destination, right);
            fallthrough(format!("v[{destination}] = {expression};"))
        }
        0xd0..=0xd7 => {
            let destination = usize::from((unit >> 8) & 0x0f);
            let source = usize::from((unit >> 12) & 0x0f);
            let literal = i64::from(read_unit(&method.instructions, pc + 1, pc, opcode)? as i16);
            validate_register(pc, destination, register_count)?;
            validate_register(pc, source, register_count)?;
            fallthrough(format!(
                "v[{destination}] = v[{source}] + INT64_C({literal});"
            ))
        }
        0xd8..=0xe2 => {
            let destination = usize::from(unit >> 8);
            let operands = read_unit(&method.instructions, pc + 1, pc, opcode)?;
            let source = usize::from(operands & 0xff);
            let literal = i64::from((operands >> 8) as u8 as i8);
            validate_register(pc, destination, register_count)?;
            validate_register(pc, source, register_count)?;
            fallthrough(format!(
                "v[{destination}] = v[{source}] + INT64_C({literal});"
            ))
        }
        _ => Err(BootstrapError::Unsupported { pc, opcode }),
    }
}

fn binary_expression(opcode: u8, left: usize, right: usize) -> String {
    match opcode {
        0x90 | 0x9b | 0xa6 => format!("v[{left}] + v[{right}]"),
        0x91 | 0x9c | 0xa7 => format!("v[{left}] - v[{right}]"),
        0x92 | 0x9d | 0xa8 => format!("v[{left}] * v[{right}]"),
        0x93 | 0x9e | 0xa9 => format!("v[{right}] == 0 ? 0 : v[{left}] / v[{right}]"),
        0x94 | 0x9f | 0xaa => format!("v[{right}] == 0 ? 0 : v[{left}] % v[{right}]"),
        0x95 | 0xa0 | 0xab => format!("v[{left}] & v[{right}]"),
        0x96 | 0xa1 | 0xac => format!("v[{left}] | v[{right}]"),
        0x97 | 0xa2 | 0xad => format!("v[{left}] ^ v[{right}]"),
        0x98 | 0xa3 | 0xae => format!("v[{left}] << (v[{right}] & 63)"),
        0x99 | 0xa4 => format!("v[{left}] >> (v[{right}] & 63)"),
        0x9a | 0xa5 | 0xaf => format!("(uint64_t)v[{left}] >> (v[{right}] & 63)"),
        _ => format!("v[{left}]"),
    }
}

fn instruction_starts(instructions: &[u16]) -> Result<Vec<usize>, BootstrapError> {
    let mut starts = Vec::new();
    let mut pc = 0_usize;

    while pc < instructions.len() {
        starts.push(pc);
        let unit = instructions[pc];
        let opcode = (unit & 0xff) as u8;
        let width =
            instruction_width(opcode, unit).ok_or(BootstrapError::Unsupported { pc, opcode })?;
        let end = pc
            .checked_add(width)
            .ok_or(BootstrapError::Truncated { pc, opcode })?;
        if end > instructions.len() {
            return Err(BootstrapError::Truncated { pc, opcode });
        }
        pc = end;
    }

    Ok(starts)
}

fn instruction_width(opcode: u8, unit: u16) -> Option<usize> {
    if opcode == 0 {
        return (unit == 0).then_some(1);
    }

    match opcode {
        0x01
        | 0x04
        | 0x07
        | 0x0a..=0x12
        | 0x1d
        | 0x1e
        | 0x21
        | 0x27
        | 0x28
        | 0x7b..=0x8f
        | 0xb0..=0xcf => Some(1),
        0x02
        | 0x05
        | 0x08
        | 0x13
        | 0x15
        | 0x16
        | 0x19
        | 0x1a
        | 0x1c
        | 0x1f
        | 0x20
        | 0x22
        | 0x23
        | 0x29
        | 0x2d..=0x3d
        | 0x44..=0x6d
        | 0x90..=0xaf
        | 0xd0..=0xe2 => Some(2),
        0x03
        | 0x06
        | 0x09
        | 0x14
        | 0x17
        | 0x1b
        | 0x24..=0x26
        | 0x2a
        | 0x6e..=0x72
        | 0x74..=0x78 => Some(3),
        0x18 => Some(5),
        _ => None,
    }
}

fn is_supported_opcode(opcode: u8, unit: u16) -> bool {
    instruction_width(opcode, unit).is_some() && !matches!(opcode, 0x26 | 0x2b | 0x2c)
}

fn branch_target(
    pc: usize,
    offset: i64,
    starts: &BTreeSet<usize>,
) -> Result<usize, BootstrapError> {
    let target = i64::try_from(pc)
        .ok()
        .and_then(|value| value.checked_add(offset))
        .ok_or(BootstrapError::InvalidBranch { pc, target: offset })?;
    let target_usize =
        usize::try_from(target).map_err(|_| BootstrapError::InvalidBranch { pc, target })?;

    if !starts.contains(&target_usize) {
        return Err(BootstrapError::InvalidBranch { pc, target });
    }

    Ok(target_usize)
}

fn validate_register(pc: usize, register: usize, count: usize) -> Result<(), BootstrapError> {
    if register >= count {
        return Err(BootstrapError::InvalidRegister {
            pc,
            register,
            register_count: count,
        });
    }
    Ok(())
}

fn get_reference<'a>(
    values: &'a [String],
    pc: usize,
    kind: &'static str,
    index: usize,
) -> Result<&'a str, BootstrapError> {
    values
        .get(index)
        .map(String::as_str)
        .ok_or(BootstrapError::InvalidReference {
            pc,
            kind,
            index,
            count: values.len(),
        })
}

fn emit_string_table(source: &mut String, name: &str, values: &[String], minimum: usize) {
    source.push_str(&format!(
        "static const char *{name}[{minimum}] __attribute__((unused)) = {{\n"
    ));
    if values.is_empty() {
        source.push_str("\t\"<empty>\",\n");
    } else {
        for value in values {
            source.push_str("\t\"");
            source.push_str(&escape_c(value));
            source.push_str("\",\n");
        }
    }
    source.push_str("};\n\n");
}

fn escape_c(value: &str) -> String {
    let mut escaped = String::new();
    for character in value.chars() {
        match character {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            other if other.is_control() => escaped.push('?'),
            other => escaped.push(other),
        }
    }
    escaped
}

fn read_unit(
    instructions: &[u16],
    index: usize,
    pc: usize,
    opcode: u8,
) -> Result<u16, BootstrapError> {
    instructions
        .get(index)
        .copied()
        .ok_or(BootstrapError::Truncated { pc, opcode })
}

fn read_u32(
    instructions: &[u16],
    index: usize,
    pc: usize,
    opcode: u8,
) -> Result<u32, BootstrapError> {
    let low = u32::from(read_unit(instructions, index, pc, opcode)?);
    let high = u32::from(read_unit(instructions, index + 1, pc, opcode)?);
    Ok(low | (high << 16))
}

fn read_i32(
    instructions: &[u16],
    index: usize,
    pc: usize,
    opcode: u8,
) -> Result<i32, BootstrapError> {
    Ok(read_u32(instructions, index, pc, opcode)? as i32)
}

fn read_i64(
    instructions: &[u16],
    index: usize,
    pc: usize,
    opcode: u8,
) -> Result<i64, BootstrapError> {
    let mut value = 0_u64;
    for offset in 0..4 {
        value |= u64::from(read_unit(instructions, index + offset, pc, opcode)?) << (offset * 16);
    }
    Ok(value as i64)
}

fn read_zip_entry(archive: &mut ZipArchive<File>, name: &str) -> Result<Vec<u8>, BootstrapError> {
    let mut entry = archive
        .by_name(name)
        .map_err(|error| BootstrapError::Apk(format!("não foi possível abrir {name}: {error}")))?;

    if entry.size() > MAX_DEX_SIZE {
        return Err(BootstrapError::Apk(format!(
            "{name}: DEX excede {} MiB",
            MAX_DEX_SIZE / 1024 / 1024
        )));
    }

    let capacity = usize::try_from(entry.size())
        .map_err(|_| BootstrapError::Apk(format!("{name}: tamanho inválido")))?;
    let mut bytes = Vec::with_capacity(capacity);
    entry.read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn is_dex_name(name: &str) -> bool {
    name == "classes.dex"
        || name
            .strip_prefix("classes")
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
        .and_then(|number| number.parse().ok())
        .unwrap_or(u32::MAX)
}

fn read_uleb_cursor(bytes: &[u8], cursor: &mut usize) -> Result<u32, BootstrapError> {
    let mut result = 0_u32;

    for index in 0..5 {
        let position = cursor
            .checked_add(index)
            .ok_or_else(|| BootstrapError::Apk("ULEB128 overflow".to_owned()))?;
        let byte = *bytes
            .get(position)
            .ok_or_else(|| BootstrapError::Apk("ULEB128 truncado".to_owned()))?;
        result |= u32::from(byte & 0x7f) << (index * 7);

        if byte & 0x80 == 0 {
            *cursor = position + 1;
            return Ok(result);
        }
    }

    Err(BootstrapError::Apk("ULEB128 longo demais".to_owned()))
}

fn skip_encoded_fields(bytes: &[u8], cursor: &mut usize, count: u32) -> Result<(), BootstrapError> {
    for _ in 0..count {
        let _field_index = read_uleb_cursor(bytes, cursor)?;
        let _access_flags = read_uleb_cursor(bytes, cursor)?;
    }
    Ok(())
}

fn checked_slice(bytes: &[u8], offset: usize, size: usize) -> Result<&[u8], BootstrapError> {
    let end = offset
        .checked_add(size)
        .ok_or_else(|| BootstrapError::Apk("slice overflow".to_owned()))?;
    bytes
        .get(offset..end)
        .ok_or_else(|| BootstrapError::Apk(format!("slice fora do DEX: {offset:#x}..{end:#x}")))
}

fn temporary_c_path() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    std::env::temp_dir().join(format!(
        "winedroid-bootstrap-{}-{nanos}.c",
        std::process::id()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analyzes_object_demo() {
        let report = BootstrapCompiler::default()
            .analyze(&BootstrapMethod::demo())
            .unwrap();
        assert!(report.compilable());
        assert_eq!(report.referenced_fields.len(), 1);
    }

    #[test]
    fn emits_object_storage() {
        let source = BootstrapCompiler::default()
            .emit_c(&BootstrapMethod::demo())
            .unwrap();
        assert!(source.contains("wd_new_object"));
        assert!(source.contains("wd_iput"));
        assert!(source.contains("wd_iget"));
    }
}
