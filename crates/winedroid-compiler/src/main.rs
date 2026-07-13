use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use winedroid_compiler::{
    AotCompiler, BootstrapCompiler, BootstrapMethod, DalvikProgram, find_bootstrap_method_in_apk,
    find_method_in_apk, find_method_in_dex, scan_apk_methods,
};

#[derive(Debug, Parser)]
#[command(
    name = "winedroid-aot",
    version,
    about = "Compilador AOT Dalvik para executáveis Linux nativos"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Demo {
        #[arg(short, long, default_value = "./winedroid-native-demo")]
        output: PathBuf,
        #[arg(long)]
        emit_c: Option<PathBuf>,
        #[arg(long)]
        run: bool,
    },
    CompileDex {
        dex: PathBuf,
        #[arg(long)]
        method: String,
        #[arg(short, long)]
        output: PathBuf,
        #[arg(long)]
        emit_c: Option<PathBuf>,
        #[arg(long)]
        default_static_int: Option<i32>,
        #[arg(long = "static-field", value_name = "DESCRIPTOR=VALUE")]
        static_fields: Vec<String>,
        #[arg(long)]
        run: bool,
    },
    CompileApk {
        apk: PathBuf,
        #[arg(long)]
        method: String,
        #[arg(short, long)]
        output: PathBuf,
        #[arg(long)]
        emit_c: Option<PathBuf>,
        #[arg(long)]
        default_static_int: Option<i32>,
        #[arg(long = "static-field", value_name = "DESCRIPTOR=VALUE")]
        static_fields: Vec<String>,
        #[arg(long)]
        run: bool,
    },
    ScanApk {
        apk: PathBuf,
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    BootstrapDemo {
        #[arg(short, long, default_value = "./winedroid-object-demo")]
        output: PathBuf,
        #[arg(long)]
        emit_c: Option<PathBuf>,
        #[arg(long)]
        run: bool,
    },
    BootstrapApk {
        apk: PathBuf,
        #[arg(long)]
        method: String,
        #[arg(short, long)]
        output: PathBuf,
        #[arg(long)]
        emit_c: Option<PathBuf>,
        #[arg(long)]
        run: bool,
    },
    SukisuFrontier {
        apk: PathBuf,
        #[arg(long, default_value = "/tmp/winedroid-sukisu-bootstrap")]
        output_dir: PathBuf,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let compiler = AotCompiler::default();
    let bootstrap = BootstrapCompiler::default();

    match cli.command {
        Commands::Demo {
            output,
            emit_c,
            run,
        } => {
            compile_and_optionally_run(
                &compiler,
                &DalvikProgram::demo(),
                &output,
                emit_c.as_deref(),
                run,
            )?;
        }
        Commands::CompileDex {
            dex,
            method,
            output,
            emit_c,
            default_static_int,
            static_fields,
            run,
        } => {
            let bytes = fs::read(&dex)
                .with_context(|| format!("não foi possível ler {}", dex.display()))?;
            let logical_name = dex.to_string_lossy();
            let body = find_method_in_dex(&logical_name, &bytes, &method)
                .map_err(anyhow::Error::msg)?
                .with_context(|| format!("método não encontrado: {method}"))?;
            let program = prepare_program(body.into(), default_static_int, &static_fields)?;
            compile_and_optionally_run(&compiler, &program, &output, emit_c.as_deref(), run)?;
        }
        Commands::CompileApk {
            apk,
            method,
            output,
            emit_c,
            default_static_int,
            static_fields,
            run,
        } => {
            let body = find_method_in_apk(&apk, &method)
                .map_err(anyhow::Error::msg)?
                .with_context(|| format!("método não encontrado no APK: {method}"))?;
            let program = prepare_program(body.into(), default_static_int, &static_fields)?;
            compile_and_optionally_run(&compiler, &program, &output, emit_c.as_deref(), run)?;
        }
        Commands::ScanApk { apk, limit } => {
            let methods = scan_apk_methods(&apk, limit).map_err(anyhow::Error::msg)?;
            for method in methods {
                println!(
                    "{}  [{} | {} regs | {} code units]",
                    method.descriptor,
                    method.dex_path,
                    method.registers_size,
                    method.instructions.len()
                );
            }
        }
        Commands::BootstrapDemo {
            output,
            emit_c,
            run,
        } => {
            bootstrap_compile_and_run(
                &bootstrap,
                &BootstrapMethod::demo(),
                &output,
                emit_c.as_deref(),
                run,
            )?;
        }
        Commands::BootstrapApk {
            apk,
            method,
            output,
            emit_c,
            run,
        } => {
            let body = find_bootstrap_method_in_apk(&apk, &method)?
                .with_context(|| format!("método não encontrado: {method}"))?;
            print_bootstrap_report(&bootstrap, &body)?;
            bootstrap_compile_and_run(&bootstrap, &body, &output, emit_c.as_deref(), run)?;
        }
        Commands::SukisuFrontier { apk, output_dir } => {
            run_sukisu_frontier(&bootstrap, &apk, &output_dir)?;
        }
    }

    Ok(())
}

fn prepare_program(
    mut program: DalvikProgram,
    default_static_int: Option<i32>,
    static_fields: &[String],
) -> Result<DalvikProgram> {
    if let Some(value) = default_static_int {
        program.set_all_static_i32(value);
    }

    for override_value in static_fields {
        let (descriptor, raw_value) = override_value
            .rsplit_once('=')
            .with_context(|| format!("override inválido: {override_value:?}"))?;
        let value = raw_value
            .parse::<i32>()
            .with_context(|| format!("valor inválido: {raw_value}"))?;
        if !program.set_static_i32(descriptor, value) {
            bail!("campo estático não encontrado: {descriptor}");
        }
    }

    Ok(program)
}

fn compile_and_optionally_run(
    compiler: &AotCompiler,
    program: &DalvikProgram,
    output: &Path,
    emit_c: Option<&Path>,
    run: bool,
) -> Result<()> {
    let artifact = compiler
        .compile(program, output, emit_c)
        .with_context(|| format!("falha compilando {}", program.descriptor))?;

    println!("Método: {}", program.descriptor);
    println!("ELF nativo: {}", artifact.executable.display());

    if run {
        run_binary(&artifact.executable)?;
    }

    Ok(())
}

fn bootstrap_compile_and_run(
    compiler: &BootstrapCompiler,
    method: &BootstrapMethod,
    output: &Path,
    emit_c: Option<&Path>,
    run: bool,
) -> Result<()> {
    compiler
        .compile(method, output, emit_c)
        .with_context(|| format!("falha no bootstrap de {}", method.descriptor))?;

    println!("Método bootstrap: {}", method.descriptor);
    println!("ELF nativo: {}", output.display());

    if run {
        run_binary(output)?;
    }

    Ok(())
}

fn print_bootstrap_report(compiler: &BootstrapCompiler, method: &BootstrapMethod) -> Result<()> {
    let report = compiler.analyze(method)?;
    println!("Método: {}", report.descriptor);
    println!(
        "Frame: {} registradores, {} entradas, {} code units",
        report.registers_size, report.ins_size, report.instruction_count
    );
    println!("Chamadas resolvidas: {}", report.referenced_methods.len());
    for called in report.referenced_methods.iter().take(20) {
        println!("  invoke {called}");
    }
    println!("Campos resolvidos: {}", report.referenced_fields.len());
    println!("Strings resolvidas: {}", report.referenced_strings.len());

    if report.unsupported.is_empty() {
        println!("Cobertura AOT: método compilável pelo backend atual");
    } else {
        println!("Cobertura AOT: {} bloqueios", report.unsupported.len());
        for item in report.unsupported.iter().take(20) {
            println!("  pc={} opcode={:#04x}", item.pc, item.opcode);
        }
    }

    Ok(())
}

fn run_sukisu_frontier(compiler: &BootstrapCompiler, apk: &Path, output_dir: &Path) -> Result<()> {
    fs::create_dir_all(output_dir)?;
    let targets = [
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

    let mut found = 0_usize;
    let mut compiled = 0_usize;

    for (name, descriptor) in targets {
        println!("\n=== {descriptor} ===");
        let Some(method) = find_bootstrap_method_in_apk(apk, descriptor)? else {
            println!("não encontrado no APK");
            continue;
        };
        found += 1;
        print_bootstrap_report(compiler, &method)?;

        let output = output_dir.join(name);
        let c_source = output_dir.join(format!("{name}.c"));

        match compiler.compile(&method, &output, Some(&c_source)) {
            Ok(()) => {
                compiled += 1;
                println!("ELF gerado: {}", output.display());
                let result = Command::new(&output).output()?;
                print!("{}", String::from_utf8_lossy(&result.stdout));
                eprint!("{}", String::from_utf8_lossy(&result.stderr));
                println!("status: {:?}", result.status.code());
            }
            Err(error) => {
                println!("fronteira atual: {error}");
            }
        }
    }

    println!("\nResumo SukiSU: {found} métodos encontrados, {compiled} compilados para ELF");
    Ok(())
}

fn run_binary(path: &Path) -> Result<()> {
    let result = Command::new(path)
        .output()
        .with_context(|| format!("não foi possível executar {}", path.display()))?;

    print!("{}", String::from_utf8_lossy(&result.stdout));
    eprint!("{}", String::from_utf8_lossy(&result.stderr));

    if !result.status.success() {
        bail!("ELF terminou com status {:?}", result.status.code());
    }

    Ok(())
}
