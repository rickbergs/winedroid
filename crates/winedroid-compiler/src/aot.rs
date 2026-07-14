use std::{
    collections::BTreeSet,
    error::Error,
    fmt, fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::dex_method::{DexFieldReference, DexMethodBody};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DalvikStaticField {
    pub index: u16,
    pub descriptor: String,
    pub field_type: String,
    pub initial_i32: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DalvikProgram {
    pub descriptor: String,
    pub register_count: u16,
    pub ins_size: u16,
    pub instructions: Vec<u16>,
    pub static_fields: Vec<DalvikStaticField>,
}

impl DalvikProgram {
    #[must_use]
    pub fn demo() -> Self {
        Self {
            descriptor: "Ldev/winedroid/Demo;->answer()I".to_owned(),
            register_count: 3,
            ins_size: 0,
            instructions: vec![
                0x0013, 20, // const/16 v0, 20
                0x0113, 22, // const/16 v1, 22
                0x0290, 0x0100, // add-int v2, v0, v1
                0x020f, // return v2
            ],
            static_fields: Vec::new(),
        }
    }

    #[must_use]
    pub fn static_field_demo() -> Self {
        Self {
            descriptor: "Ldev/winedroid/StaticDemo;->roundTrip()I".to_owned(),
            register_count: 2,
            ins_size: 0,
            instructions: vec![
                0x0013, 42, // const/16 v0, 42
                0x0067, 0, // sput v0, field@0
                0x0160, 0,      // sget v1, field@0
                0x010f, // return v1
            ],
            static_fields: vec![DalvikStaticField {
                index: 0,
                descriptor: "Ldev/winedroid/StaticDemo;->counter:I".to_owned(),
                field_type: "I".to_owned(),
                initial_i32: 0,
            }],
        }
    }

    pub fn set_all_static_i32(&mut self, value: i32) {
        for field in &mut self.static_fields {
            if is_int_like_type(&field.field_type) {
                field.initial_i32 = value;
            }
        }
    }

    pub fn set_static_i32(&mut self, descriptor: &str, value: i32) -> bool {
        let Some(field) = self
            .static_fields
            .iter_mut()
            .find(|field| field.descriptor == descriptor)
        else {
            return false;
        };

        field.initial_i32 = value;
        true
    }
}

impl From<DexMethodBody> for DalvikProgram {
    fn from(body: DexMethodBody) -> Self {
        Self {
            descriptor: body.descriptor,
            register_count: body.registers_size,
            ins_size: body.ins_size,
            instructions: body.instructions,
            static_fields: body
                .field_table
                .iter()
                .map(DalvikStaticField::from)
                .collect(),
        }
    }
}

impl From<&DexFieldReference> for DalvikStaticField {
    fn from(field: &DexFieldReference) -> Self {
        Self {
            index: field.index,
            descriptor: field.descriptor.clone(),
            field_type: field.field_type.clone(),
            initial_i32: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompileArtifact {
    pub executable: PathBuf,
    pub c_source: String,
    pub referenced_static_fields: Vec<DalvikStaticField>,
}

#[derive(Debug)]
pub enum AotError {
    EmptyProgram,
    MethodHasInputs {
        ins_size: u16,
    },
    TruncatedInstruction {
        pc: usize,
        opcode: u8,
    },
    UnsupportedOpcode {
        pc: usize,
        opcode: u8,
    },
    InvalidRegister {
        pc: usize,
        register: usize,
        register_count: usize,
    },
    InvalidBranchTarget {
        pc: usize,
        target: i64,
    },
    InvalidFieldIndex {
        pc: usize,
        index: u16,
        field_count: usize,
    },
    UnsupportedFieldType {
        pc: usize,
        descriptor: String,
        field_type: String,
    },
    MissingReturn,
    Io(std::io::Error),
    ClangFailed {
        status: Option<i32>,
        stderr: String,
    },
}

impl fmt::Display for AotError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyProgram => write!(formatter, "o método não possui bytecode"),
            Self::MethodHasInputs { ins_size } => write!(
                formatter,
                "o backend inicial só compila métodos sem argumentos; ins_size={ins_size}"
            ),
            Self::TruncatedInstruction { pc, opcode } => write!(
                formatter,
                "instrução truncada em pc={pc}, opcode={opcode:#04x}"
            ),
            Self::UnsupportedOpcode { pc, opcode } => write!(
                formatter,
                "opcode ainda não suportado em pc={pc}: {opcode:#04x}"
            ),
            Self::InvalidRegister {
                pc,
                register,
                register_count,
            } => write!(
                formatter,
                "pc={pc}: v{register} não existe; método possui {register_count} registradores"
            ),
            Self::InvalidBranchTarget { pc, target } => {
                write!(formatter, "pc={pc}: destino de salto inválido: {target}")
            }
            Self::InvalidFieldIndex {
                pc,
                index,
                field_count,
            } => write!(
                formatter,
                "pc={pc}: field_id #{index} não existe; tabela possui {field_count} campos"
            ),
            Self::UnsupportedFieldType {
                pc,
                descriptor,
                field_type,
            } => write!(
                formatter,
                "pc={pc}: campo {descriptor} usa tipo ainda não suportado: {field_type}"
            ),
            Self::MissingReturn => write!(formatter, "o método pode terminar sem return"),
            Self::Io(error) => write!(formatter, "{error}"),
            Self::ClangFailed { status, stderr } => {
                write!(formatter, "Clang falhou com status {status:?}:\n{stderr}")
            }
        }
    }
}

impl Error for AotError {}

impl From<std::io::Error> for AotError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

#[derive(Debug, Clone)]
pub struct AotCompiler {
    clang: PathBuf,
}

impl Default for AotCompiler {
    fn default() -> Self {
        Self {
            clang: PathBuf::from("clang"),
        }
    }
}

impl AotCompiler {
    #[must_use]
    pub fn with_clang(clang: impl Into<PathBuf>) -> Self {
        Self {
            clang: clang.into(),
        }
    }

    pub fn emit_c(&self, program: &DalvikProgram) -> Result<String, AotError> {
        if program.instructions.is_empty() {
            return Err(AotError::EmptyProgram);
        }
        if program.ins_size != 0 {
            return Err(AotError::MethodHasInputs {
                ins_size: program.ins_size,
            });
        }

        let decoded = decode_program(program)?;
        let referenced_static_fields = referenced_static_fields(program)?;
        let register_count = usize::from(program.register_count).max(1);
        let mut source = String::new();

        source.push_str(
            "#include <stdint.h>\n#include <stdio.h>\n#include <stdlib.h>\n#include <limits.h>\n\n",
        );
        source.push_str(
            "static __attribute__((unused)) int32_t wd_div(int32_t a, int32_t b) {\n\
             \tif (b == 0) { fputs(\"WineDroid: division by zero\\n\", stderr); exit(101); }\n\
             \tif (a == INT32_MIN && b == -1) { return INT32_MIN; }\n\
             \treturn a / b;\n\
             }\n\n",
        );
        source.push_str(
            "static __attribute__((unused)) int32_t wd_rem(int32_t a, int32_t b) {\n\
             \tif (b == 0) { fputs(\"WineDroid: division by zero\\n\", stderr); exit(101); }\n\
             \tif (a == INT32_MIN && b == -1) { return 0; }\n\
             \treturn a % b;\n\
             }\n\n",
        );

        for field in &referenced_static_fields {
            source.push_str(&format!(
                "static int32_t wd_sfield_{} = {};\n",
                field.index,
                c_i32(field.initial_i32)
            ));
        }
        if !referenced_static_fields.is_empty() {
            source.push('\n');
        }

        source.push_str("static int32_t winedroid_method(void) {\n");
        source.push_str(&format!("\tint32_t v[{register_count}] = {{0}};\n"));
        source.push_str("\tgoto L0;\n");

        for instruction in &decoded {
            source.push_str(&format!("L{}:\n", instruction.pc));
            source.push_str(&instruction.c);
        }

        source.push_str("}\n\n");
        source.push_str(
            "int main(void) {\n\
             \tint32_t result = winedroid_method();\n\
             \tprintf(\"%d\\n\", result);\n\
             \treturn 0;\n\
             }\n",
        );

        Ok(source)
    }

    pub fn compile(
        &self,
        program: &DalvikProgram,
        output: &Path,
        emit_c: Option<&Path>,
    ) -> Result<CompileArtifact, AotError> {
        let c_source = self.emit_c(program)?;

        if let Some(parent) = output.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent)?;
        }

        if let Some(c_path) = emit_c {
            if let Some(parent) = c_path.parent()
                && !parent.as_os_str().is_empty()
            {
                fs::create_dir_all(parent)?;
            }
            fs::write(c_path, &c_source)?;
        }

        let temporary = temporary_c_path();
        fs::write(&temporary, &c_source)?;

        let result = Command::new(&self.clang)
            .args([
                "-std=c11",
                "-O2",
                "-fPIE",
                "-pie",
                "-Wall",
                "-Wextra",
                "-Werror",
                "-Wno-unused-label",
                "-o",
            ])
            .arg(output)
            .arg(&temporary)
            .output();

        let _ = fs::remove_file(&temporary);

        let result = result?;
        if !result.status.success() {
            return Err(AotError::ClangFailed {
                status: result.status.code(),
                stderr: String::from_utf8_lossy(&result.stderr).into_owned(),
            });
        }

        let referenced_static_fields = referenced_static_fields(program)?
            .into_iter()
            .cloned()
            .collect();

        Ok(CompileArtifact {
            executable: output.to_owned(),
            c_source,
            referenced_static_fields,
        })
    }
}

