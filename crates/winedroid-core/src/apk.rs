use std::{
    fs::{self, File},
    io::Read,
    path::Path,
};

use anyhow::{Context, Result};
use zip::ZipArchive;

use crate::{
    axml::inspect_manifest,
    dex::inspect_dex,
    model::{ApkReport, EntryInfo, EntryKind, NativeLibrary},
};

const MANIFEST_SAMPLE_LIMIT: u64 = 1024 * 1024;
const MAX_DEX_FILE_SIZE: u64 = 256 * 1024 * 1024;

pub fn inspect_apk(path: impl AsRef<Path>) -> Result<ApkReport> {
    let path = path.as_ref();
    let metadata = fs::metadata(path)
        .with_context(|| format!("não foi possível consultar {}", path.display()))?;
    let file =
        File::open(path).with_context(|| format!("não foi possível abrir {}", path.display()))?;
    let mut archive = ZipArchive::new(file)
        .with_context(|| format!("{} não é um APK/ZIP legível", path.display()))?;

    let mut report = ApkReport {
        path: path.display().to_string(),
        archive_size: metadata.len(),
        entries: Vec::with_capacity(archive.len()),
        manifest: None,
        dex_files: Vec::new(),
        native_libraries: Vec::new(),
        has_resources_arsc: false,
        v1_signature_entries: Vec::new(),
        warnings: Vec::new(),
    };

    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .with_context(|| format!("falha ao ler a entrada ZIP de índice {index}"))?;

        let name = entry.name().to_owned();
        let kind = classify_entry(&name, entry.is_dir());
        let compressed_size = entry.compressed_size();
        let uncompressed_size = entry.size();
        let compression = format!("{:?}", entry.compression());

        if entry.enclosed_name().is_none() {
            report.warnings.push(format!(
                "entrada contém caminho inseguro ou absoluto: {name:?}"
            ));
        }

        match kind {
            EntryKind::Manifest => {
                if report.manifest.is_some() {
                    report
                        .warnings
                        .push("mais de um AndroidManifest.xml foi encontrado".to_owned());
                } else {
                    let bytes = read_limited(&mut entry, MANIFEST_SAMPLE_LIMIT)
                        .with_context(|| "falha ao ler AndroidManifest.xml")?;
                    report.manifest = Some(inspect_manifest(&bytes));
                }
            }
            EntryKind::Dex => {
                if uncompressed_size > MAX_DEX_FILE_SIZE {
                    report.warnings.push(format!(
                        "{name}: DEX de {uncompressed_size} bytes excede o limite de segurança de {MAX_DEX_FILE_SIZE} bytes"
                    ));
                } else {
                    let mut bytes = Vec::new();
                    entry
                        .read_to_end(&mut bytes)
                        .with_context(|| format!("falha ao ler o DEX completo {name}"))?;

                    match inspect_dex(&name, &bytes, uncompressed_size) {
                        Ok(dex) => report.dex_files.push(dex),
                        Err(error) => report.warnings.push(error),
                    }
                }
            }
            EntryKind::NativeLibrary => {
                if let Some(library) = parse_native_library(&name) {
                    report.native_libraries.push(library);
                } else {
                    report.warnings.push(format!(
                        "caminho de biblioteca nativa não reconhecido: {name}"
                    ));
                }
            }
            EntryKind::ResourcesTable => {
                report.has_resources_arsc = true;
            }
            EntryKind::SignatureV1 => {
                report.v1_signature_entries.push(name.clone());
            }
            _ => {}
        }

        report.entries.push(EntryInfo {
            path: name,
            kind,
            compressed_size,
            uncompressed_size,
            compression,
        });
    }

    report.dex_files.sort_by_key(|dex| dex_sort_key(&dex.path));
    report.native_libraries.sort_by(|left, right| {
        left.abi
            .cmp(&right.abi)
            .then_with(|| left.soname.cmp(&right.soname))
    });

    if report.manifest.is_none() {
        report
            .warnings
            .push("AndroidManifest.xml não foi encontrado".to_owned());
    }

    if report.dex_files.is_empty() && report.native_libraries.is_empty() {
        report
            .warnings
            .push("nenhum classes*.dex nem biblioteca nativa foi encontrado".to_owned());
    }

    Ok(report)
}

fn read_limited(reader: &mut impl Read, limit: u64) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.take(limit).read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn classify_entry(name: &str, is_directory: bool) -> EntryKind {
    if is_directory {
        return EntryKind::Directory;
    }

    if name == "AndroidManifest.xml" {
        EntryKind::Manifest
    } else if is_root_dex(name) {
        EntryKind::Dex
    } else if name == "resources.arsc" {
        EntryKind::ResourcesTable
    } else if name.starts_with("lib/") && name.ends_with(".so") {
        EntryKind::NativeLibrary
    } else if is_v1_signature_entry(name) {
        EntryKind::SignatureV1
    } else if name.starts_with("META-INF/") {
        EntryKind::Metadata
    } else if name.starts_with("res/") {
        EntryKind::Resource
    } else if name.starts_with("assets/") {
        EntryKind::Asset
    } else {
        EntryKind::Other
    }
}

fn is_root_dex(name: &str) -> bool {
    if name.contains('/') {
        return false;
    }

    if name == "classes.dex" {
        return true;
    }

    name.strip_prefix("classes")
        .and_then(|rest| rest.strip_suffix(".dex"))
        .is_some_and(|number| {
            !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit())
        })
}

fn dex_sort_key(name: &str) -> u32 {
    if name == "classes.dex" {
        return 1;
    }

    name.strip_prefix("classes")
        .and_then(|rest| rest.strip_suffix(".dex"))
        .and_then(|number| number.parse::<u32>().ok())
        .unwrap_or(u32::MAX)
}

fn is_v1_signature_entry(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();

    upper.starts_with("META-INF/")
        && (upper.ends_with(".RSA")
            || upper.ends_with(".DSA")
            || upper.ends_with(".EC")
            || upper.ends_with(".SF")
            || upper == "META-INF/MANIFEST.MF")
}

fn parse_native_library(path: &str) -> Option<NativeLibrary> {
    let mut parts = path.split('/');

    if parts.next()? != "lib" {
        return None;
    }

    let abi = parts.next()?;
    let soname = parts.next()?;

    if parts.next().is_some() || abi.is_empty() || !soname.ends_with(".so") {
        return None;
    }

    Some(NativeLibrary {
        path: path.to_owned(),
        abi: abi.to_owned(),
        soname: soname.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_multidex_names() {
        assert!(is_root_dex("classes.dex"));
        assert!(is_root_dex("classes2.dex"));
        assert!(is_root_dex("classes999.dex"));
        assert!(!is_root_dex("classesx.dex"));
        assert!(!is_root_dex("assets/classes.dex"));
    }

    #[test]
    fn sorts_multidex_numerically() {
        assert!(dex_sort_key("classes.dex") < dex_sort_key("classes2.dex"));
        assert!(dex_sort_key("classes2.dex") < dex_sort_key("classes10.dex"));
    }

    #[test]
    fn parses_standard_native_library_path() {
        let library = parse_native_library("lib/x86_64/libdemo.so").unwrap();
        assert_eq!(library.abi, "x86_64");
        assert_eq!(library.soname, "libdemo.so");
    }
}
