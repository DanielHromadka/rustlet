use super::{
    install,
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
