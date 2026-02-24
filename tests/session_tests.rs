// tests/session_tests.rs

// --- should_copy_plan ---

#[test]
fn test_should_copy_same_file() {
    let dir = tempfile::tempdir().unwrap();
    let plan = dir.path().join("plan.md");
    std::fs::write(&plan, "# Plan").unwrap();
    assert!(!cryochamber::session::should_copy_plan(&plan, &plan));
}

#[test]
fn test_should_copy_different_files() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("source.md");
    let dst = dir.path().join("plan.md");
    std::fs::write(&src, "# Source").unwrap();
    std::fs::write(&dst, "# Old").unwrap();
    assert!(cryochamber::session::should_copy_plan(&src, &dst));
}

#[test]
fn test_should_copy_dest_nonexistent() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("source.md");
    let dst = dir.path().join("plan.md");
    std::fs::write(&src, "# Source").unwrap();
    assert!(cryochamber::session::should_copy_plan(&src, &dst));
}

#[test]
fn test_should_copy_source_nonexistent() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("nonexistent.md");
    let dst = dir.path().join("plan.md");
    std::fs::write(&dst, "# Plan").unwrap();
    assert!(cryochamber::session::should_copy_plan(&src, &dst));
}
