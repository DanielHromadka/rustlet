use super::{
    bootstrap, install,
    releases::{self, Release},
};
use base64::{engine::general_purpose::STANDARD, Engine};
use ed25519_dalek::{Signer, SigningKey};
use std::{fs, io, path::Path};
fn fixture() -> (releases::Tool, Release, SigningKey) {
    let mut tool = releases::catalog().remove(0);
    let signing = SigningKey::from_bytes(&[42; 32]);
    tool.public_key = STANDARD.encode(signing.verifying_key().to_bytes());
    let release = Release {
        schema: 1,
        id: tool.id.clone(),
        bundle_id: tool.bundle_id.clone(),
        version: "0.3.0".into(),
        build: 3,
        minimum_macos: "12.0".into(),
        architectures: vec!["aarch64".into()],
        archive_url: format!("{}v0.3.0/tool.zip", tool.archive_prefix),
        archive_size: 3,
        archive_signature: STANDARD.encode(signing.sign(b"zip").to_bytes()),
    };
    (tool, release, signing)
}
fn envelope(release: &Release, key: &SigningKey) -> Vec<u8> {
    let payload = serde_json::to_vec(release).unwrap();
    serde_json::to_vec(&serde_json::json!({"payload":STANDARD.encode(&payload),"signature":STANDARD.encode(key.sign(&payload).to_bytes())})).unwrap()
}
#[test]
fn signed_metadata_and_archives_reject_tampering() {
    let (tool, release, key) = fixture();
    let valid = envelope(&release, &key);
    assert_eq!(releases::decode(&tool, &valid).unwrap().build, 3);
    let mut changed: serde_json::Value = serde_json::from_slice(&valid).unwrap();
    changed["payload"] = serde_json::Value::String(STANDARD.encode(b"untrusted metadata"));
    assert!(releases::decode(&tool, &serde_json::to_vec(&changed).unwrap()).is_err());
    let wrong = SigningKey::from_bytes(&[17; 32]);
    assert!(releases::decode(&tool, &envelope(&release, &wrong)).is_err());
    assert!(releases::verify(b"zip", &release.archive_signature, &tool.public_key).is_ok());
    assert!(releases::verify(b"ziq", &release.archive_signature, &tool.public_key).is_err());
    assert!(releases::verify(b"zip", "bad", &tool.public_key).is_err());
}
#[test]
fn trusted_signatures_do_not_bypass_tool_identity_limits_or_compatibility() {
    let (tool, release, key) = fixture();
    for mutated in ["id", "bundle", "url", "size", "build", "schema"] {
        let mut r = release.clone();
        match mutated {
            "id" => r.id = "other".into(),
            "bundle" => r.bundle_id = "other".into(),
            "url" => r.archive_url = "http://evil.invalid/app.zip".into(),
            "size" => r.archive_size = releases::MAX_ARCHIVE + 1,
            "build" => r.build = 0,
            _ => r.schema = 99,
        }
        assert!(releases::decode(&tool, &envelope(&r, &key)).is_err());
    }
    assert!(releases::compatible(&release, "26.6.2", "aarch64").is_ok());
    assert!(releases::compatible(&release, "11.7", "aarch64").is_err());
    assert!(releases::compatible(&release, "26.6", "x86_64").is_err());
    assert!(releases::compatible(&release, "invalid", "aarch64").is_err());
    assert!(releases::decode(&tool, &vec![0; 65537]).is_err());
    assert!(releases::download("file:///tmp/private", 1).is_err());
}
#[test]
fn replacement_rolls_back_and_does_not_touch_preferences() {
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("Tool.app");
    let staged = root.path().join("next.app");
    let backup = root.path().join("old.app");
    fs::create_dir(&target).unwrap();
    fs::write(target.join("version"), "old").unwrap();
    fs::create_dir(&staged).unwrap();
    fs::write(staged.join("version"), "new").unwrap();
    let preferences = root.path().join("preferences.plist");
    fs::write(&preferences, "saved profiles").unwrap();
    assert!(
        install::replace(&staged, &target, &backup, |from, to| if from == staged {
            Err(io::Error::other("disk failure"))
        } else {
            fs::rename(from, to)
        })
        .is_err()
    );
    assert_eq!(fs::read_to_string(target.join("version")).unwrap(), "old");
    install::replace(&staged, &target, &backup, |from, to| fs::rename(from, to)).unwrap();
    assert_eq!(fs::read_to_string(target.join("version")).unwrap(), "new");
    assert_eq!(fs::read_to_string(backup.join("version")).unwrap(), "old");
    assert_eq!(fs::read_to_string(preferences).unwrap(), "saved profiles");
}
#[test]
fn missing_staged_app_rolls_back_and_targets_are_restricted() {
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("Tool.app");
    fs::create_dir(&target).unwrap();
    assert!(install::replace(
        &root.path().join("missing"),
        &target,
        &root.path().join("backup"),
        |from, to| fs::rename(from, to)
    )
    .is_err());
    assert!(target.exists());
    let tool = releases::catalog().remove(0);
    let home = Path::new("/Users/test");
    assert!(install::allowed_target(
        &Path::new("/Applications").join(&tool.bundle_name),
        &tool,
        home
    ));
    assert!(install::allowed_target(
        &home.join("Applications").join(&tool.bundle_name),
        &tool,
        home
    ));
    assert!(!install::allowed_target(
        Path::new("/System/Applications/Other.app"),
        &tool,
        home
    ));
}

