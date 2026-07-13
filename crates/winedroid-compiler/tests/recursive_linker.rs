use std::{
    path::PathBuf,
    process::Command,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use winedroid_compiler::{BootstrapMethod, RecursiveLifecycleCompiler};

#[test]
fn executes_an_internal_dex_call_in_the_same_elf() {
    if Command::new("clang").arg("--version").output().is_err() {
        eprintln!("clang não instalado; teste ignorado");
        return;
    }

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

    methods[1].registers_size = 2;
    methods[1].instructions = vec![0x0071, 4, 0, 0x010a, 0x010f];
    methods[4].access_flags = 0x0008;
    methods[4].registers_size = 1;
    methods[4].ins_size = 0;
    methods[4].instructions = vec![0x0013, 42, 0x000f];

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let output: PathBuf = std::env::temp_dir().join(format!(
        "winedroid-recursive-test-{}-{nanos}",
        std::process::id()
    ));

    RecursiveLifecycleCompiler::default()
        .compile_methods(&methods, &[0, 1, 2, 3, 4], &output, None)
        .expect("grafo recursivo deveria compilar");

    let result = Command::new(&output)
        .output()
        .expect("ELF recursivo deveria executar");
    let _ = std::fs::remove_file(&output);

    assert!(result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("internal method_id=4"));
    assert!(String::from_utf8_lossy(&result.stdout).contains("recursive lifecycle completed"));
}
