use winedroid_compiler::{BootstrapMethod, LinkedLifecycleCompiler};

fn methods_with_four_inputs() -> Vec<BootstrapMethod> {
    let descriptors = [
        "Ldemo/Application;-><init>()V",
        "Ldemo/Application;->onCreate()V",
        "Ldemo/Activity;-><init>()V",
        "Ldemo/Activity;->onCreate(III)V",
    ];

    descriptors
        .iter()
        .enumerate()
        .map(|(index, descriptor)| {
            let mut method = BootstrapMethod::demo();
            method.descriptor = (*descriptor).to_owned();
            method.access_flags = 0;
            method.registers_size = if index == 3 { 7 } else { 3 };
            method.ins_size = if index == 3 { 4 } else { 1 };
            method
        })
        .collect()
}

#[test]
fn linked_functions_receive_all_dalvik_input_registers() {
    let source = LinkedLifecycleCompiler::default()
        .emit_linked_c(&methods_with_four_inputs())
        .expect("ABI genérica deveria gerar C");

    assert!(source.contains("wd_linked_method_3(uint32_t argc, const wd_value *args)"));
    assert!(source.contains("argc > 3"));
    assert!(source.contains("args[3]"));
    assert!(source.contains("activity_create_args"));
}
