use std::{env, fs, path::PathBuf, process::Command};

use anyhow::Result;
use clap::{Parser, Subcommand};
use winedroid_core::{ApkReport, ManifestFormat, inspect_apk};

#[derive(Debug, Parser)]
#[command(
    name = "winedroid",
    version,
    about = "Camada experimental de compatibilidade Android para Linux"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Inspeciona a estrutura de um APK sem executá-lo.
    Inspect {
        /// Caminho para o arquivo APK.
        apk: PathBuf,

        /// Imprime o relatório completo em JSON.
        #[arg(long)]
        json: bool,
    },

    /// Mostra informações do host úteis ao desenvolvimento do runtime.
    Doctor,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Inspect { apk, json } => {
            let report = inspect_apk(apk)?;

            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                print_human_report(&report);
            }
        }
        Commands::Doctor => print_doctor(),
    }

    Ok(())
}

fn print_human_report(report: &ApkReport) {
    println!("WineDroid APK Inspector");
    println!("APK: {}", report.path);
    println!("Tamanho: {} bytes", report.archive_size);
    println!("Entradas ZIP: {}", report.entries.len());

    match &report.manifest {
        Some(manifest) => {
            let format = match manifest.format {
                ManifestFormat::AndroidBinaryXml => "Android Binary XML",
                ManifestFormat::PlainXml => "XML textual",
                ManifestFormat::Unknown => "desconhecido",
            };
            println!("Manifesto: {format}");

            if let Some(size) = manifest.declared_size {
                println!("  tamanho declarado: {size} bytes");
            }
            if let Some(package) = &manifest.package_name {
                println!("  pacote: {package}");
            }
            if let Some(version_code) = &manifest.version_code {
                println!("  versionCode: {version_code}");
            }
            if let Some(version_name) = &manifest.version_name {
                println!("  versionName: {version_name}");
            }
            if manifest.min_sdk.is_some() || manifest.target_sdk.is_some() {
                println!(
                    "  SDK: min={} target={}",
                    manifest.min_sdk.as_deref().unwrap_or("?"),
                    manifest.target_sdk.as_deref().unwrap_or("?")
                );
            }
            if let Some(application) = &manifest.application_name {
                println!("  Application: {application}");
            }
            if let Some(launcher) = &manifest.launcher_activity {
                println!("  launcher: {launcher}");
            }
            println!("  activities: {}", manifest.activities.len());
            println!("  permissions: {}", manifest.permissions.len());
            for warning in &manifest.warnings {
                println!("  aviso AXML: {warning}");
            }
        }
        None => println!("Manifesto: ausente"),
    }

    println!("DEX: {}", report.dex_files.len());
    for dex in &report.dex_files {
        println!(
            "  {}: v{}, {} classes, {} métodos, {} campos, {} protótipos, {} strings",
            dex.path,
            dex.version,
            dex.parsed_classes,
            dex.parsed_methods,
            dex.parsed_fields,
            dex.parsed_protos,
            dex.parsed_strings
        );

        for warning in &dex.warnings {
            println!("    aviso: {warning}");
        }
    }

    if let Some(first_dex) = report.dex_files.first() {
        if !first_dex.class_samples.is_empty() {
            println!("Amostra de classes de {}:", first_dex.path);
            for class in &first_dex.class_samples {
                println!("  {class}");
            }
        }
        if !first_dex.method_samples.is_empty() {
            println!("Amostra de métodos de {}:", first_dex.path);
            for method in &first_dex.method_samples {
                println!("  {method}");
            }
        }
    }

    println!("Bibliotecas nativas: {}", report.native_libraries.len());
    for library in &report.native_libraries {
        println!("  {} [{}]", library.soname, library.abi);
    }

    println!(
        "Tabela resources.arsc: {}",
        if report.has_resources_arsc {
            "presente"
        } else {
            "ausente"
        }
    );
    println!(
        "Entradas de assinatura v1: {}",
        report.v1_signature_entries.len()
    );

    if !report.warnings.is_empty() {
        println!("Avisos:");
        for warning in &report.warnings {
            println!("  - {warning}");
        }
    }
}

fn print_doctor() {
    println!("WineDroid Doctor");
    println!("host.os = {}", env::consts::OS);
    println!("host.arch = {}", env::consts::ARCH);
    println!(
        "host.kernel = {}",
        read_trimmed("/proc/sys/kernel/osrelease").unwrap_or_else(|| "desconhecido".to_owned())
    );
    println!(
        "session.type = {}",
        env::var("XDG_SESSION_TYPE").unwrap_or_else(|_| "não definido".to_owned())
    );
    println!(
        "wayland.display = {}",
        env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "não definido".to_owned())
    );
    println!(
        "display.x11 = {}",
        env::var("DISPLAY").unwrap_or_else(|_| "não definido".to_owned())
    );
    println!(
        "host.libc = {}",
        command_first_line("ldd", &["--version"]).unwrap_or_else(|| "não detectada".to_owned())
    );
}

fn read_trimmed(path: &str) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|value| value.trim().to_owned())
}

fn command_first_line(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program).args(args).output().ok()?;
    let text = if output.stdout.is_empty() {
        String::from_utf8(output.stderr).ok()?
    } else {
        String::from_utf8(output.stdout).ok()?
    };

    text.lines().next().map(str::to_owned)
}