#[derive(Debug)]
struct DecodedInstruction {
    pc: usize,
    c: String,
}

fn decode_program(program: &DalvikProgram) -> Result<Vec<DecodedInstruction>, AotError> {
    let starts = instruction_starts(&program.instructions)?;
    let start_set: BTreeSet<usize> = starts.iter().copied().collect();
    let register_count = usize::from(program.register_count);
    let mut decoded = Vec::with_capacity(starts.len());
    let mut has_return = false;

    for (position, pc) in starts.iter().copied().enumerate() {
        let instruction = program.instructions[pc];
        let opcode = (instruction & 0xff) as u8;
        let next = starts.get(position + 1).copied();
        let fallthrough = |statement: String| -> Result<String, AotError> {
            let next = next.ok_or(AotError::MissingReturn)?;
            Ok(format!("\t{statement}\n\tgoto L{next};\n"))
        };

        let c = match opcode {
            0x00 => fallthrough("/* nop */".to_owned())?,
            0x01 => {
                let destination = usize::from((instruction >> 8) & 0x0f);
                let source = usize::from((instruction >> 12) & 0x0f);
                validate_register(pc, destination, register_count)?;
                validate_register(pc, source, register_count)?;
                fallthrough(format!("v[{destination}] = v[{source}];"))?
            }
            0x02 => {
                let destination = usize::from(instruction >> 8);
                let source = usize::from(read_unit(&program.instructions, pc + 1, pc, opcode)?);
                validate_register(pc, destination, register_count)?;
                validate_register(pc, source, register_count)?;
                fallthrough(format!("v[{destination}] = v[{source}];"))?
            }
            0x03 => {
                let destination =
                    usize::from(read_unit(&program.instructions, pc + 1, pc, opcode)?);
                let source = usize::from(read_unit(&program.instructions, pc + 2, pc, opcode)?);
                validate_register(pc, destination, register_count)?;
                validate_register(pc, source, register_count)?;
                fallthrough(format!("v[{destination}] = v[{source}];"))?
            }
            0x0e => {
                has_return = true;
                "\treturn 0;\n".to_owned()
            }
            0x0f => {
                let source = usize::from(instruction >> 8);
                validate_register(pc, source, register_count)?;
                has_return = true;
                format!("\treturn v[{source}];\n")
            }
            0x12 => {
                let destination = usize::from((instruction >> 8) & 0x0f);
                validate_register(pc, destination, register_count)?;
                let nibble = ((instruction >> 12) & 0x0f) as u8;
                let literal = i32::from(((nibble << 4) as i8) >> 4);
                fallthrough(format!("v[{destination}] = INT32_C({literal});"))?
            }
            0x13 => {
                let destination = usize::from(instruction >> 8);
                validate_register(pc, destination, register_count)?;
                let literal =
                    i32::from(read_unit(&program.instructions, pc + 1, pc, opcode)? as i16);
                fallthrough(format!("v[{destination}] = INT32_C({literal});"))?
            }
            0x14 => {
                let destination = usize::from(instruction >> 8);
                validate_register(pc, destination, register_count)?;
                let literal = read_i32(&program.instructions, pc + 1, pc, opcode)?;
                fallthrough(format!("v[{destination}] = INT32_C({literal});"))?
            }
            0x15 => {
                let destination = usize::from(instruction >> 8);
                validate_register(pc, destination, register_count)?;
                let high = i32::from(read_unit(&program.instructions, pc + 1, pc, opcode)? as i16);
                let literal = high.wrapping_shl(16);
                fallthrough(format!("v[{destination}] = INT32_C({literal});"))?
            }
            0x28 => {
                let offset = i64::from((instruction >> 8) as u8 as i8);
                let target = branch_target(pc, offset, &start_set)?;
                format!("\tgoto L{target};\n")
            }
            0x29 => {
                let offset =
                    i64::from(read_unit(&program.instructions, pc + 1, pc, opcode)? as i16);
                let target = branch_target(pc, offset, &start_set)?;
                format!("\tgoto L{target};\n")
            }
            0x2a => {
                let offset = i64::from(read_i32(&program.instructions, pc + 1, pc, opcode)?);
                let target = branch_target(pc, offset, &start_set)?;
                format!("\tgoto L{target};\n")
            }
            0x32..=0x37 => {
                let left = usize::from((instruction >> 8) & 0x0f);
                let right = usize::from((instruction >> 12) & 0x0f);
                validate_register(pc, left, register_count)?;
                validate_register(pc, right, register_count)?;
                let offset =
                    i64::from(read_unit(&program.instructions, pc + 1, pc, opcode)? as i16);
                let target = branch_target(pc, offset, &start_set)?;
                let next = next.ok_or(AotError::MissingReturn)?;
                let operator = match opcode {
                    0x32 => "==",
                    0x33 => "!=",
                    0x34 => "<",
                    0x35 => ">=",
                    0x36 => ">",
                    0x37 => "<=",
                    _ => unreachable!(),
                };
                format!(
                    "\tif (v[{left}] {operator} v[{right}]) {{ goto L{target}; }}\n\tgoto L{next};\n"
                )
            }
            0x38..=0x3d => {
                let source = usize::from(instruction >> 8);
                validate_register(pc, source, register_count)?;
                let offset =
                    i64::from(read_unit(&program.instructions, pc + 1, pc, opcode)? as i16);
                let target = branch_target(pc, offset, &start_set)?;
                let next = next.ok_or(AotError::MissingReturn)?;
                let operator = match opcode {
                    0x38 => "==",
                    0x39 => "!=",
                    0x3a => "<",
                    0x3b => ">=",
                    0x3c => ">",
                    0x3d => "<=",
                    _ => unreachable!(),
                };
                format!("\tif (v[{source}] {operator} 0) {{ goto L{target}; }}\n\tgoto L{next};\n")
            }
            0x60 | 0x63..=0x66 => {
                let destination = usize::from(instruction >> 8);
                validate_register(pc, destination, register_count)?;
                let field_index = read_unit(&program.instructions, pc + 1, pc, opcode)?;
                let field = static_field(program, pc, field_index)?;
                validate_static_opcode_type(pc, opcode, field)?;
                let expression = static_read_expression(opcode, field.index);
                fallthrough(format!("v[{destination}] = {expression};"))?
            }
            0x67 | 0x6a..=0x6d => {
                let source_register = usize::from(instruction >> 8);
                validate_register(pc, source_register, register_count)?;
                let field_index = read_unit(&program.instructions, pc + 1, pc, opcode)?;
                let field = static_field(program, pc, field_index)?;
                validate_static_opcode_type(pc, opcode, field)?;
                let statement = static_write_statement(opcode, field.index, source_register);
                fallthrough(statement)?
            }
            0x90..=0x9a => {
                let destination = usize::from(instruction >> 8);
                let operands = read_unit(&program.instructions, pc + 1, pc, opcode)?;
                let left = usize::from(operands & 0xff);
                let right = usize::from(operands >> 8);
                validate_register(pc, destination, register_count)?;
                validate_register(pc, left, register_count)?;
                validate_register(pc, right, register_count)?;
                let expression = binary_expression(opcode, left, right);
                fallthrough(format!("v[{destination}] = {expression};"))?
            }
            0xb0..=0xba => {
                let destination = usize::from((instruction >> 8) & 0x0f);
                let right = usize::from((instruction >> 12) & 0x0f);
                validate_register(pc, destination, register_count)?;
                validate_register(pc, right, register_count)?;
                let expression = binary_expression(opcode - 0x20, destination, right);
                fallthrough(format!("v[{destination}] = {expression};"))?
            }
            0xd0..=0xd7 => {
                let destination = usize::from((instruction >> 8) & 0x0f);
                let source = usize::from((instruction >> 12) & 0x0f);
                validate_register(pc, destination, register_count)?;
                validate_register(pc, source, register_count)?;
                let literal =
                    i32::from(read_unit(&program.instructions, pc + 1, pc, opcode)? as i16);
                let expression = literal_expression(opcode, source, literal);
                fallthrough(format!("v[{destination}] = {expression};"))?
            }
            0xd8..=0xe2 => {
                let destination = usize::from(instruction >> 8);
                let operands = read_unit(&program.instructions, pc + 1, pc, opcode)?;
                let source = usize::from(operands & 0xff);
                let literal = i32::from((operands >> 8) as u8 as i8);
                validate_register(pc, destination, register_count)?;
                validate_register(pc, source, register_count)?;
                let expression = literal_expression(opcode, source, literal);
                fallthrough(format!("v[{destination}] = {expression};"))?
            }
            _ => return Err(AotError::UnsupportedOpcode { pc, opcode }),
        };

        decoded.push(DecodedInstruction { pc, c });
    }

    if !has_return {
        return Err(AotError::MissingReturn);
    }

    Ok(decoded)
}

