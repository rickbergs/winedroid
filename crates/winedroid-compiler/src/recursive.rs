use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{
    BootstrapCompiler, BootstrapError, BootstrapMethod, LinkedLifecycleCompiler,
    SUKISU_LIFECYCLE_TARGETS, find_bootstrap_method_in_apk,
};

const INVOKE_SIGNATURE: &str = concat!(
    "static __attribute__((unused)) wd_value wd_invoke(",
    "uint32_t method_index, uint32_t argc, const wd_value *args) {"
);
const EXTERNAL_INVOKE_SIGNATURE: &str = concat!(
    "static __attribute__((unused)) wd_value wd_invoke_external(",
    "uint32_t method_index, uint32_t argc, const wd_value *args) {"
);
const FIRST_LINKED_METHOD: &str = "static wd_value wd_linked_method_0(";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecursiveRejectedMethod {
    pub descriptor: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecursiveLifecycleReport {
    pub root_methods: usize,
    pub linked_methods: usize,
    pub external_methods: Vec<String>,
    pub rejected_methods: Vec<RecursiveRejectedMethod>,
    pub depth_limited_calls: usize,
    pub maximum_depth_reached: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecursiveLifecycleArtifact {
    pub executable: PathBuf,
    pub c_source: String,
    pub report: RecursiveLifecycleReport,
}

#[derive(Debug)]
struct CollectedGraph {
    methods: Vec<BootstrapMethod>,
    method_indices: Vec<usize>,
    report: RecursiveLifecycleReport,
}

#[derive(Debug, Clone)]
pub struct RecursiveLifecycleCompiler {
    bootstrap: BootstrapCompiler,
    linked: LinkedLifecycleCompiler,
    clang: PathBuf,
}

impl Default for RecursiveLifecycleCompiler {
    fn default() -> Self {
        Self {
            bootstrap: BootstrapCompiler::default(),
            linked: LinkedLifecycleCompiler::default(),
            clang: PathBuf::from("clang"),
        }
    }
}

impl RecursiveLifecycleCompiler {
    #[must_use]
    pub fn with_clang(clang: impl Into<PathBuf>) -> Self {
        Self {
            bootstrap: BootstrapCompiler::default(),
            linked: LinkedLifecycleCompiler::default(),
            clang: clang.into(),
        }
    }

    fn collect_graph(
        &self,
        apk: &Path,
        max_depth: usize,
        max_methods: usize,
    ) -> Result<CollectedGraph, BootstrapError> {
        if max_methods < SUKISU_LIFECYCLE_TARGETS.len() {
            return Err(BootstrapError::Apk(
                "o limite precisa comportar os quatro métodos raiz".to_owned(),
            ));
        }

        let roots = SUKISU_LIFECYCLE_TARGETS
            .iter()
            .map(|(_, descriptor)| (*descriptor).to_owned())
            .collect::<Vec<_>>();
        let root_set = roots.iter().cloned().collect::<BTreeSet<_>>();
        let mut queue = VecDeque::new();
        for descriptor in &roots {
            queue.push_back((descriptor.clone(), 0_usize));
        }

        let mut visited = BTreeSet::new();
        let mut linked_methods = Vec::new();
        let mut linked_indices = Vec::new();
        let mut external = BTreeSet::new();
        let mut rejected = BTreeMap::new();
        let mut depth_limited_calls = 0_usize;
        let mut maximum_depth_reached = 0_usize;

        while let Some((descriptor, depth)) = queue.pop_front() {
            if !visited.insert(descriptor.clone()) {
                continue;
            }

            maximum_depth_reached = maximum_depth_reached.max(depth);

            if is_known_external_namespace(&descriptor) {
                external.insert(descriptor);
                continue;
            }

            let Some(method) = find_bootstrap_method_in_apk(apk, &descriptor)? else {
                external.insert(descriptor);
                continue;
            };

            let analysis = match self.bootstrap.analyze(&method) {
                Ok(analysis) => analysis,
                Err(error) => {
                    rejected.insert(descriptor.clone(), error.to_string());
                    continue;
                }
            };

            if !analysis.unsupported.is_empty() {
                let blockers = analysis
                    .unsupported
                    .iter()
                    .take(8)
                    .map(|item| format!("pc={} opcode={:#04x}", item.pc, item.opcode))
                    .collect::<Vec<_>>()
                    .join(", ");
                rejected.insert(
                    descriptor.clone(),
                    format!("opcodes não suportados: {blockers}"),
                );
                continue;
            }

            if !supports_current_argument_abi(&method) {
                rejected.insert(
                    descriptor.clone(),
                    format!(
                        "ABI atual não cobre access_flags={:#x}, ins_size={}",
                        method.access_flags, method.ins_size
                    ),
                );
                continue;
            }

            if !root_set.contains(&descriptor) && !is_safe_recursive_candidate(&method) {
                rejected.insert(
                    descriptor.clone(),
                    format!(
                        "método adiado por segurança: {} code units",
                        method.instructions.len()
                    ),
                );
                continue;
            }

            if let Err(error) = self.bootstrap.emit_c(&method) {
                rejected.insert(descriptor.clone(), error.to_string());
                continue;
            }

            let method_index = method
                .methods
                .iter()
                .position(|candidate| candidate == &descriptor)
                .ok_or_else(|| {
                    BootstrapError::Apk(format!("índice DEX não encontrado para {descriptor}"))
                })?;

            linked_methods.push(method);
            linked_indices.push(method_index);

            if linked_methods.len() >= max_methods {
                break;
            }

            if depth >= max_depth {
                depth_limited_calls =
                    depth_limited_calls.saturating_add(analysis.referenced_methods.len());
                continue;
            }

            for called in analysis.referenced_methods {
                if !visited.contains(&called) {
                    queue.push_back((called, depth + 1));
                }
            }
        }

        for root in &roots {
            if !linked_methods
                .iter()
                .any(|method| &method.descriptor == root)
            {
                let reason = rejected
                    .get(root)
                    .cloned()
                    .unwrap_or_else(|| "método raiz ausente".to_owned());
                return Err(BootstrapError::Apk(format!(
                    "não foi possível ligar o método raiz {root}: {reason}"
                )));
            }
        }

        let mut ordered_methods = Vec::with_capacity(linked_methods.len());
        let mut ordered_indices = Vec::with_capacity(linked_indices.len());

        for root in &roots {
            let position = linked_methods
                .iter()
                .position(|method| &method.descriptor == root)
                .ok_or_else(|| BootstrapError::MethodNotFound(root.clone()))?;
            ordered_methods.push(linked_methods[position].clone());
            ordered_indices.push(linked_indices[position]);
        }

        for (method, method_index) in linked_methods.into_iter().zip(linked_indices) {
            if !root_set.contains(&method.descriptor) {
                ordered_methods.push(method);
                ordered_indices.push(method_index);
            }
        }

        let report = RecursiveLifecycleReport {
            root_methods: roots.len(),
            linked_methods: ordered_methods.len(),
            external_methods: external.into_iter().collect(),
            rejected_methods: rejected
                .into_iter()
                .map(|(descriptor, reason)| RecursiveRejectedMethod { descriptor, reason })
                .collect(),
            depth_limited_calls,
            maximum_depth_reached,
        };

        Ok(CollectedGraph {
            methods: ordered_methods,
            method_indices: ordered_indices,
            report,
        })
    }

    pub fn emit_recursive_c(
        &self,
        methods: &[BootstrapMethod],
        method_indices: &[usize],
    ) -> Result<String, BootstrapError> {
        if methods.len() != method_indices.len() {
            return Err(BootstrapError::Apk(
                "métodos e índices DEX possuem tamanhos diferentes".to_owned(),
            ));
        }

        let source = self.linked.emit_linked_c(methods)?;
        patch_internal_dispatch(source, method_indices)
    }

    pub fn compile_sukisu(
        &self,
        apk: &Path,
        output: &Path,
        emit_c: Option<&Path>,
        max_depth: usize,
        max_methods: usize,
    ) -> Result<RecursiveLifecycleArtifact, BootstrapError> {
        let graph = self.collect_graph(apk, max_depth, max_methods)?;
        self.compile_graph(
            &graph.methods,
            &graph.method_indices,
            graph.report,
            output,
            emit_c,
        )
    }

    pub fn compile_methods(
        &self,
        methods: &[BootstrapMethod],
        method_indices: &[usize],
        output: &Path,
        emit_c: Option<&Path>,
    ) -> Result<RecursiveLifecycleArtifact, BootstrapError> {
        let report = RecursiveLifecycleReport {
            root_methods: methods.len().min(4),
            linked_methods: methods.len(),
            external_methods: Vec::new(),
            rejected_methods: Vec::new(),
            depth_limited_calls: 0,
            maximum_depth_reached: 0,
        };
        self.compile_graph(methods, method_indices, report, output, emit_c)
    }

    fn compile_graph(
        &self,
        methods: &[BootstrapMethod],
        method_indices: &[usize],
        report: RecursiveLifecycleReport,
        output: &Path,
        emit_c: Option<&Path>,
    ) -> Result<RecursiveLifecycleArtifact, BootstrapError> {
        let c_source = self.emit_recursive_c(methods, method_indices)?;

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

        Ok(RecursiveLifecycleArtifact {
            executable: output.to_owned(),
            c_source,
            report,
        })
    }
}

fn is_known_external_namespace(descriptor: &str) -> bool {
    const PREFIXES: [&str; 8] = [
        "Landroid/",
        "Landroidx/",
        "Ljava/",
        "Ljavax/",
        "Lkotlin/",
        "Lkotlinx/",
        "Lorg/jetbrains/",
        "Lorg/json/",
    ];
    PREFIXES.iter().any(|prefix| descriptor.starts_with(prefix))
}

fn is_safe_recursive_candidate(method: &BootstrapMethod) -> bool {
    method.instructions.len() <= 512
        && !method.instructions.iter().any(|unit| (unit & 0xff) == 0x27)
}

fn supports_current_argument_abi(method: &BootstrapMethod) -> bool {
    method.ins_size <= method.registers_size
}

fn patch_internal_dispatch(
    mut source: String,
    method_indices: &[usize],
) -> Result<String, BootstrapError> {
    if !source.contains(INVOKE_SIGNATURE) {
        return Err(BootstrapError::Apk(
            "wd_invoke não encontrado no C ligado".to_owned(),
        ));
    }

    source = source.replacen(INVOKE_SIGNATURE, EXTERNAL_INVOKE_SIGNATURE, 1);

    let insertion = source
        .find(FIRST_LINKED_METHOD)
        .ok_or_else(|| BootstrapError::Apk("primeiro método ligado não encontrado".to_owned()))?;

    let mut dispatch = String::new();
    dispatch.push_str("static __attribute__((unused)) uint32_t wd_recursive_depth = 0;\n");

    for index in 0..method_indices.len() {
        dispatch.push_str(&format!(
            "static wd_value wd_linked_method_{index}(uint32_t argc, const wd_value *args);\n"
        ));
    }

    dispatch.push_str(
        "\nstatic __attribute__((unused)) wd_value wd_invoke(\n\
         \tuint32_t method_index, uint32_t argc, const wd_value *args) {\n\
         \twd_value result = 0;\n\
         \tif (wd_recursive_depth >= 128) {\n\
         \t\tfputs(\"WineDroid: recursive call depth exceeded\\n\", stderr);\n\
         \t\texit(105);\n\
         \t}\n\
         \twd_recursive_depth++;\n\
         \tswitch (method_index) {\n",
    );

    for (linked_index, method_index) in method_indices.iter().copied().enumerate() {
        dispatch.push_str(&format!(
            "\t\tcase {method_index}: fprintf(stderr, \"[WineDroid] internal method_id={method_index} argc=%u\\n\", argc); result = wd_linked_method_{linked_index}(argc, args); break;\n"
        ));
    }

    dispatch.push_str(
        "\t\tdefault: result = wd_invoke_external(method_index, argc, args); break;\n\
         \t}\n\
         \twd_recursive_depth--;\n\
         \treturn result;\n\
         }\n\n",
    );

    source.insert_str(insertion, &dispatch);
    source = source.replace(
        "WineDroid: SukiSU linked lifecycle completed",
        "WineDroid: SukiSU recursive lifecycle completed",
    );
    source = source.replace(
        "[WineDroid] linked SukiSU lifecycle start",
        "[WineDroid] recursive SukiSU lifecycle start",
    );
    source = source.replace(
        "[WineDroid] linked SukiSU lifecycle complete",
        "[WineDroid] recursive SukiSU lifecycle complete",
    );

    Ok(source)
}

fn temporary_c_path() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    std::env::temp_dir().join(format!(
        "winedroid-recursive-{}-{nanos}.c",
        std::process::id()
    ))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    #[test]
    fn inserts_internal_dispatch_cases() {
        let descriptors: Arc<[String]> = Arc::from(vec![
            "Ldemo/App;-><init>()V".to_owned(),
            "Ldemo/App;->onCreate()V".to_owned(),
            "Ldemo/Activity;-><init>()V".to_owned(),
            "Ldemo/Activity;->onCreate(Landroid/os/Bundle;)V".to_owned(),
            "Ldemo/Helper;->answer()I".to_owned(),
        ]);
        let mut methods = Vec::new();
        for (index, descriptor) in descriptors.iter().enumerate() {
            let mut method = BootstrapMethod::demo();
            method.descriptor = descriptor.clone();
            method.methods = Arc::clone(&descriptors);
            method.access_flags = 0;
            method.ins_size = if index == 3 { 2 } else { 1 };
            methods.push(method);
        }
        methods[4].access_flags = 0x0008;
        methods[4].ins_size = 0;

        let source = RecursiveLifecycleCompiler::default()
            .emit_recursive_c(&methods, &[0, 1, 2, 3, 4])
            .unwrap();
        assert!(source.contains("case 4:"));
        assert!(source.contains("wd_invoke_external"));
        assert!(source.contains("recursive lifecycle completed"));
    }
}
