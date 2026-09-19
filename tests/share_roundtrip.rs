//! Round trip through a real registry: `keryx share`, then a plain `oras pull`
//! by someone with no Keryx at all, then `keryx inspect` and `keryx pull`.
//!
//! Ignored by default: it needs `oras` on PATH and a registry. Run it with
//!
//!   docker run -d --rm -p 127.0.0.1:45000:5000 registry:3.1.1
//!   KERYX_TEST_REGISTRY=127.0.0.1:45000 cargo test --test share_roundtrip -- --ignored
//!
//! Before a release, run it once against a registry that issues bearer tokens
//! (GHCR, or zot behind a token server), so oci-client's token expiry path is
//! exercised without a jsonwebtoken crypto backend.
#![cfg(feature = "share")]

use std::net::TcpListener;
use std::path::Path;
use std::process::{Child, Command, Output};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;
use tempfile::TempDir;

const HTML: &str = "<!doctype html><title>Q3 Migration Plan</title><main>caf\u{e9} \u{2713}</main>";

struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
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

fn keryx(home: &Path, args: &[&str]) -> Output {
    keryx_binary()
        .args(args)
        .env("HOME", home)
        .output()
        .unwrap()
}

fn stdout_of(output: Output, what: &str) -> String {
    assert!(
        output.status.success(),
        "{what} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn field(stdout: &str, label: &str) -> String {
    stdout
        .lines()
        .find_map(|line| line.strip_prefix(label))
        .unwrap_or_else(|| panic!("no {label:?} in:\n{stdout}"))
        .trim()
        .to_string()
}

#[test]
#[ignore = "needs oras on PATH and KERYX_TEST_REGISTRY=<host:port> of a plain-HTTP registry"]
fn a_shared_draft_is_usable_with_plain_oras_and_pulls_back_into_keryx() {
    let registry = std::env::var("KERYX_TEST_REGISTRY").expect("KERYX_TEST_REGISTRY");
    let _ = rustls::crypto::ring::default_provider().install_default();
    let temp = TempDir::new().unwrap();
    let home = temp.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let html_path = temp.path().join("plan.html");
    std::fs::write(&html_path, HTML).unwrap();

    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let base_url = format!("http://127.0.0.1:{port}");
    let _server = Server(
        keryx_binary()
            .args(["serve", "--port", &port.to_string()])
            .args(["--db", temp.path().join("keryx.db").to_str().unwrap()])
            .args(["--data-dir", temp.path().join("data").to_str().unwrap()])
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    while !reqwest::blocking::get(format!("{base_url}/healthz"))
        .is_ok_and(|r| r.status().is_success())
    {
        assert!(Instant::now() < deadline, "Keryx test server did not start");
        thread::sleep(Duration::from_millis(25));
    }

    let uploaded = stdout_of(
        keryx(
            &home,
            &[
                "upload",
                html_path.to_str().unwrap(),
                "--new",
                "--api-url",
                &base_url,
            ],
        ),
        "upload",
    );
    let draft_id = field(&uploaded, "Draft ID:");

    // Share pushes exactly <base>/<draft-id>:v1.
    let base = format!("{registry}/plans");
    let share = [
        "share",
        &draft_id,
        "--to",
        &base,
        "--plain-http",
        "--api-url",
        &base_url,
    ];
    let shared = stdout_of(keryx(&home, &share), "share");
    let reference = field(&shared, "Reference:");
    let digest = field(&shared, "Digest:");
    assert_eq!(reference, format!("{base}/{draft_id}:v1"));
    assert!(shared.starts_with("Shared draft"));

    // Sharing the same version again is a no-op at the same digest.
    let again = stdout_of(keryx(&home, &share), "share again");
    assert!(again.starts_with("Already shared"));
    assert_eq!(field(&again, "Digest:"), digest);

    // The interop test: no Keryx, just oras. It names the file from the title.
    let oras_dir = temp.path().join("oras");
    std::fs::create_dir_all(&oras_dir).unwrap();
    let oras = Command::new("oras")
        .args(["pull", "--plain-http", &reference])
        .current_dir(&oras_dir)
        .output()
        .unwrap();
    assert!(
        oras.status.success(),
        "oras pull failed: {}",
        String::from_utf8_lossy(&oras.stderr)
    );
    assert_eq!(
        std::fs::read(oras_dir.join("q3-migration-plan-v1.html")).unwrap(),
        HTML.as_bytes(),
        "oras must write the exact stored bytes under the slugged title"
    );

    // Inspect reads metadata without the document.
    let inspected = stdout_of(
        keryx(&home, &["inspect", &reference, "--plain-http"]),
        "inspect",
    );
    assert_eq!(field(&inspected, "Digest:"), digest);
    assert_eq!(field(&inspected, "Title:"), "Q3 Migration Plan");
    assert_eq!(field(&inspected, "Draft ID:"), draft_id);

    // Moving tags are refused before any network call.
    let latest = keryx(
        &home,
        &["pull", &format!("{base}/{draft_id}:latest"), "--plain-http"],
    );
    assert!(!latest.status.success());
    assert!(String::from_utf8_lossy(&latest.stderr).contains("explicit version"));

    // Pull to a file touches no server; pull by digest works too.
    let out_path = temp.path().join("pulled.html");
    let by_digest = format!("{base}/{draft_id}@{digest}");
    stdout_of(
        keryx(
            &home,
            &[
                "pull",
                &by_digest,
                "--plain-http",
                "--output",
                out_path.to_str().unwrap(),
            ],
        ),
        "pull --output",
    );
    assert_eq!(std::fs::read(&out_path).unwrap(), HTML.as_bytes());

    // Pull into Keryx as a new draft, recording tag plus digest as provenance.
    let pulled = stdout_of(
        keryx(
            &home,
            &["pull", &reference, "--plain-http", "--api-url", &base_url],
        ),
        "pull",
    );
    let pulled_id = field(&pulled, "Draft ID:");
    assert_ne!(pulled_id, draft_id);
    let detail: Value = reqwest::blocking::get(format!("{base_url}/api/drafts/{pulled_id}"))
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(
        detail["draft"]["versions"][0]["originalFilename"],
        format!("{reference}@{digest}")
    );
    let raw = reqwest::blocking::get(format!("{base_url}/d/{pulled_id}/raw")).unwrap();
    assert_eq!(raw.bytes().unwrap().as_ref(), HTML.as_bytes());
}
