//! Regression for issue #19: the Rust logical export must survive a Git
//! checkout byte-for-byte under `core.autocrlf=true` (Windows default).
//!
//! Uses disposable temp paths only. Requires `git` on PATH (present in CI
//! and dev environments); fails loudly otherwise so a missing tool cannot
//! masquerade as a passing proof.

use std::path::{Path, PathBuf};
use std::process::Command;

use chrono::Utc;
use lighting_core::{Memory, MemoryKind, MemoryRepository, NewMemory};
use lighting_store_surreal::{
    StoreConfig, SurrealLedgerRepository, SurrealMemoryPathRepository, SurrealMemoryRepository,
    SurrealSourceRepository, SurrealStore,
};
use uuid::Uuid;

const EXPECTED_GITATTRIBUTES: &str =
    "# Generated Lantern evidence: disable text/EOL conversion in this tree.\n* -text\n";

fn test_path(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "lantern-keeper-{label}-{}",
        Uuid::new_v4().simple()
    ))
}

fn embedded_config(path: PathBuf, database: &str) -> StoreConfig {
    let mut config = StoreConfig::from_env();
    config.storage = "embedded-surrealkv".to_owned();
    config.path = path;
    config.namespace = "export_gitattributes".to_owned();
    config.database = database.to_owned();
    config.username.clear();
    config.password.clear();
    config
}

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-c")
        .arg("core.autocrlf=true")
        .arg("-c")
        .arg("user.email=export-test")
        .arg("-c")
        .arg("user.name=export-test")
        .args(args)
        .current_dir(dir)
        .status()
        .unwrap_or_else(|error| panic!("git not runnable on PATH: {error}"));
    assert!(status.success(), "git {args:?} failed in {}", dir.display());
}

fn file_bytes(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
}

#[tokio::test]
async fn export_checkout_preserves_exact_bytes() -> Result<(), Box<dyn std::error::Error>> {
    let source_path = test_path("gitattr-source");
    let export_path = test_path("gitattr-export");
    let clone_parent = test_path("gitattr-clone-parent");
    let control_parent = test_path("gitattr-control");
    let restore_path = test_path("gitattr-restore");
    std::fs::create_dir_all(&clone_parent)?;
    std::fs::create_dir_all(&control_parent)?;

    let source_store =
        SurrealStore::connect(&embedded_config(source_path.clone(), "source")).await?;
    source_store.initialise_schema().await?;
    SurrealSourceRepository::new(source_store.clone())
        .migrate()
        .await?;
    SurrealMemoryPathRepository::new(source_store.clone())
        .migrate()
        .await?;
    SurrealMemoryRepository::new(source_store.clone())
        .migrate()
        .await?;
    SurrealLedgerRepository::new(source_store.clone())
        .migrate()
        .await?;
    SurrealMemoryRepository::new(source_store.clone())
        .store(Memory::new(NewMemory {
            content: "byte-integrity proof memory".to_owned(),
            kind: MemoryKind::Fact,
            project_id: None,
            confidence: 0.9,
            importance: 0.8,
            recorded_at: Utc::now(),
            known_at: Utc::now(),
            observed_at: None,
            valid_from: Utc::now(),
            valid_until: None,
            derived_from: vec!["source:byte-proof".to_owned()],
            updates: Vec::new(),
            extends: Vec::new(),
            supersedes: Vec::new(),
            contradicts: Vec::new(),
            supports: Vec::new(),
            agent: "export-test".to_owned(),
        })?)
        .await?;
    let original = source_store.export_to(&export_path).await?;
    assert_eq!(original.record_count, 1);

    // The export carries its own exact-byte policy as transport metadata.
    let policy = std::fs::read_to_string(export_path.join(".gitattributes"))?;
    assert_eq!(policy, EXPECTED_GITATTRIBUTES);

    // Control file OUTSIDE the export dir: must show normal autocrlf
    // conversion on checkout, proving the policy (not the machine) protects
    // the export.
    let control_path = control_parent.join("control.txt");
    std::fs::write(&control_path, "line one\nline two\n")?;

    // Commit the export as-is, then clone fresh with autocrlf forced on.
    git(&export_path, &["init", "-q"]);
    git(&export_path, &["add", "-A"]);
    git(&export_path, &["commit", "-qm", "export snapshot"]);
    let clone_path = clone_parent.join("clone");
    let status = Command::new("git")
        .arg("-c")
        .arg("core.autocrlf=true")
        .arg("clone")
        .arg("-q")
        .arg(&export_path)
        .arg(&clone_path)
        .status()
        .expect("git clone not runnable");
    assert!(status.success(), "git clone of export failed");

    // Byte-for-byte equality for every exported file, manifest included.
    // .gitattributes itself is transport metadata: compare it too.
    let mut compared = 0;
    for entry in std::fs::read_dir(&export_path)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let name = entry.file_name();
        let before = file_bytes(&entry.path());
        let after = file_bytes(&clone_path.join(&name));
        assert_eq!(
            before,
            after,
            "checkout changed bytes of {}",
            name.to_string_lossy()
        );
        compared += 1;
    }
    assert!(
        compared >= 7,
        "expected manifest + 5 ndjson + .gitattributes, saw {compared}"
    );

    // Control proves conversion is otherwise active.
    git(&control_parent, &["init", "-q"]);
    git(&control_parent, &["add", "control.txt"]);
    git(&control_parent, &["commit", "-qm", "control"]);
    let control_clone = test_path("gitattr-control-clone");
    let status = Command::new("git")
        .arg("-c")
        .arg("core.autocrlf=true")
        .arg("clone")
        .arg("-q")
        .arg(&control_parent)
        .arg(&control_clone)
        .status()
        .expect("git clone of control not runnable");
    assert!(status.success(), "git clone of control failed");
    let control_bytes = file_bytes(&control_clone.join("control.txt"));
    assert!(
        control_bytes.windows(2).any(|w| w == b"\r\n"),
        "control file shows no CRLF conversion; autocrlf simulation inactive, proof vacuous"
    );

    // Restore from the CHECKOUT (not the original) and confirm semantics.
    let restore_store =
        SurrealStore::connect(&embedded_config(restore_path.clone(), "restored")).await?;
    restore_store.initialise_schema().await?;
    SurrealSourceRepository::new(restore_store.clone())
        .migrate()
        .await?;
    SurrealMemoryPathRepository::new(restore_store.clone())
        .migrate()
        .await?;
    SurrealMemoryRepository::new(restore_store.clone())
        .migrate()
        .await?;
    SurrealLedgerRepository::new(restore_store.clone())
        .migrate()
        .await?;
    restore_store.restore_from(&clone_path).await?;
    let restored = restore_store
        .export_to(test_path("gitattr-reexport"))
        .await?;
    assert_eq!(restored.record_count, original.record_count);
    assert_eq!(restored.export_hash, original.export_hash);

    let _ = std::fs::remove_dir_all(source_path);
    let _ = std::fs::remove_dir_all(export_path);
    let _ = std::fs::remove_dir_all(clone_parent);
    let _ = std::fs::remove_dir_all(control_parent);
    let _ = std::fs::remove_dir_all(control_clone);
    let _ = std::fs::remove_dir_all(restore_path);
    let _ = std::fs::remove_dir_all(restored.directory);
    Ok(())
}
