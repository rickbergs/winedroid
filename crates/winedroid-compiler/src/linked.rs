use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{BootstrapCompiler, BootstrapError, BootstrapMethod, find_bootstrap_method_in_apk};

const GENERATED_METHOD_MARKER: &str = "static wd_value winedroid_method(void) {\n";
const GENERATED_MAIN_MARKER: &str = "\nint main(void) {";
const STATIC_FIELDS_PREFIX: &str = "static __attribute__((unused)) wd_value wd_static_fields[";

pub const SUKISU_LIFECYCLE_TARGETS: [(&str, &str); 4] = [
    (
        "application-init",
        "Lcom/sukisu/ultra/KernelSUApplication;-><init>()V",
    ),
    (
        "application-oncreate",
        "Lcom/sukisu/ultra/KernelSUApplication;->onCreate()V",
    ),
    (
        "activity-init",
        "Lcom/sukisu/ultra/ui/MainActivity;-><init>()V",
    ),
    (
        "activity-oncreate",
        "Lcom/sukisu/ultra/ui/MainActivity;->onCreate(Landroid/os/Bundle;)V",
    ),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkedLifecycleArtifact {
    pub executable: PathBuf,
    pub method_count: usize,
    pub c_source: String,
}

#[derive(Debug, Clone)]
pub struct LinkedLifecycleCompiler {
    bootstrap: BootstrapCompiler,
    clang: PathBuf,
}

impl Default for LinkedLifecycleCompiler {
    fn default() -> Self {
        Self {
            bootstrap: BootstrapCompiler::default(),
            clang: PathBuf::from("clang"),
        }
    }
}

impl LinkedLifecycleCompiler {
    #[must_use]
    pub fn with_clang(clang: impl Into<PathBuf>) -> Self {
        Self {
            bootstrap: BootstrapCompiler::default(),
            clang: clang.into(),
        }
    }

    pub fn load_sukisu_methods(&self, apk: &Path) -> Result<Vec<BootstrapMethod>, BootstrapError> {
        let mut methods = Vec::with_capacity(SUKISU_LIFECYCLE_TARGETS.len());

        for (_, descriptor) in SUKISU_LIFECYCLE_TARGETS {
            let method = find_bootstrap_method_in_apk(apk, descriptor)?
                .ok_or_else(|| BootstrapError::MethodNotFound(descriptor.to_owned()))?;
            methods.push(method);
        }

        Ok(methods)
    }

    pub fn emit_sukisu_c(&self, apk: &Path) -> Result<String, BootstrapError> {
        let methods = self.load_sukisu_methods(apk)?;
        self.emit_linked_c(&methods)
    }

    pub fn emit_linked_c(&self, methods: &[BootstrapMethod]) -> Result<String, BootstrapError> {
        if methods.is_empty() {
            return Err(BootstrapError::Apk(
                "nenhum método foi fornecido ao linkador".to_owned(),
            ));
        }

        ensure_compatible_tables(methods)?;

        let generated_sources = methods
            .iter()
            .map(|method| self.bootstrap.emit_c(method))
            .collect::<Result<Vec<_>, _>>()?;

        let first_source = generated_sources
            .first()
            .ok_or_else(|| BootstrapError::Apk("fonte inicial ausente".to_owned()))?;
        let runtime_end = first_source.find(GENERATED_METHOD_MARKER).ok_or_else(|| {
            BootstrapError::Apk("marcador do método gerado não encontrado".to_owned())
        })?;

        let local_fields = methods
            .iter()
            .map(field_descriptors_in_compact_order)
            .collect::<Result<Vec<_>, _>>()?;

        let global_fields = build_global_field_map(&local_fields);
        let mut source =
            patch_static_field_capacity(&first_source[..runtime_end], global_fields.len().max(1))?;

        source
            .push_str("/* WineDroid linked lifecycle: one process, shared heap and fields. */\n\n");

        for (index, ((method, generated), fields)) in methods
            .iter()
            .zip(&generated_sources)
            .zip(&local_fields)
            .enumerate()
        {
            let local_to_global = fields
                .iter()
                .map(|descriptor| {
                    global_fields.get(descriptor).copied().ok_or_else(|| {
                        BootstrapError::Apk(format!("campo global não encontrado: {descriptor}"))
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;

            let function = transform_generated_method(generated, method, index, &local_to_global)?;
            source.push_str(&function);
            source.push('\n');
        }

        let types = &methods[0].types;
        let application_type = type_index(types, "Lcom/sukisu/ultra/KernelSUApplication;");
        let activity_type = type_index(types, "Lcom/sukisu/ultra/ui/MainActivity;");

        source.push_str("int main(void) {\n");
        source.push_str("\tfputs(\"[WineDroid] linked SukiSU lifecycle start\\n\", stderr);\n");
        source.push_str(&format!(
            "\twd_value application = wd_new_object({application_type});\n"
        ));
        source.push_str(
            "\twd_value application_args[] = { application };\n\
         \t(void)wd_linked_method_0(1, application_args);\n\
             \t(void)wd_linked_method_1(1, application_args);\n",
        );
        source.push_str(&format!(
            "\twd_value activity = wd_new_object({activity_type});\n"
        ));
        source.push_str(
            "\twd_value activity_init_args[] = { activity };\n\
         \twd_value activity_create_args[] = { activity, 0 };\n\
         \t(void)wd_linked_method_2(1, activity_init_args);\n\
             \t(void)wd_linked_method_3(2, activity_create_args);\n",
        );
        source.push_str(
            "\tfputs(\"[WineDroid] linked SukiSU lifecycle complete\\n\", stderr);\n\
             \tputs(\"WineDroid: SukiSU linked lifecycle completed\");\n\
             \treturn 0;\n\
             }\n",
        );

        Ok(source)
    }

    pub fn compile_sukisu(
        &self,
        apk: &Path,
        output: &Path,
        emit_c: Option<&Path>,
    ) -> Result<LinkedLifecycleArtifact, BootstrapError> {
        let methods = self.load_sukisu_methods(apk)?;
        self.compile_methods(&methods, output, emit_c)
    }

    pub fn compile_methods(
        &self,
        methods: &[BootstrapMethod],
        output: &Path,
        emit_c: Option<&Path>,
    ) -> Result<LinkedLifecycleArtifact, BootstrapError> {
        let c_source = self.emit_linked_c(methods)?;

        if let Some(parent) = output.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent)?;
        }

        if let Some(path) = emit_c {
            if let Some(parent) = path.parent()
                && !parent.as_os_str().is_empty()
            {
                fs::create_dir_all(parent)?;
            }
            fs::write(path, &c_source)?;
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
            return Err(BootstrapError::Clang {
                status: result.status.code(),
                stderr: String::from_utf8_lossy(&result.stderr).into_owned(),
            });
        }

        Ok(LinkedLifecycleArtifact {
            executable: output.to_owned(),
            method_count: methods.len(),
            c_source,
        })
    }
}

fn ensure_compatible_tables(methods: &[BootstrapMethod]) -> Result<(), BootstrapError> {
    let first = &methods[0];

    for method in &methods[1..] {
        if method.methods != first.methods
            || method.fields != first.fields
            || method.strings != first.strings
            || method.types != first.types
        {
            return Err(BootstrapError::Apk(format!(
                "método {} pertence a uma tabela DEX incompatível",
                method.descriptor
            )));
        }
    }

    Ok(())
}

fn build_global_field_map(local_fields: &[Vec<String>]) -> BTreeMap<String, usize> {
    let mut global = BTreeMap::new();

    for descriptor in local_fields.iter().flatten() {
        if !global.contains_key(descriptor) {
            let slot = global.len();
            global.insert(descriptor.clone(), slot);
        }
    }

    global
}

fn patch_static_field_capacity(runtime: &str, capacity: usize) -> Result<String, BootstrapError> {
    let start = runtime.find(STATIC_FIELDS_PREFIX).ok_or_else(|| {
        BootstrapError::Apk("declaração de campos estáticos não encontrada".to_owned())
    })?;
    let digits_start = start + STATIC_FIELDS_PREFIX.len();
    let digits_end = runtime[digits_start..]
        .find(']')
        .map(|offset| digits_start + offset)
        .ok_or_else(|| {
            BootstrapError::Apk("fim da capacidade de campos estáticos não encontrado".to_owned())
        })?;

    let mut patched = String::with_capacity(runtime.len() + 16);
    patched.push_str(&runtime[..digits_start]);
    patched.push_str(&capacity.to_string());
    patched.push_str(&runtime[digits_end..]);
    Ok(patched)
}

fn transform_generated_method(
    generated: &str,
    method: &BootstrapMethod,
    index: usize,
    local_to_global: &[usize],
) -> Result<String, BootstrapError> {
    let start = generated.find(GENERATED_METHOD_MARKER).ok_or_else(|| {
        BootstrapError::Apk(format!(
            "{}: início da função gerada não encontrado",
            method.descriptor
        ))
    })?;
    let relative_end = generated[start..]
        .find(GENERATED_MAIN_MARKER)
        .ok_or_else(|| {
            BootstrapError::Apk(format!(
                "{}: fim da função gerada não encontrado",
                method.descriptor
            ))
        })?;
    let end = start + relative_end;

    if method.ins_size > method.registers_size {
        return Err(BootstrapError::Apk(format!(
            "{}: ins_size={} excede registers_size={}",
            method.descriptor, method.ins_size, method.registers_size
        )));
    }

    let signature = format!(
        "static wd_value wd_linked_method_{index}(uint32_t argc, const wd_value *args) {{\n"
    );
    let mut function = generated[start..end].replacen(GENERATED_METHOD_MARKER, &signature, 1);

    let register_count = usize::from(method.registers_size).max(1);
    let frame_declaration = format!("\twd_value v[{register_count}] = {{0}};\n");
    let incoming_start = register_count.saturating_sub(usize::from(method.ins_size));
    let mut incoming = String::new();

    if method.ins_size == 0 {
        incoming.push_str("\t(void)argc;\n\t(void)args;\n");
    } else {
        for input in 0..usize::from(method.ins_size) {
            incoming.push_str(&format!(
                "\tv[{}] = (args != NULL && argc > {}) ? args[{}] : 0;\n",
                incoming_start + input,
                input,
                input
            ));
        }
    }

    if !function.contains(&frame_declaration) {
        return Err(BootstrapError::Apk(format!(
            "{}: declaração do frame não encontrada",
            method.descriptor
        )));
    }

    function = function.replacen(
        &frame_declaration,
        &(frame_declaration.clone() + &incoming),
        1,
    );

    if !method.is_static() && method.ins_size > 0 {
        let generated_this = format!("\tv[{incoming_start}] = wd_new_object(0); /* this */");
        if !function.contains(&generated_this) {
            return Err(BootstrapError::Apk(format!(
                "{}: inicialização de this não encontrada",
                method.descriptor
            )));
        }
        function = function.replacen(&generated_this, "", 1);
    }

    function = remap_bracket_indices(&function, "wd_static_fields[", local_to_global)?;
    function = remap_second_call_argument(&function, "wd_iget(", local_to_global)?;
    function = remap_second_call_argument(&function, "wd_iput(", local_to_global)?;

    Ok(function)
}

fn remap_bracket_indices(
    input: &str,
    prefix: &str,
    mapping: &[usize],
) -> Result<String, BootstrapError> {
    let mut output = String::with_capacity(input.len());
    let mut cursor = 0;

    while let Some(relative) = input[cursor..].find(prefix) {
        let occurrence = cursor + relative;
        let digits_start = occurrence + prefix.len();
        output.push_str(&input[cursor..digits_start]);

        let digits_end = input[digits_start..]
            .find(|character: char| !character.is_ascii_digit())
            .map(|offset| digits_start + offset)
            .unwrap_or(input.len());

        if digits_end == digits_start {
            cursor = digits_start;
            continue;
        }

        let local = input[digits_start..digits_end]
            .parse::<usize>()
            .map_err(|error| {
                BootstrapError::Apk(format!("índice local inválido em {prefix}: {error}"))
            })?;
        let global = mapping.get(local).copied().ok_or_else(|| {
            BootstrapError::Apk(format!("slot local de campo inexistente: {local}"))
        })?;

        output.push_str(&global.to_string());
        cursor = digits_end;
    }

    output.push_str(&input[cursor..]);
    Ok(output)
}

fn remap_second_call_argument(
    input: &str,
    call_prefix: &str,
    mapping: &[usize],
) -> Result<String, BootstrapError> {
    let mut output = String::with_capacity(input.len());
    let mut cursor = 0;

    while let Some(relative) = input[cursor..].find(call_prefix) {
        let call_start = cursor + relative;
        let search_start = call_start + call_prefix.len();
        let comma = input[search_start..]
            .find(',')
            .map(|offset| search_start + offset)
            .ok_or_else(|| {
                BootstrapError::Apk(format!(
                    "primeiro argumento de {call_prefix} não encontrado"
                ))
            })?;
        let mut digits_start = comma + 1;
        while input
            .as_bytes()
            .get(digits_start)
            .is_some_and(u8::is_ascii_whitespace)
        {
            digits_start += 1;
        }

        output.push_str(&input[cursor..digits_start]);

        let digits_end = input[digits_start..]
            .find(|character: char| !character.is_ascii_digit())
            .map(|offset| digits_start + offset)
            .unwrap_or(input.len());

        if digits_end == digits_start {
            cursor = digits_start;
            continue;
        }

        let local = input[digits_start..digits_end]
            .parse::<usize>()
            .map_err(|error| {
                BootstrapError::Apk(format!(
                    "segundo argumento inválido em {call_prefix}: {error}"
                ))
            })?;
        let global = mapping.get(local).copied().ok_or_else(|| {
            BootstrapError::Apk(format!(
                "slot local de campo inexistente em {call_prefix}: {local}"
            ))
        })?;

        output.push_str(&global.to_string());
        cursor = digits_end;
    }

    output.push_str(&input[cursor..]);
    Ok(output)
}

fn field_descriptors_in_compact_order(
    method: &BootstrapMethod,
) -> Result<Vec<String>, BootstrapError> {
    let mut descriptors = Vec::new();
    let mut seen = BTreeSet::new();
    let mut pc = 0;

    while pc < method.instructions.len() {
        let unit = method.instructions[pc];
        let opcode = (unit & 0xff) as u8;
        let width =
            instruction_width(opcode, unit).ok_or(BootstrapError::Unsupported { pc, opcode })?;

        if matches!(opcode, 0x52..=0x6d) {
            let field_index = usize::from(
                *method
                    .instructions
                    .get(pc + 1)
                    .ok_or(BootstrapError::Truncated { pc, opcode })?,
            );

            if seen.insert(field_index) {
                let descriptor = method
                    .fields
                    .get(field_index)
                    .ok_or(BootstrapError::InvalidReference {
                        pc,
                        kind: "field",
                        index: field_index,
                        count: method.fields.len(),
                    })?
                    .clone();
                descriptors.push(descriptor);
            }
        }

        pc = pc
            .checked_add(width)
            .ok_or(BootstrapError::Truncated { pc, opcode })?;
    }

    Ok(descriptors)
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

fn type_index(types: &[String], descriptor: &str) -> usize {
    types
        .iter()
        .position(|candidate| candidate == descriptor)
        .unwrap_or(0)
}

fn temporary_c_path() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    std::env::temp_dir().join(format!("winedroid-linked-{}-{nanos}.c", std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lifecycle_demo_methods() -> Vec<BootstrapMethod> {
        let descriptors = [
            "Ldemo/Application;-><init>()V",
            "Ldemo/Application;->onCreate()V",
            "Ldemo/Activity;-><init>()V",
            "Ldemo/Activity;->onCreate(Landroid/os/Bundle;)V",
        ];

        descriptors
            .iter()
            .enumerate()
            .map(|(index, descriptor)| {
                let mut method = BootstrapMethod::demo();
                method.descriptor = (*descriptor).to_owned();
                method.access_flags = 0;
                method.ins_size = if index == 3 { 2 } else { 1 };
                method
            })
            .collect()
    }

    #[test]
    fn emits_one_main_for_four_methods() {
        let source = LinkedLifecycleCompiler::default()
            .emit_linked_c(&lifecycle_demo_methods())
            .unwrap();

        assert_eq!(source.matches("int main(void)").count(), 1);
        assert_eq!(
            source.matches("static wd_value wd_linked_method_").count(),
            4
        );
        assert!(source.contains("activity_create_args"));
        assert!(source.contains("linked SukiSU lifecycle complete"));
    }

    #[test]
    fn shares_field_slot_between_methods() {
        let source = LinkedLifecycleCompiler::default()
            .emit_linked_c(&lifecycle_demo_methods())
            .unwrap();

        assert!(source.contains("wd_instance_fields[4096]"));
        assert!(source.contains("wd_iput((int32_t)v[0], 0"));
        assert!(source.contains("wd_iget((int32_t)v[0], 0"));
    }
}