fn instruction_starts(instructions: &[u16]) -> Result<Vec<usize>, AotError> {
    let mut starts = Vec::new();
    let mut pc = 0_usize;

    while pc < instructions.len() {
        starts.push(pc);
        let opcode = (instructions[pc] & 0xff) as u8;
        let width = instruction_width(opcode).ok_or(AotError::UnsupportedOpcode { pc, opcode })?;
        let end = pc
            .checked_add(width)
            .ok_or(AotError::TruncatedInstruction { pc, opcode })?;
        if end > instructions.len() {
            return Err(AotError::TruncatedInstruction { pc, opcode });
        }
        pc = end;
    }

    Ok(starts)
}

fn instruction_width(opcode: u8) -> Option<usize> {
    match opcode {
        0x00 | 0x01 | 0x0e | 0x0f | 0x12 | 0x28 | 0xb0..=0xba => Some(1),
        0x02 | 0x13 | 0x15 | 0x29 | 0x32..=0x3d | 0x60..=0x6d | 0x90..=0x9a | 0xd0..=0xe2 => {
            Some(2)
        }
        0x03 | 0x14 | 0x2a => Some(3),
        _ => None,
    }
}

fn referenced_static_fields(program: &DalvikProgram) -> Result<Vec<&DalvikStaticField>, AotError> {
    let mut indices = BTreeSet::new();
    let starts = instruction_starts(&program.instructions)?;

    for pc in starts {
        let opcode = (program.instructions[pc] & 0xff) as u8;
        if matches!(opcode, 0x60..=0x6d) {
            let index = read_unit(&program.instructions, pc + 1, pc, opcode)?;
            let field = static_field(program, pc, index)?;
            validate_static_opcode_type(pc, opcode, field)?;
            indices.insert(index);
        }
    }

    indices
        .into_iter()
        .map(|index| static_field(program, 0, index))
        .collect()
}

