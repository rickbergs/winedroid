use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct ApkReport {
    pub path: String,
    pub archive_size: u64,
    pub entries: Vec<EntryInfo>,
    pub manifest: Option<ManifestInfo>,
    pub dex_files: Vec<DexInfo>,
    pub native_libraries: Vec<NativeLibrary>,
    pub has_resources_arsc: bool,
    pub v1_signature_entries: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EntryInfo {
    pub path: String,
    pub kind: EntryKind,
    pub compressed_size: u64,
    pub uncompressed_size: u64,
    pub compression: String,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    Manifest,
    Dex,
    NativeLibrary,
    ResourcesTable,
    Resource,
    Asset,
    SignatureV1,
    Metadata,
    Directory,
    Other,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ManifestFormat {
    AndroidBinaryXml,
    PlainXml,
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
pub struct ManifestInfo {
    pub format: ManifestFormat,
    pub chunk_type: Option<u16>,
    pub header_size: Option<u16>,
    pub declared_size: Option<u32>,
    pub bytes_sampled: usize,
    pub package_name: Option<String>,
    pub version_code: Option<String>,
    pub version_name: Option<String>,
    pub min_sdk: Option<String>,
    pub target_sdk: Option<String>,
    pub application_name: Option<String>,
    pub launcher_activity: Option<String>,
    pub permissions: Vec<String>,
    pub activities: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DexInfo {
    pub path: String,
    pub version: String,
    pub checksum_adler32: u32,
    pub signature_sha1: String,
    pub declared_file_size: u32,
    pub archive_file_size: u64,
    pub header_size: u32,
    pub endian_tag: u32,
    pub string_ids: u32,
    pub type_ids: u32,
    pub method_ids: u32,
    pub class_defs: u32,
    pub data_size: u32,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NativeLibrary {
    pub path: String,
    pub abi: String,
    pub soname: String,
}