#[test]
fn external_apps_keep_their_own_updates() {
    let tool = releases::tool("afinder").unwrap();
    assert_eq!(tool.bundle_id, "cz.attosoft.afinder");
    assert_eq!(tool.integration, releases::Integration::External);
    // Reject external tools before any network access or release verification.
    assert_eq!(
        releases::latest(&tool, "26.0").unwrap_err(),
        "Aktualizace spravuje samotná aplikace."
    );
    let (_, release, key) = fixture();
    assert_eq!(
        releases::decode(&tool, &envelope(&release, &key)).unwrap_err(),
        "Aktualizace spravuje samotná aplikace."
    );
    let managed = releases::tool("desktop-tool-ring").unwrap();
    assert_eq!(managed.integration, releases::Integration::Rustlet);
    assert!(!managed.manifest_url.is_empty());
    assert!(!managed.public_key.is_empty());
}

fn github_fixture() -> serde_json::Value {
    use sha2::{Digest, Sha256};
    serde_json::json!({
        "tag_name":"v1.2.3", "draft":false, "prerelease":false,
        "assets":(["arm64", "x64"].map(|arch| serde_json::json!({
            "name":format!("aFinder-{arch}-mac.zip"), "size":3,
            "browser_download_url":format!("https://github.com/DanielHromadka/afinder-downloads/releases/download/v1.2.3/aFinder-{arch}-mac.zip"),
            "digest":format!("sha256:{:x}", Sha256::digest(b"zip"))
        })))
    })
}
#[test]
fn bootstrap_selects_the_native_archive_and_rejects_tampering() {
    let tool = releases::tool("afinder").unwrap();
    let metadata = github_fixture();
    for (arch, name) in [("aarch64", "arm64"), ("x86_64", "x64")] {
        let (version, asset) =
            bootstrap::select_asset(&serde_json::to_vec(&metadata).unwrap(), &tool, arch).unwrap();
        assert_eq!(version, "1.2.3");
        assert!(asset
            .browser_download_url
            .ends_with(&format!("aFinder-{name}-mac.zip")));
        assert!(bootstrap::verify_archive(b"zip", &asset).is_ok());
        assert!(bootstrap::verify_archive(b"ziq", &asset).is_err());
        assert!(bootstrap::verify_archive(b"zip-extra", &asset).is_err());
    }
    for mutation in ["url", "size", "digest", "missing", "prerelease", "draft"] {
        let mut metadata = github_fixture();
        match mutation {
            "url" => {
                metadata["assets"][0]["browser_download_url"] =
                    "https://evil.invalid/aFinder-arm64-mac.zip".into()
            }
            "size" => metadata["assets"][0]["size"] = (releases::MAX_ARCHIVE + 1).into(),
            "digest" => metadata["assets"][0]["digest"] = "sha256:bad".into(),
            "missing" => metadata["assets"][0]
                .as_object_mut()
                .unwrap()
                .remove("digest")
                .map(|_| ())
                .unwrap(),
            "prerelease" => metadata["prerelease"] = true.into(),
            _ => metadata["draft"] = true.into(),
        }
        assert!(
            bootstrap::select_asset(&serde_json::to_vec(&metadata).unwrap(), &tool, "aarch64")
                .is_err(),
            "{mutation}"
        );
    }
    assert!(
        bootstrap::select_asset(&serde_json::to_vec(&metadata).unwrap(), &tool, "riscv64").is_err()
    );
}
#[test]
fn bootstrap_never_overwrites_an_existing_installation() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source.app");
    let target = root.path().join("target.app");
    fs::create_dir(&source).unwrap();
    fs::write(source.join("version"), "new").unwrap();
    fs::create_dir(&target).unwrap();
    fs::write(target.join("version"), "existing").unwrap();
    assert!(bootstrap::ensure_absent(&target).is_err());
    assert!(bootstrap::commit(&source, &target).is_err());
    assert_eq!(
        fs::read_to_string(target.join("version")).unwrap(),
        "existing"
    );
    let dangling = root.path().join("dangling.app");
    std::os::unix::fs::symlink(root.path().join("missing"), &dangling).unwrap();
    assert!(bootstrap::ensure_absent(&dangling).is_err());
    assert!(bootstrap::commit(&source, &dangling).is_err());
    let fresh = root.path().join("fresh.app");
    bootstrap::commit(&source, &fresh).unwrap();
    assert_eq!(fs::read_to_string(fresh.join("version")).unwrap(), "new");
}

#[test]
#[ignore = "Downloads and validates the current public aFinder release with Gatekeeper"]
fn afinder_download_install_end_to_end() {
    let home = tempfile::tempdir().unwrap();
    let tool = releases::tool("afinder").unwrap();
    let target = home.path().join("Applications").join(&tool.bundle_name);
    let output = std::process::Command::new("/usr/bin/sw_vers")
        .arg("-productVersion")
        .output()
        .unwrap();
    let os = String::from_utf8(output.stdout).unwrap();
    bootstrap::install_in_home(&tool, &target, os.trim(), home.path()).unwrap();
    let info = plist::Value::from_file(target.join("Contents/Info.plist")).unwrap();
    assert_eq!(
        info.as_dictionary().unwrap()["CFBundleIdentifier"].as_string(),
        Some(tool.bundle_id.as_str())
    );
    let mut wrong_publisher = tool.clone();
    wrong_publisher.team_id = "WRONGTEAM".into();
    assert!(install::validate_publisher(&target, &wrong_publisher).is_err());
    assert!(
        bootstrap::install_in_home(&tool, &target, os.trim(), home.path())
            .unwrap_err()
            .contains("už je nainstalovaná")
    );
    assert_eq!(
        fs::read_dir(target.parent().unwrap()).unwrap().count(),
        1,
        "Staging files must be cleaned up"
    );
}
