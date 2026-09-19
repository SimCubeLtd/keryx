//! Parity level 3: the real binary, end to end, on a database and data
//! directory written by the released Keryx 0.5.1. One copy is left exactly as
//! 0.5.1 wrote it and one is adopted first; upload, serve, list and publish
//! must behave identically on both, and serving must be byte-identical to
//! what 0.5.1 stored.

use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;
use tempfile::TempDir;

const DB_0_5_1: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/crates/keryx-db/tests/fixtures/keryx-0.5.1.db"
);
const DATA_0_5_1: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/crates/keryx-store/tests/fixtures/data-0.5.1"
);
/// The draft 0.5.1 uploaded twice.
const DRAFT: &str = "g3q1hbw3lw0c";
const NEW_VERSION: &str = "<!doctype html><title>Plan one</title><p>third version</p>";

struct Server {
    child: Child,
    base_url: String,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The built binary, cut off from the developer's own config.toml: tests must
/// not change with whatever is in ~/.config/keryx on the machine running them.
fn keryx_binary() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_keryx"));
    command
        .env("XDG_CONFIG_HOME", "/nonexistent/keryx-tests")
        .env("HOME", "/nonexistent/keryx-tests")
        .env_remove("KERYX_CONFIG");
    command
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// A private copy of what 0.5.1 wrote.
fn installation(root: &Path, name: &str) -> (PathBuf, PathBuf) {
    let dir = root.join(name);
    copy_dir(Path::new(DATA_0_5_1), &dir);
    let db = dir.join("keryx.db");
    std::fs::copy(DB_0_5_1, &db).unwrap();
    (db, dir)
}

fn serve(db: &Path, data_dir: &Path) -> Server {
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let child = keryx_binary()
        .args(["serve", "--port", &port.to_string()])
        .args(["--db", db.to_str().unwrap()])
        .args(["--data-dir", data_dir.to_str().unwrap()])
        .spawn()
        .unwrap();
    let base_url = format!("http://127.0.0.1:{port}");
    let deadline = Instant::now() + Duration::from_secs(5);
    while !reqwest::blocking::get(format!("{base_url}/healthz"))
        .is_ok_and(|r| r.status().is_success())
    {
        assert!(Instant::now() < deadline, "Keryx test server did not start");
        thread::sleep(Duration::from_millis(25));
    }
    Server { child, base_url }
}

/// Everything observable about one installation: upload, serve, list, publish.
fn exercise(root: &Path, db: &Path, data_dir: &Path) -> Value {
    let server = serve(db, data_dir);
    let base = &server.base_url;
    let get = |path: &str| reqwest::blocking::get(format!("{base}{path}")).unwrap();

    // Serve what 0.5.1 stored, byte for byte.
    let manifest = std::fs::read_to_string(data_dir.join("manifest.tsv")).unwrap();
    let stored: Vec<Vec<u8>> = manifest
        .lines()
        .map(|row| std::fs::read(data_dir.join(row.split('\t').next().unwrap())).unwrap())
        .collect();
    let v1 = get(&format!("/d/{DRAFT}/v/1/raw"))
        .bytes()
        .unwrap()
        .to_vec();
    let v2 = get(&format!("/d/{DRAFT}/v/2/raw"))
        .bytes()
        .unwrap()
        .to_vec();
    assert!(stored.contains(&v1) && stored.contains(&v2) && v1 != v2);
    assert_eq!(
        get(&format!("/d/{DRAFT}/raw")).bytes().unwrap().to_vec(),
        v2
    );

    // Upload a third version onto the legacy draft.
    let html_path = root.join("v3.html");
    std::fs::write(&html_path, NEW_VERSION).unwrap();
    let home = root.join("home");
    std::fs::create_dir_all(&home).unwrap();
    let upload = keryx_binary()
        .args([
            "upload",
            html_path.to_str().unwrap(),
            "--draft",
            DRAFT,
            "--api-url",
            base,
        ])
        .current_dir(root)
        .env("HOME", &home)
        .output()
        .unwrap();
    assert!(
        upload.status.success(),
        "upload failed: {}",
        String::from_utf8_lossy(&upload.stderr)
    );
    assert_eq!(
        get(&format!("/d/{DRAFT}/raw")).bytes().unwrap().as_ref(),
        NEW_VERSION.as_bytes()
    );

    // Publish a legacy version as a PDF.
    let pdf = get(&format!("/api/drafts/{DRAFT}/pdf?version=1"));
    assert!(
        pdf.status().is_success(),
        "publish failed: {}",
        pdf.status()
    );
    assert!(pdf.bytes().unwrap().starts_with(b"%PDF"));

    // List. Ids and times of the new upload differ per run, so keep what is
    // comparable across two installations.
    let listing: Value = get("/api/drafts").json().unwrap();
    let detail: Value = get(&format!("/api/drafts/{DRAFT}")).json().unwrap();
    let drafts: Vec<Value> = listing["drafts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| serde_json::json!([d["draftId"], d["title"], d["versionCount"], d["createdAt"]]))
        .collect();
    let versions: Vec<Value> = detail["draft"]["versions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| serde_json::json!([v["versionNumber"], v["fileSize"], v["originalFilename"]]))
        .collect();
    serde_json::json!({ "drafts": drafts, "versions": versions, "v1": v1, "v2": v2 })
}

#[test]
fn an_adopted_0_5_1_installation_behaves_exactly_like_an_untouched_one() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let temp = TempDir::new().unwrap();

    let (untouched_db, untouched_dir) = installation(temp.path(), "untouched");
    let (adopted_db, adopted_dir) = installation(temp.path(), "adopted");

    let adoption = tokio::runtime::Runtime::new().unwrap().block_on(async {
        let (db, adoption) = keryx_db::adopt::open_sqlite(&adopted_db, true)
            .await
            .unwrap();
        db.close().await.unwrap();
        adoption
    });
    assert!(
        matches!(
            adoption,
            keryx_db::adopt::Adoption::Adopted {
                from_user_version: 2,
                ..
            }
        ),
        "{adoption:?}"
    );

    let untouched = exercise(
        &temp.path().join("untouched"),
        &untouched_db,
        &untouched_dir,
    );
    let adopted = exercise(&temp.path().join("adopted"), &adopted_db, &adopted_dir);
    assert_eq!(adopted, untouched);
    assert_eq!(adopted["drafts"].as_array().unwrap().len(), 2);
    assert_eq!(adopted["versions"].as_array().unwrap().len(), 3);
}
