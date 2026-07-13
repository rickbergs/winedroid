use std::{
    path::PathBuf,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use winedroid_compiler::{BootstrapMethod, LinkedLifecycleCompiler};

#[test]
fn compiles_four_methods_into_one_native_process() {
    if Command::new("clang").arg("--version").output().is_err() {
        eprintln!("clang não instalado; teste ignorado");
        return;
    }

    let descriptors = [
        "Ldemo/Application;-><init>()V",
        "Ldemo/Application;->onCreate()V",
        "Ldemo/Activity;-><init>()V",
        "Ldemo/Activity;->onCreate(Landroid/os/Bundle;)V",
    ];
    let methods = descriptors
        .iter()
        .enumerate()
        .map(|(index, descriptor)| {
            let mut method = BootstrapMethod::demo();
            method.descriptor = (*descriptor).to_owned();
            method.access_flags = 0;
            method.ins_size = if index == 3 { 2 } else { 1 };
            method
        })
        .collect::<Vec<_>>();

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let output: PathBuf = std::env::temp_dir().join(format!(
        "winedroid-linked-test-{}-{nanos}",
        std::process::id()
    ));

    LinkedLifecycleCompiler::default()
        .compile_methods(&methods, &output, None)
        .expect("quatro métodos deveriam virar um ELF");

    let result = Command::new(&output)
        .output()
        .expect("ELF ligado deveria executar");
    let _ = std::fs::remove_file(&output);

    assert!(result.status.success());
    assert!(String::from_utf8_lossy(&result.stdout).contains("SukiSU linked lifecycle completed"));
}
