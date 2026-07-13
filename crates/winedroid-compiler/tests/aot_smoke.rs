use std::{
    path::PathBuf,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use winedroid_compiler::{AotCompiler, DalvikProgram};

#[test]
fn compiles_and_executes_a_real_native_elf() {
    if Command::new("clang").arg("--version").output().is_err() {
        eprintln!("clang não está instalado; teste AOT ignorado");
        return;
    }

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let output: PathBuf =
        std::env::temp_dir().join(format!("winedroid-aot-test-{}-{nanos}", std::process::id()));

    AotCompiler::default()
        .compile(&DalvikProgram::demo(), &output, None)
        .expect("compilação AOT deveria funcionar");

    let result = Command::new(&output)
        .output()
        .expect("ELF gerado deveria executar");
    let _ = std::fs::remove_file(&output);

    assert!(result.status.success());
    assert_eq!(String::from_utf8_lossy(&result.stdout).trim(), "42");
}
