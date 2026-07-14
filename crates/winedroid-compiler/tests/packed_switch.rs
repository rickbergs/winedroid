use std::{
    fs,
    process::Command,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use winedroid_compiler::{BootstrapCompiler, BootstrapMethod};

fn packed_switch_demo(value: i16) -> BootstrapMethod {
    BootstrapMethod {
        dex_path: "packed-switch-test.dex".to_owned(),
        descriptor: "Ldev/winedroid/PackedSwitch;->select()I".to_owned(),
        access_flags: 0x0008,
        registers_size: 2,
        ins_size: 0,
        outs_size: 0,
        instructions: vec![
            0x0013,
            value as u16,
            0x002b,
            6,
            0,
            0x0113,
            9,
            0x010f,
            0x0100,
            2,
            0,
            0,
            3,
            0,
            14,
            0,
            0x0113,
            42,
            0x010f,
        ],
        methods: Arc::from(Vec::<String>::new()),
        fields: Arc::from(Vec::<String>::new()),
        field_types: Arc::from(Vec::<String>::new()),
        strings: Arc::from(Vec::<String>::new()),
        types: Arc::from(Vec::<String>::new()),
    }
}

fn temporary_elf() -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    std::env::temp_dir().join(format!(
        "winedroid-packed-switch-{}-{nanos}.elf",
        std::process::id()
    ))
}

#[test]
fn compiles_and_executes_packed_switch() {
    let method = packed_switch_demo(1);
    let compiler = BootstrapCompiler::default();
    let source = compiler
        .emit_c(&method)
        .expect("packed-switch deveria baixar para C");

    assert!(source.contains("switch ((int32_t)v[0])"));
    assert!(source.contains("case INT32_C(0): goto L5"));
    assert!(source.contains("case INT32_C(1): goto L16"));
    assert!(source.contains("default: goto L5"));
    assert!(!source.contains("L8:"));

    let elf = temporary_elf();
    compiler
        .compile(&method, &elf, None)
        .expect("packed-switch deveria compilar para ELF");

    let output = Command::new(&elf)
        .output()
        .expect("ELF de packed-switch deveria executar");
    let _ = fs::remove_file(&elf);

    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "42");
}
