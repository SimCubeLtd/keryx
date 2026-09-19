use std::path::Path;

use keryx_config::{load, load_file, set_client_api_url, StorageKind};

fn write(dir: &Path, text: &str) -> std::path::PathBuf {
    let path = dir.join("config.toml");
    std::fs::write(&path, text).unwrap();
    path
}

#[test]
fn a_full_file_loads_and_maps_onto_the_flags_it_stands_in_for() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(
        dir.path(),
        r#"
        [client]
        api_url = "http://plans.internal:7812"
        share_to = "ghcr.io/acme/plans"

        [server]
        host = "0.0.0.0"
        port = 9000
        data_dir = "/var/lib/keryx"
        max_html_bytes = 10485760
        allow_font_links = true

        [database]
        url = "postgres://keryx@db/keryx"
        pool_size = 8

        [storage]
        kind = "s3"
        [storage.s3]
        bucket = "keryx-plans"
        prefix = "prod"
        "#,
    );
    let config = load_file(&path).unwrap();
    assert_eq!(config.storage.kind, Some(StorageKind::S3));
    assert_eq!(
        config.client.share_to.as_deref(),
        Some("ghcr.io/acme/plans")
    );

    assert_eq!(
        config.serve_arg_defaults(),
        [
            ("host", "0.0.0.0".to_string()),
            ("port", "9000".to_string()),
            ("max_html_bytes", "10485760".to_string()),
            ("allow_font_links", "true".to_string()),
        ]
    );
    assert_eq!(
        config.database_arg_defaults(),
        [
            ("database_url", "postgres://keryx@db/keryx".to_string()),
            ("db_pool_size", "8".to_string()),
        ]
    );
    assert_eq!(
        config.s3_arg_defaults(),
        [
            ("s3_bucket", "keryx-plans".to_string()),
            ("s3_prefix", "prod".to_string())
        ]
    );
    assert_eq!(
        config.storage_kind_arg_default(),
        [("storage", "s3".to_string())]
    );
    assert_eq!(
        config.data_dir_arg_default(),
        [("data_dir", "/var/lib/keryx".to_string())]
    );
    assert_eq!(
        config.share_arg_defaults(),
        [("to", "ghcr.io/acme/plans".to_string())]
    );
}

#[test]
fn an_empty_or_missing_file_sets_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let config = load_file(&write(dir.path(), "")).unwrap();
    assert!(config.serve_arg_defaults().is_empty());
    assert!(config.database_arg_defaults().is_empty());

    // An explicit path must exist; that is a typo, not an absent config.
    let error = load(Some(&dir.path().join("nope.toml"))).unwrap_err();
    assert!(error.to_string().contains("does not exist"), "{error:#}");
}

#[test]
fn invalid_entries_are_errors_that_name_the_file_and_the_problem() {
    let dir = tempfile::tempdir().unwrap();
    for (text, expected) in [
        ("[server]\nprot = 9000\n", "prot"),
        ("[sever]\nport = 9000\n", "sever"),
        ("[server]\nport = \"nine thousand\"\n", "port"),
        ("[server]\nport = 70000\n", "port"),
        ("[storage]\nkind = \"azure\"\n", "azure"),
        ("[storage.s3]\naccess_key = \"AKIA\"\n", "access_key"),
        ("[server\nport = 1\n", "config.toml"),
    ] {
        let path = write(dir.path(), text);
        let error = format!("{:#}", load_file(&path).unwrap_err());
        assert!(error.contains("invalid config file"), "{text:?}: {error}");
        assert!(error.contains(path.to_str().unwrap()), "{text:?}: {error}");
        assert!(
            error.contains(expected),
            "{text:?} should mention {expected:?}: {error}"
        );
    }
}

#[test]
fn a_leading_tilde_in_a_path_is_the_home_directory() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(
        dir.path(),
        "[server]\ndata_dir = \"~/keryx-data\"\n[database]\npath = \"~/keryx-data/keryx.db\"\n",
    );
    let config = load_file(&path).unwrap();
    let home = dirs::home_dir().unwrap();
    assert_eq!(config.server.data_dir, Some(home.join("keryx-data")));
    assert_eq!(config.database.path, Some(home.join("keryx-data/keryx.db")));
}

#[test]
fn setting_the_api_url_keeps_everything_else_in_the_file_including_comments() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(
        dir.path(),
        "# my keryx config\n[server]\nport = 9000 # behind the proxy\n\n[client]\n# old server\napi_url = \"http://old:7812\"\nshare_to = \"ghcr.io/acme/plans\"\n",
    );
    set_client_api_url(Some(&path), "http://new:7812").unwrap();

    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("# my keryx config"));
    assert!(text.contains("port = 9000 # behind the proxy"));
    assert!(text.contains("# old server"));
    assert!(text.contains("share_to = \"ghcr.io/acme/plans\""));
    assert!(!text.contains("http://old:7812"));
    let config = load_file(&path).unwrap();
    assert_eq!(config.client.api_url.as_deref(), Some("http://new:7812"));
    assert_eq!(config.server.port, Some(9000));
}

#[test]
fn setting_the_api_url_creates_a_private_file_when_there_is_none() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nested/keryx/config.toml");
    set_client_api_url(Some(&path), "http://new:7812").unwrap();

    assert_eq!(
        load_file(&path).unwrap().client.api_url.as_deref(),
        Some("http://new:7812")
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "a new config file may come to hold secrets");
    }
}
