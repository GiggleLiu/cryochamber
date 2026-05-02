use std::path::{Path, PathBuf};

pub const PROJECT_CHAMBER_DIR: &str = ".cryo";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChamberPaths {
    project_dir: PathBuf,
    chamber_dir: PathBuf,
}

impl ChamberPaths {
    pub fn project_local(project_dir: impl AsRef<Path>) -> Self {
        let project_dir = project_dir.as_ref().to_path_buf();
        let chamber_dir = project_local_chamber_dir(&project_dir);
        Self {
            project_dir,
            chamber_dir,
        }
    }

    pub fn standalone(chamber_dir: impl AsRef<Path>) -> Self {
        let chamber_dir = chamber_dir.as_ref().to_path_buf();
        Self {
            project_dir: chamber_dir.clone(),
            chamber_dir,
        }
    }

    pub fn project_dir(&self) -> &Path {
        &self.project_dir
    }

    pub fn chamber_dir(&self) -> &Path {
        &self.chamber_dir
    }

    pub fn plan_path(&self) -> PathBuf {
        plan_path(&self.chamber_dir)
    }

    pub fn config_path(&self) -> PathBuf {
        config_path(&self.chamber_dir)
    }

    pub fn notes_path(&self) -> PathBuf {
        notes_path(&self.chamber_dir)
    }

    pub fn messages_dir(&self) -> PathBuf {
        messages_dir(&self.chamber_dir)
    }
}

pub fn project_local_chamber_dir(project_dir: impl AsRef<Path>) -> PathBuf {
    project_dir.as_ref().join(PROJECT_CHAMBER_DIR)
}

pub fn plan_path(chamber_dir: impl AsRef<Path>) -> PathBuf {
    chamber_dir.as_ref().join("plan.md")
}

pub fn config_path(chamber_dir: impl AsRef<Path>) -> PathBuf {
    chamber_dir.as_ref().join("cryo.toml")
}

pub fn notes_path(chamber_dir: impl AsRef<Path>) -> PathBuf {
    chamber_dir.as_ref().join("NOTES.md")
}

pub fn messages_dir(chamber_dir: impl AsRef<Path>) -> PathBuf {
    chamber_dir.as_ref().join("messages")
}
