//! Compatibility gate: the disk backend must read, in place, a data directory
//! laid out by Keryx 0.5.1.

use std::path::Path;

use keryx_core::sha256_hex;
use keryx_store::{create_backend, BackendConfig, DiskConfig};

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/data-0.5.1");

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

#[tokio::test]
async fn a_data_directory_written_by_0_5_1_reads_in_place() {
    // Work on a copy: opening the backend creates `.staging` beside `drafts/`.
    let data_dir = tempfile::tempdir().unwrap();
    copy_dir(Path::new(FIXTURE), data_dir.path());

    let backend = create_backend(&BackendConfig::Disk(DiskConfig {
        data_dir: data_dir.path().to_path_buf(),
    }))
    .await
    .unwrap();

    let manifest = std::fs::read_to_string(data_dir.path().join("manifest.tsv")).unwrap();
    let mut expected_keys = Vec::new();
    for row in manifest.lines() {
        let [key, content_hash, file_size] = row.split('\t').collect::<Vec<_>>()[..] else {
            panic!("malformed manifest row: {row}");
        };
        let html = backend
            .get(key)
            .await
            .unwrap()
            .unwrap_or_else(|| panic!("{key} written by 0.5.1 did not read in place"));
        assert_eq!(sha256_hex(&html), content_hash, "{key} content changed");
        assert_eq!(html.len().to_string(), file_size, "{key} size changed");
        expected_keys.push(key.to_string());
    }
    assert_eq!(expected_keys.len(), 3);

    let mut listed: Vec<String> = backend
        .list("drafts/")
        .await
        .unwrap()
        .into_iter()
        .map(|entry| entry.key)
        .collect();
    listed.sort();
    assert_eq!(listed, expected_keys);
}
