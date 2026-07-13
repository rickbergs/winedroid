use std::{
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use winedroid_compiler::{
    AotCompiler, DalvikProgram, find_method_in_apk, find_method_in_dex, scan_apk_methods,
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
    /// Compila um método Dalvik controlado para ELF e comprova o retorno 42.
    Demo {
        #[arg(short, long, default_value = "./winedroid-native-demo")]
        output: PathBuf,
        #[arg(long)]
        emit_c: Option<PathBuf>,
        #[arg(long)]
        run: bool,
    },
    /// Compila um método zero-argumento existente em um arquivo DEX.
    CompileDex {
        dex: PathBuf,
        #[arg(long)]
        method: String,
        #[arg(short, long)]
        output: PathBuf,
        #[arg(long)]
        emit_c: Option<PathBuf>,
        #[arg(long)]
        run: bool,
    },
    /// Procura e compila um método zero-argumento dentro de um APK multidex.
    CompileApk {
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
    /// Lista métodos com corpo e sem argumentos, candidatos ao backend inicial.
    ScanApk {
        apk: PathBuf,
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let compiler = AotCompiler::default();

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
            run,
        } => {
            let bytes = std::fs::read(&dex)
                .with_context(|| format!("não foi possível ler {}", dex.display()))?;
            let logical_name = dex.to_string_lossy();
            let body = find_method_in_dex(&logical_name, &bytes, &method)
                .map_err(anyhow::Error::msg)?
                .with_context(|| format!("método não encontrado: {method}"))?;
            compile_and_optionally_run(&compiler, &body.into(), &output, emit_c.as_deref(), run)?;
        }
        Commands::CompileApk {
            apk,
            method,
            output,
            emit_c,
            run,
        } => {
            let body = find_method_in_apk(&apk, &method)
                .map_err(anyhow::Error::msg)?
                .with_context(|| format!("método não encontrado no APK: {method}"))?;
            compile_and_optionally_run(&compiler, &body.into(), &output, emit_c.as_deref(), run)?;
        }
        Commands::ScanApk { apk, limit } => {
            let methods = scan_apk_methods(&apk, limit).map_err(anyhow::Error::msg)?;
            if methods.is_empty() {
                println!("Nenhum método zero-argumento com corpo foi encontrado.");
            } else {
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
        }
    }

    Ok(())
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
    println!("Backend: Dalvik → C → Clang AOT → ELF");

    if run {
        let result = Command::new(&artifact.executable)
            .output()
            .with_context(|| {
                format!(
                    "não foi possível executar {}",
                    artifact.executable.display()
                )
            })?;

        print!("{}", String::from_utf8_lossy(&result.stdout));
        eprint!("{}", String::from_utf8_lossy(&result.stderr));

        if !result.status.success() {
            bail!(
                "o executável nativo terminou com status {:?}",
                result.status.code()
            );
        }
    }

    Ok(())
}
