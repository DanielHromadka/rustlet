use crate::releases::{self, Release, Tool};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

pub fn bundle_info(path: &Path) -> Result<(String, u64), String> {
    let value = plist::Value::from_file(path.join("Contents/Info.plist"))
        .map_err(|_| "Aplikace nemá platný Info.plist.")?;
    let info = value
        .as_dictionary()
        .ok_or("Neplatné informace o aplikaci.")?;
    let id = info
        .get("CFBundleIdentifier")
        .and_then(plist::Value::as_string)
        .ok_or("Chybí identifikátor aplikace.")?;
    let build = info
        .get("CFBundleVersion")
        .and_then(plist::Value::as_string)
        .ok_or("Chybí číslo sestavení.")?
        .parse()
        .map_err(|_| "Neplatné číslo sestavení.")?;
    Ok((id.to_owned(), build))
}
fn run(command: &mut Command, message: &str) -> Result<(), String> {
    let output = command.output().map_err(|e| e.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        Err(message.into())
    }
}
pub fn validate_bundle(app: &Path, tool: &Tool, release: &Release) -> Result<(), String> {
    if fs::symlink_metadata(app)
        .map_err(|e| e.to_string())?
        .file_type()
        .is_symlink()
    {
        return Err("Balíček aplikace nesmí být symbolický odkaz.".into());
    }
    let (id, build) = bundle_info(app)?;
    if id != tool.bundle_id || build != release.build {
        return Err("Stažená aplikace neodpovídá podepsané verzi.".into());
    }
    let requirement = format!(
        "anchor apple generic and identifier \"{}\" and certificate leaf[subject.OU] = \"{}\"",
        tool.bundle_id, tool.team_id
    );
    run(
        Command::new("/usr/bin/codesign")
            .args(["--verify", "--deep", "--strict", "-R", &requirement])
            .arg(app),
        "Podpis aplikace nebo identita vydavatele nesouhlasí.",
    )?;
    run(
        Command::new("/usr/sbin/spctl")
            .args(["--assess", "--type", "execute"])
            .arg(app),
        "macOS nepotvrdil notarizaci stažené aplikace.",
    )
}
pub fn allowed_target(path: &Path, tool: &Tool, home: &Path) -> bool {
    path == Path::new("/Applications").join(&tool.bundle_name)
        || path == home.join("Applications").join(&tool.bundle_name)
}
// Stage and validate before asking the native lifecycle adapter to stop the app.
// Rename on one filesystem, keeping the old app until the replacement succeeds.
pub fn replace(
    staged: &Path,
    target: &Path,
    backup: &Path,
    rename: impl Fn(&Path, &Path) -> std::io::Result<()>,
) -> Result<(), String> {
    let existed = target.exists();
    if existed {
        rename(target, backup).map_err(|e| format!("Původní aplikaci nelze připravit: {e}"))?;
    }
    if let Err(error) = rename(staged, target) {
        if existed {
            rename(backup,target).map_err(|rollback|format!("Instalace selhala: {error}. Obnovení selhalo: {rollback}. Původní aplikace je v {}.",backup.display()))?;
        }
        return Err(format!(
            "Instalace selhala, původní aplikace zůstala zachovaná: {error}"
        ));
    }
    Ok(())
}
pub fn install(
    tool: &Tool,
    release: &Release,
    target: &Path,
    stop: impl FnOnce() -> Result<bool, String>,
) -> (Result<(), String>, bool) {
    let mut relaunch = false;
    let operation = (|| {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or("Chybí domovský adresář.")?;
        if !allowed_target(target, tool, &home) {
            return Err("Cíl instalace není povolený.".into());
        }
        if target.exists() {
            if fs::symlink_metadata(target)
                .map_err(|e| e.to_string())?
                .file_type()
                .is_symlink()
            {
                return Err("Cílová aplikace je symbolický odkaz.".into());
            }
            let (id, build) = bundle_info(target)?;
            if id != tool.bundle_id {
                return Err("Cílové umístění patří jiné aplikaci.".into());
            }
            if build >= release.build {
                return Err("Tato nebo novější verze už je nainstalovaná.".into());
            }
        }
        let data = releases::download(&release.archive_url, release.archive_size)?;
        if data.len() as u64 != release.archive_size {
            return Err("Velikost staženého balíčku nesouhlasí.".into());
        }
        releases::verify(&data, &release.archive_signature, &tool.public_key)?;
        let parent = target.parent().ok_or("Neplatné umístění instalace.")?;
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        let stage = tempfile::Builder::new()
            .prefix(".rustlet-install-")
            .tempdir_in(parent)
            .map_err(|_| {
                "Do složky Applications nelze zapisovat. Použij aplikaci v ~/Applications."
                    .to_owned()
            })?;
        let zip = stage.path().join("update.zip");
        fs::write(&zip, data).map_err(|e| e.to_string())?;
        let unpacked = stage.path().join("unpacked");
        run(
            Command::new("/usr/bin/ditto")
                .args(["-x", "-k"])
                .arg(&zip)
                .arg(&unpacked),
            "Balíček nelze rozbalit.",
        )?;
        let app = unpacked.join(&tool.bundle_name);
        validate_bundle(&app, tool, release)?;
        relaunch = stop()?;
        // Re-check after stopping; a standalone updater may have replaced the tool meanwhile.
        if target.exists() {
            let (id, build) = bundle_info(target)?;
            if id != tool.bundle_id || build >= release.build {
                return Err("Aplikace se během stahování změnila. Zkontroluj verze znovu.".into());
            }
        }
        let backup = stage.path().join("previous.app");
        let result = replace(&app, target, &backup, |from, to| fs::rename(from, to));
        if result.is_err() && backup.exists() && !target.exists() {
            // Preserve recovery data even if rollback failed, rather than deleting it on Drop.
            let _retained = stage.keep();
        }
        result
    })();
    (operation, relaunch)
}
