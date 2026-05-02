use super::*;

fn write_chamber(dir: &std::path::Path) {
    std::fs::create_dir_all(dir).unwrap();
    let cfg = crate::config::CryoConfig::default();
    crate::config::save_config(&dir.join("cryo.toml"), &cfg).unwrap();
}

#[test]
fn record_at_deduplicates_canonical_paths() {
    let tmp = tempfile::tempdir().unwrap();
    let registry = tmp.path().join("chambers.json");
    let chamber = tmp.path().join("alpha");
    write_chamber(&chamber);

    record_at(&registry, &chamber).unwrap();
    record_at(&registry, &chamber.join(".")).unwrap();

    let entries = list_at(&registry).unwrap();
    assert_eq!(entries, vec![chamber.canonicalize().unwrap()]);
}

#[test]
fn prune_invalid_at_removes_missing_and_non_chambers() {
    let tmp = tempfile::tempdir().unwrap();
    let registry = tmp.path().join("chambers.json");
    let valid = tmp.path().join("valid");
    let plain_dir = tmp.path().join("plain");
    let missing = tmp.path().join("missing");
    write_chamber(&valid);
    std::fs::create_dir_all(&plain_dir).unwrap();

    record_at(&registry, &valid).unwrap();
    record_at(&registry, &plain_dir).unwrap();
    record_at(&registry, &missing).unwrap();

    let entries = prune_invalid_at(&registry).unwrap();

    assert_eq!(entries, vec![valid.canonicalize().unwrap()]);
    assert_eq!(list_at(&registry).unwrap(), entries);
}

#[test]
fn import_daemon_entries_at_records_valid_running_chambers() {
    let tmp = tempfile::tempdir().unwrap();
    let registry = tmp.path().join("chambers.json");
    let valid = tmp.path().join("valid");
    let plain_dir = tmp.path().join("plain");
    write_chamber(&valid);
    std::fs::create_dir_all(&plain_dir).unwrap();

    let entries = vec![
        crate::registry::DaemonEntry {
            pid: 1,
            dir: valid.to_string_lossy().into_owned(),
            socket_path: None,
        },
        crate::registry::DaemonEntry {
            pid: 2,
            dir: plain_dir.to_string_lossy().into_owned(),
            socket_path: None,
        },
    ];

    import_daemon_entries_at(&registry, &entries).unwrap();

    assert_eq!(
        list_at(&registry).unwrap(),
        vec![valid.canonicalize().unwrap()]
    );
}