fn static_field(
    program: &DalvikProgram,
    pc: usize,
    index: u16,
) -> Result<&DalvikStaticField, AotError> {
    program
        .static_fields
        .get(usize::from(index))
        .filter(|field| field.index == index)
        .ok_or(AotError::InvalidFieldIndex {
            pc,
            index,
            field_count: program.static_fields.len(),
        })
}

fn validate_static_opcode_type(
    pc: usize,
    opcode: u8,
    field: &DalvikStaticField,
) -> Result<(), AotError> {
    let expected = match opcode {
        0x60 | 0x67 => "I",
        0x63 | 0x6a => "Z",
        0x64 | 0x6b => "B",
        0x65 | 0x6c => "C",
        0x66 | 0x6d => "S",
        _ => {
            return Err(AotError::UnsupportedOpcode { pc, opcode });
        }
    };

    if field.field_type != expected {
        return Err(AotError::UnsupportedFieldType {
            pc,
            descriptor: field.descriptor.clone(),
            field_type: field.field_type.clone(),
        });
    }

    Ok(())
}

fn static_read_expression(opcode: u8, index: u16) -> String {
    match opcode {
        0x60 => format!("wd_sfield_{index}"),
        0x63 => format!("wd_sfield_{index} != 0"),
        0x64 => format!("(int32_t)(int8_t)wd_sfield_{index}"),
        0x65 => format!("(int32_t)(uint16_t)wd_sfield_{index}"),
        0x66 => format!("(int32_t)(int16_t)wd_sfield_{index}"),
        _ => unreachable!(),
    }
}

