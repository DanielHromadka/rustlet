use base64::{engine::general_purpose::STANDARD, Engine};
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::{
    io::Read,
    process::{Command, Stdio},
};

pub const MAX_ARCHIVE: u64 = 256 * 1024 * 1024;
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Tool {
    pub id: String,
    pub name: String,
    pub description: String,
    pub bundle_id: String,
    pub bundle_name: String,
    pub symbol: String,
    pub manifest_url: String,
    pub archive_prefix: String,
    pub public_key: String,
    pub team_id: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Release {
    pub schema: u32,
    pub id: String,
    pub bundle_id: String,
    pub version: String,
    pub build: u64,
    pub minimum_macos: String,
    pub architectures: Vec<String>,
    pub archive_url: String,
    pub archive_size: u64,
    pub archive_signature: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    payload: String,
    signature: String,
}
pub fn catalog() -> Vec<Tool> {
    serde_json::from_str(include_str!("../resources/catalog.json"))
        .expect("Invalid embedded catalog")
}
pub fn tool(id: &str) -> Result<Tool, String> {
    catalog()
        .into_iter()
        .find(|t| t.id == id)
        .ok_or_else(|| "Neznámý nástroj.".into())
}
pub fn verify(data: &[u8], signature: &str, public: &str) -> Result<(), String> {
    let key: [u8; 32] = STANDARD
        .decode(public)
        .map_err(|_| "Neplatný veřejný klíč.")?
        .try_into()
        .map_err(|_| "Neplatná délka klíče.")?;
    let key = VerifyingKey::from_bytes(&key).map_err(|_| "Neplatný veřejný klíč.")?;
    let signature =
        Signature::from_slice(&STANDARD.decode(signature).map_err(|_| "Neplatný podpis.")?)
            .map_err(|_| "Neplatná délka podpisu.")?;
    key.verify_strict(data, &signature)
        .map_err(|_| "Podpis vydavatele nesouhlasí. Instalace byla zastavena.".into())
}
pub fn decode(tool: &Tool, bytes: &[u8]) -> Result<Release, String> {
    if bytes.len() > 65536 {
        return Err("Informace o verzi jsou příliš velké.".into());
    }
    let envelope: Envelope =
        serde_json::from_slice(bytes).map_err(|_| "Neplatné informace o verzi.")?;
    let payload = STANDARD
        .decode(envelope.payload)
        .map_err(|_| "Neplatná data verze.")?;
    verify(&payload, &envelope.signature, &tool.public_key)?;
    let release: Release =
        serde_json::from_slice(&payload).map_err(|_| "Nepodporovaný formát verze.")?;
    if release.schema != 1
        || release.id != tool.id
        || release.bundle_id != tool.bundle_id
        || release.build == 0
        || release.version.is_empty()
        || release.version.len() > 40
        || !release.archive_url.starts_with(&tool.archive_prefix)
        || !release.archive_url.starts_with("https://")
        || release.archive_url.len() > 2048
        || release.archive_size == 0
        || release.archive_size > MAX_ARCHIVE
    {
        return Err("Vydání neodpovídá nástroji v katalogu.".into());
    }
    Ok(release)
}
fn os_version(value: &str) -> Result<(u32, u32, u32), String> {
    let parts = value
        .split('.')
        .map(str::parse::<u32>)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "Neplatná verze macOS.")?;
    if parts.is_empty() || parts.len() > 3 {
        return Err("Neplatná verze macOS.".into());
    }
    Ok((
        parts[0],
        *parts.get(1).unwrap_or(&0),
        *parts.get(2).unwrap_or(&0),
    ))
}
pub fn compatible(release: &Release, os: &str, arch: &str) -> Result<(), String> {
    if !release.architectures.iter().any(|a| a == arch) {
        return Err("Tato verze není dostupná pro procesor tohoto Macu.".into());
    }
    if os_version(os)? < os_version(&release.minimum_macos)? {
        return Err(format!(
            "Vyžaduje macOS {} nebo novější.",
            release.minimum_macos
        ));
    }
    Ok(())
}
pub fn download(url: &str, maximum: u64) -> Result<Vec<u8>, String> {
    if !url.starts_with("https://") {
        return Err("Stahování vyžaduje HTTPS.".into());
    }
    // macOS supplies curl. Ignore user curlrc; never invoke a shell or allow TLS downgrade.
    let mut child = Command::new("/usr/bin/curl")
        .args([
            "--disable",
            "--globoff",
            "--silent",
            "--show-error",
            "--fail",
            "--location",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--max-redirs",
            "5",
            "--connect-timeout",
            "15",
            "--max-time",
            "120",
            "--user-agent",
            "Rustlet/0.1",
            "--url",
            url,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    let read = child
        .stdout
        .take()
        .ok_or("Chybí odpověď serveru.")?
        .take(maximum + 1)
        .read_to_end(&mut bytes);
    if read.is_err() || bytes.len() as u64 > maximum {
        let _ = child.kill();
        let _ = child.wait();
        return Err("Stažený soubor překročil povolenou velikost nebo byl přerušen.".into());
    }
    let output = child.wait_with_output().map_err(|e| e.to_string())?;
    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr);
        return Err(if error.contains("404") {
            "Zatím není zveřejněné dostupné vydání.".into()
        } else {
            "Stažení se nepodařilo. Zkontroluj připojení a zkus to znovu.".into()
        });
    }
    Ok(bytes)
}
pub fn latest(tool: &Tool, os: &str) -> Result<Release, String> {
    let release = decode(tool, &download(&tool.manifest_url, 65536)?)?;
    compatible(&release, os, std::env::consts::ARCH)?;
    Ok(release)
}
