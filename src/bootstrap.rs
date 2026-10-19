//! First installation of applications that retain their own update mechanism.
use crate::{
    install,
    releases::{self, Integration, Tool},
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Debug, Deserialize)]
pub(crate) struct Asset {
    name: String,
    pub(crate) size: u64,
    pub(crate) browser_download_url: String,
    digest: String,
}
#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<Asset>,
}
pub(crate) fn select_asset(
    bytes: &[u8],
    tool: &Tool,
    arch: &str,
) -> Result<(String, Asset), String> {
    let release: GithubRelease =
        serde_json::from_slice(bytes).map_err(|_| "Neplatné informace o vydání aplikace.")?;
    let version = release
        .tag_name
        .strip_prefix('v')
        .unwrap_or(&release.tag_name);
    if release.draft || release.prerelease || version.is_empty() || version.len() > 40 {
        return Err("Není dostupné stabilní vydání aplikace.".into());
    }
    let arch = match arch {
        "aarch64" => "arm64",
        "x86_64" => "x64",
        _ => return Err("Tento procesor není podporován.".into()),
    };
    let name = format!(
        "{}-{arch}-mac.zip",
        tool.bundle_name.trim_end_matches(".app")
    );
    let asset = release
        .assets
        .into_iter()
        .find(|a| a.name == name)
        .ok_or("Pro tento procesor není dostupný instalační balíček.")?;
    let expected_url = format!(
        "https://github.com/{}/releases/download/{}/{}",
        tool.github_repository, release.tag_name, name
    );
    if asset.browser_download_url != expected_url
        || asset.size == 0
        || asset.size > releases::MAX_ARCHIVE
        || !valid_digest(&asset.digest)
    {
        return Err("Instalační balíček nemá důvěryhodný původ nebo kontrolní součet.".into());
    }
    Ok((version.to_owned(), asset))
}
fn valid_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hash| hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()))
}
pub(crate) fn verify_archive(data: &[u8], asset: &Asset) -> Result<(), String> {
    let digest = format!("sha256:{:x}", Sha256::digest(data));
    if data.len() as u64 != asset.size || !digest.eq_ignore_ascii_case(&asset.digest) {
        return Err("Kontrolní součet nebo velikost staženého balíčku nesouhlasí.".into());
    }
    Ok(())
}
fn validate_application(app: &Path, tool: &Tool, version: &str, os: &str) -> Result<(), String> {
    if fs::symlink_metadata(app)
        .map_err(|e| e.to_string())?
        .file_type()
        .is_symlink()
    {
        return Err("Balíček aplikace nesmí být symbolický odkaz.".into());
    }
    let value = plist::Value::from_file(app.join("Contents/Info.plist"))
        .map_err(|_| "Aplikace nemá platný Info.plist.")?;
    let info = value
        .as_dictionary()
        .ok_or("Neplatné informace o aplikaci.")?;
    let string = |key: &str| info.get(key).and_then(plist::Value::as_string);
    if string("CFBundleIdentifier") != Some(tool.bundle_id.as_str())
        || string("CFBundleShortVersionString") != Some(version)
    {
        return Err("Stažená aplikace neodpovídá vybranému vydání.".into());
    }
    releases::require_macos(
        string("LSMinimumSystemVersion").ok_or("Chybí minimální verze macOS.")?,
        os,
    )?;
    install::validate_publisher(app, tool)
}
pub fn install(tool: &Tool, target: &Path, os: &str) -> Result<(), String> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or("Chybí domovský adresář.")?;
    install_in_home(tool, target, os, &home)
}
pub(crate) fn install_in_home(
    tool: &Tool,
    target: &Path,
    os: &str,
    home: &Path,
) -> Result<(), String> {
    if tool.integration != Integration::External
        || tool.github_repository.is_empty()
        || !install::allowed_target(target, tool, home)
    {
        return Err("Instalace této aplikace nebo cílové umístění nejsou povolené.".into());
    }
    // This path never replaces an existing installation (including App Store builds).
    ensure_absent(target)?;
    let metadata = releases::download(
        &format!(
            "https://api.github.com/repos/{}/releases/latest",
            tool.github_repository
        ),
        1024 * 1024,
    )?;
    let (version, asset) = select_asset(&metadata, tool, std::env::consts::ARCH)?;
    let data = releases::download(&asset.browser_download_url, asset.size)?;
    verify_archive(&data, &asset)?;
    let parent = target.parent().ok_or("Neplatné umístění instalace.")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let stage = tempfile::Builder::new()
        .prefix(".rustlet-install-")
        .tempdir_in(parent)
        .map_err(|_| "Do cílové složky Applications nelze zapisovat.")?;
    let zip = stage.path().join("download.zip");
    fs::write(&zip, data).map_err(|e| e.to_string())?;
    let unpacked = stage.path().join("unpacked");
    install::run(
        Command::new("/usr/bin/ditto")
            .args(["-x", "-k"])
            .arg(&zip)
            .arg(&unpacked),
        "Balíček nelze rozbalit.",
    )?;
    let app = unpacked.join(&tool.bundle_name);
    validate_application(&app, tool, &version, os)?;
    // RENAME_EXCL makes the final commit atomic and refuses an app installed meanwhile.
    commit(&app, target)
}
pub(crate) fn ensure_absent(target: &Path) -> Result<(), String> {
    match fs::symlink_metadata(target) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Ok(_) => Err("Aplikace už je nainstalovaná. Aktualizace prováděj přímo v ní.".into()),
        Err(e) => Err(e.to_string()),
    }
}
pub(crate) fn commit(app: &Path, target: &Path) -> Result<(), String> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};
    extern "C" {
        fn renamex_np(
            from: *const std::ffi::c_char,
            to: *const std::ffi::c_char,
            flags: u32,
        ) -> i32;
    }
    let from = CString::new(app.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
    let to = CString::new(target.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
    if unsafe {
        renamex_np(from.as_ptr(), to.as_ptr(), 0x4 /* RENAME_EXCL */)
    } != 0
    {
        return Err(format!(
            "Aplikaci nelze nainstalovat: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}