fn static_write_statement(opcode: u8, index: u16, source: usize) -> String {
    match opcode {
        0x67 => format!("wd_sfield_{index} = v[{source}];"),
        0x6a => format!("wd_sfield_{index} = v[{source}] != 0;"),
        0x6b => format!("wd_sfield_{index} = (int32_t)(int8_t)v[{source}];"),
        0x6c => format!("wd_sfield_{index} = (int32_t)(uint16_t)v[{source}];"),
        0x6d => format!("wd_sfield_{index} = (int32_t)(int16_t)v[{source}];"),
        _ => unreachable!(),
    }
}

fn is_int_like_type(field_type: &str) -> bool {
    matches!(field_type, "I" | "Z" | "B" | "C" | "S")
}

fn c_i32(value: i32) -> String {
    format!("(int32_t)UINT32_C(0x{:08x})", value as u32)
}

fn binary_expression(opcode: u8, left: usize, right: usize) -> String {
    match opcode {
        0x90 => format!("(int32_t)((uint32_t)v[{left}] + (uint32_t)v[{right}])"),
        0x91 => format!("(int32_t)((uint32_t)v[{left}] - (uint32_t)v[{right}])"),
        0x92 => format!("(int32_t)((uint32_t)v[{left}] * (uint32_t)v[{right}])"),
        0x93 => format!("wd_div(v[{left}], v[{right}])"),
        0x94 => format!("wd_rem(v[{left}], v[{right}])"),
        0x95 => format!("v[{left}] & v[{right}]"),
        0x96 => format!("v[{left}] | v[{right}]"),
        0x97 => format!("v[{left}] ^ v[{right}]"),
        0x98 => format!("(int32_t)((uint32_t)v[{left}] << ((uint32_t)v[{right}] & UINT32_C(31)))"),
        0x99 => format!("v[{left}] >> ((uint32_t)v[{right}] & UINT32_C(31))"),
        0x9a => format!("(int32_t)((uint32_t)v[{left}] >> ((uint32_t)v[{right}] & UINT32_C(31)))"),
        _ => unreachable!(),
    }
}

