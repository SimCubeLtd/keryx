use std::net::TcpListener;
use std::path::Path;
use std::process::{Child, Command};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;
use tempfile::TempDir;

struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn run_git(repo: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn reserve_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn wait_until_ready(base_url: &str) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if reqwest::blocking::get(format!("{base_url}/healthz"))
            .is_ok_and(|response| response.status().is_success())
        {
            return;
        }
        assert!(Instant::now() < deadline, "Keryx test server did not start");
        thread::sleep(Duration::from_millis(25));
    }
}

#[test]
fn upload_captures_the_invocation_checkout_when_html_is_elsewhere() {
    let temp = TempDir::new().unwrap();
    let repo = temp.path().join("workspace");
    let client_home = temp.path().join("home");
    let data_dir = temp.path().join("data");
    let db_path = temp.path().join("keryx.db");
    let html_path = temp.path().join("draft.html");

    std::fs::create_dir_all(&repo).unwrap();
    std::fs::create_dir_all(&client_home).unwrap();
    std::fs::write(repo.join("README.md"), "fixture\n").unwrap();
    std::fs::write(
        &html_path,
        "<!doctype html><title>Provenance fixture</title><main>fixture</main>",
    )
    .unwrap();

    run_git(&repo, &["init", "--quiet"]);
    run_git(&repo, &["config", "user.email", "test@example.com"]);
    run_git(&repo, &["config", "user.name", "Keryx Test"]);
    run_git(&repo, &["add", "README.md"]);
    run_git(&repo, &["commit", "--quiet", "-m", "fixture"]);
    run_git(&repo, &["checkout", "--quiet", "-b", "feature/provenance"]);
    run_git(
        &repo,
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/acme/widgets.git",
        ],
    );

    let port = reserve_port();
    let base_url = format!("http://127.0.0.1:{port}");
    let server = Command::new(env!("CARGO_BIN_EXE_keryx"))
        .args([
            "serve",
            "--port",
            &port.to_string(),
            "--db",
            db_path.to_str().unwrap(),
            "--data-dir",
            data_dir.to_str().unwrap(),
        ])
        .spawn()
        .unwrap();
    let _server = Server(server);
    wait_until_ready(&base_url);

    let upload = Command::new(env!("CARGO_BIN_EXE_keryx"))
        .args([
            "upload",
            html_path.to_str().unwrap(),
            "--new",
            "--api-url",
            &base_url,
        ])
        .current_dir(&repo)
        .env("HOME", &client_home)
        .output()
        .unwrap();
    assert!(
        upload.status.success(),
        "upload failed: {}",
        String::from_utf8_lossy(&upload.stderr)
    );

    let client = reqwest::blocking::Client::new();
    let list: Value = client
        .get(format!("{base_url}/api/drafts"))
        .send()
        .unwrap()
        .json()
        .unwrap();
    let draft = &list["drafts"][0];
    assert_eq!(draft["repoOrg"], "acme");
    assert_eq!(draft["repoName"], "widgets");
    assert_eq!(draft["repoHost"], "github.com");
    assert_eq!(draft["latestGitBranch"], "feature/provenance");

    let detail: Value = client
        .get(format!(
            "{base_url}/api/drafts/{}",
            draft["draftId"].as_str().unwrap()
        ))
        .send()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(
        detail["draft"]["versions"][0]["gitBranch"],
        "feature/provenance"
    );
    assert_eq!(detail["draft"]["versions"][0]["repoOrg"], "acme");
    assert_eq!(detail["draft"]["versions"][0]["repoName"], "widgets");
}
