use std::{
    path::PathBuf,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use winedroid_compiler::{BootstrapCompiler, BootstrapMethod};

#[test]
fn compiles_object_fields_to_native_elf() {
    if Command::new("clang").arg("--version").output().is_err() {
        eprintln!("clang não instalado; teste ignorado");
        return;
    }

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let output: PathBuf = std::env::temp_dir().join(format!(
        "winedroid-bootstrap-test-{}-{nanos}",
        std::process::id()
    ));

    BootstrapCompiler::default()
        .compile(&BootstrapMethod::demo(), &output, None)
        .expect("bootstrap demo deveria compilar");

    let result = Command::new(&output)
        .output()
        .expect("ELF deveria executar");
    let _ = std::fs::remove_file(&output);

    assert!(result.status.success());
    assert_eq!(String::from_utf8_lossy(&result.stdout).trim(), "42");
}
