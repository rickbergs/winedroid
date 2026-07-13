use std::{path::PathBuf, process::Command};

use anyhow::{Context, Result, bail};
use clap::Parser;
use winedroid_compiler::LinkedLifecycleCompiler;

#[derive(Debug, Parser)]
#[command(
    name = "winedroid-sukisu-link",
    version,
    about = "Liga o ciclo de vida inicial do SukiSU em um único ELF Linux"
)]
struct Cli {
    /// APK do SukiSU Manager.
    apk: PathBuf,

    /// Caminho do ELF x86-64 gerado.
    #[arg(short, long, default_value = "./winedroid-sukisu-linked")]
    output: PathBuf,

    /// Salva também o código C intermediário auditável.
    #[arg(long)]
    emit_c: Option<PathBuf>,

    /// Executa o ELF após a compilação.
    #[arg(long)]
    run: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let compiler = LinkedLifecycleCompiler::default();
    let artifact = compiler
        .compile_sukisu(&cli.apk, &cli.output, cli.emit_c.as_deref())
        .with_context(|| format!("falha ligando o ciclo de vida de {}", cli.apk.display()))?;

    println!("APK: {}", cli.apk.display());
    println!("Métodos ligados: {}", artifact.method_count);
    println!("ELF único: {}", artifact.executable.display());
    println!("Estado compartilhado: heap, campos estáticos e campos de instância");
    println!("Chamadas externas: stubs rastreáveis do runtime atual");

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
            bail!(
                "o ELF ligado terminou com status {:?}",
                result.status.code()
            );
        }
    }

    Ok(())
}
