use std::path::PathBuf;

use cryochamber::chamber_paths::{self, ChamberPaths, PROJECT_CHAMBER_DIR};

#[test]
fn project_local_paths_live_under_dot_cryo() {
    let project = PathBuf::from("existing-project");

    let paths = ChamberPaths::project_local(&project);

    assert_eq!(paths.project_dir(), project.as_path());
    assert_eq!(
        paths.chamber_dir(),
        project.join(PROJECT_CHAMBER_DIR).as_path()
    );
    assert_eq!(
        paths.plan_path(),
        project.join(PROJECT_CHAMBER_DIR).join("plan.md")
    );
    assert_eq!(
        paths.config_path(),
        project.join(PROJECT_CHAMBER_DIR).join("cryo.toml")
    );
    assert_eq!(
        paths.notes_path(),
        project.join(PROJECT_CHAMBER_DIR).join("NOTES.md")
    );
    assert_eq!(
        paths.messages_dir(),
        project.join(PROJECT_CHAMBER_DIR).join("messages")
    );
}

#[test]
fn standalone_paths_use_the_chamber_directory_directly() {
    let chamber = PathBuf::from("workspace/chambers/reminder");

    let paths = ChamberPaths::standalone(&chamber);

    assert_eq!(paths.project_dir(), chamber.as_path());
    assert_eq!(paths.chamber_dir(), chamber.as_path());
    assert_eq!(paths.plan_path(), chamber.join("plan.md"));
    assert_eq!(paths.config_path(), chamber.join("cryo.toml"));
    assert_eq!(paths.notes_path(), chamber.join("NOTES.md"));
    assert_eq!(paths.messages_dir(), chamber.join("messages"));
}

#[test]
fn free_path_helpers_are_relative_to_a_chamber_directory() {
    let chamber = PathBuf::from("chamber");

    assert_eq!(chamber_paths::plan_path(&chamber), chamber.join("plan.md"));
    assert_eq!(
        chamber_paths::config_path(&chamber),
        chamber.join("cryo.toml")
    );
    assert_eq!(
        chamber_paths::notes_path(&chamber),
        chamber.join("NOTES.md")
    );
    assert_eq!(
        chamber_paths::messages_dir(&chamber),
        chamber.join("messages")
    );
}