fn literal_expression(opcode: u8, source: usize, literal: i32) -> String {
    match opcode {
        0xd0 | 0xd8 => {
            format!("(int32_t)((uint32_t)v[{source}] + (uint32_t)INT32_C({literal}))")
        }
        0xd1 | 0xd9 => {
            format!("(int32_t)((uint32_t)INT32_C({literal}) - (uint32_t)v[{source}])")
        }
        0xd2 | 0xda => {
            format!("(int32_t)((uint32_t)v[{source}] * (uint32_t)INT32_C({literal}))")
        }
        0xd3 | 0xdb => format!("wd_div(v[{source}], INT32_C({literal}))"),
        0xd4 | 0xdc => format!("wd_rem(v[{source}], INT32_C({literal}))"),
        0xd5 | 0xdd => format!("v[{source}] & INT32_C({literal})"),
        0xd6 | 0xde => format!("v[{source}] | INT32_C({literal})"),
        0xd7 | 0xdf => format!("v[{source}] ^ INT32_C({literal})"),
        0xe0 => format!(
            "(int32_t)((uint32_t)v[{source}] << (UINT32_C({}) & UINT32_C(31)))",
            literal as u32
        ),
        0xe1 => format!(
            "v[{source}] >> (UINT32_C({}) & UINT32_C(31))",
            literal as u32
        ),
        0xe2 => format!(
            "(int32_t)((uint32_t)v[{source}] >> (UINT32_C({}) & UINT32_C(31)))",
            literal as u32
        ),
        _ => unreachable!(),
    }
}

