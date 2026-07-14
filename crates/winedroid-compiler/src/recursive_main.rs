use std::{path::PathBuf, process::Command};

use anyhow::{Context, Result, bail};
use clap::Parser;
use winedroid_compiler::RecursiveLifecycleCompiler;

#[derive(Debug, Parser)]
#[command(
    name = "winedroid-sukisu-recursive",
    version,
    about = "Liga recursivamente chamadas DEX internas do SukiSU"
)]
struct Cli {
    apk: PathBuf,

    #[arg(short, long, default_value = "./winedroid-sukisu-recursive.elf")]
    output: PathBuf,

    #[arg(long)]
    emit_c: Option<PathBuf>,

    #[arg(long, default_value_t = 4)]
    max_depth: usize,

    #[arg(long, default_value_t = 192)]
    max_methods: usize,

    #[arg(long)]
    run: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    if cli.max_methods < 4 {
        bail!("--max-methods deve ser ao menos 4");
    }

    let artifact = RecursiveLifecycleCompiler::default()
        .compile_sukisu(
            &cli.apk,
            &cli.output,
            cli.emit_c.as_deref(),
            cli.max_depth,
            cli.max_methods,
        )
        .with_context(|| format!("falha ligando {}", cli.apk.display()))?;

    println!("APK: {}", cli.apk.display());
    println!("Métodos raiz: {}", artifact.report.root_methods);
    println!(
        "Métodos internos ligados: {}",
        artifact.report.linked_methods
    );
    println!(
        "Chamadas externas mantidas em stub: {}",
        artifact.report.external_methods.len()
    );
    println!(
        "Métodos internos rejeitados: {}",
        artifact.report.rejected_methods.len()
    );
    println!(
        "Chamadas cortadas pelo limite de profundidade: {}",
        artifact.report.depth_limited_calls
    );
    println!(
        "Profundidade máxima alcançada: {}",
        artifact.report.maximum_depth_reached
    );

    for rejected in artifact.report.rejected_methods.iter().take(12) {
        println!("  rejeitado {}: {}", rejected.descriptor, rejected.reason);
    }

    println!("ELF recursivo: {}", artifact.executable.display());

    if cli.run {
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
            bail!("ELF terminou com status {:?}", result.status.code());
        }
    }

    Ok(())
}