fn validate_register(pc: usize, register: usize, register_count: usize) -> Result<(), AotError> {
    if register >= register_count {
        return Err(AotError::InvalidRegister {
            pc,
            register,
            register_count,
        });
    }
    Ok(())
}

fn branch_target(pc: usize, offset: i64, starts: &BTreeSet<usize>) -> Result<usize, AotError> {
    let target = i64::try_from(pc)
        .ok()
        .and_then(|pc| pc.checked_add(offset))
        .ok_or(AotError::InvalidBranchTarget { pc, target: offset })?;
    let target_usize =
        usize::try_from(target).map_err(|_| AotError::InvalidBranchTarget { pc, target })?;

    if !starts.contains(&target_usize) {
        return Err(AotError::InvalidBranchTarget { pc, target });
    }

    Ok(target_usize)
}

fn read_unit(instructions: &[u16], index: usize, pc: usize, opcode: u8) -> Result<u16, AotError> {
    instructions
        .get(index)
        .copied()
        .ok_or(AotError::TruncatedInstruction { pc, opcode })
}

fn read_i32(instructions: &[u16], index: usize, pc: usize, opcode: u8) -> Result<i32, AotError> {
    let low = u32::from(read_unit(instructions, index, pc, opcode)?);
    let high = u32::from(read_unit(instructions, index + 1, pc, opcode)?);
    Ok((low | (high << 16)) as i32)
}

fn temporary_c_path() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    std::env::temp_dir().join(format!("winedroid-aot-{}-{nanos}.c", std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emits_native_source_for_demo() {
        let source = AotCompiler::default()
            .emit_c(&DalvikProgram::demo())
            .expect("demo deveria compilar para C");
        assert!(source.contains("INT32_C(20)"));
        assert!(source.contains("INT32_C(22)"));
        assert!(source.contains("uint32_t)v[0] + (uint32_t)v[1]"));
        assert!(source.contains("return v[2]"));
    }

    #[test]
    fn emits_static_field_storage_and_accesses() {
        let source = AotCompiler::default()
            .emit_c(&DalvikProgram::static_field_demo())
            .expect("static field demo deveria compilar para C");
        assert!(source.contains("static int32_t wd_sfield_0"));
        assert!(source.contains("wd_sfield_0 = v[0]"));
        assert!(source.contains("v[1] = wd_sfield_0"));
    }

    #[test]
    fn applies_static_field_overrides() {
        let mut program = DalvikProgram::static_field_demo();
        assert!(program.set_static_i32("Ldev/winedroid/StaticDemo;->counter:I", 99));
        let source = AotCompiler::default().emit_c(&program).unwrap();
        assert!(source.contains("UINT32_C(0x00000063)"));
    }

    #[test]
    fn rejects_input_methods_for_now() {
        let mut program = DalvikProgram::demo();
        program.ins_size = 1;
        assert!(matches!(
            AotCompiler::default().emit_c(&program),
            Err(AotError::MethodHasInputs { ins_size: 1 })
        ));
    }

    #[test]
    fn rejects_unsupported_opcode() {
        let program = DalvikProgram {
            descriptor: "LTest;->unsupported()I".to_owned(),
            register_count: 1,
            ins_size: 0,
            instructions: vec![0x0022, 0x000f],
            static_fields: Vec::new(),
        };
        assert!(matches!(
            AotCompiler::default().emit_c(&program),
            Err(AotError::UnsupportedOpcode {
                pc: 0,
                opcode: 0x22
            })
        ));
    }
}
