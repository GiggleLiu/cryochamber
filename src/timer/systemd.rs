// src/timer/systemd.rs
use super::{run_checked, CryoTimer, TimerId, TimerStatus};
use crate::fallback::FallbackAction;
use anyhow::Result;
use chrono::NaiveDateTime;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::process::Command;

pub struct SystemdTimer;

impl Default for SystemdTimer {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemdTimer {
    pub fn new() -> Self {
        Self
    }

    pub fn make_unit_name(work_dir: &str) -> String {
        let mut hasher = DefaultHasher::new();
        work_dir.hash(&mut hasher);
        let hash = hasher.finish();
        format!("cryochamber-{:x}", hash)
    }

    fn user_unit_dir() -> PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        PathBuf::from(home).join(".config/systemd/user")
    }

    pub fn generate_timer_unit(&self, name: &str, time: &NaiveDateTime) -> String {
        format!(
            r#"[Unit]
Description=Cryochamber wake timer: {name}

[Timer]
OnCalendar={time}
RemainAfterElapse=false
Persistent=true

[Install]
WantedBy=timers.target
"#,
            time = time.format("%Y-%m-%d %H:%M:%S")
        )
    }

    pub fn generate_service_unit(&self, name: &str, command: &str, work_dir: &str) -> String {
        format!(
            r#"[Unit]
Description=Cryochamber task: {name}

[Service]
Type=oneshot
WorkingDirectory={work_dir}
ExecStart={command}
"#
        )
    }

    fn reload_daemon() -> Result<()> {
        run_checked(
            Command::new("systemctl").args(["--user", "daemon-reload"]),
            "Failed to reload systemd daemon",
        )?;
        Ok(())
    }
}

impl SystemdTimer {
    fn schedule_job(
        &self,
        suffix: &str,
        time: NaiveDateTime,
        command: &str,
        work_dir: &str,
    ) -> Result<TimerId> {
        let name = Self::make_unit_name(work_dir);
        let job_name = format!("{name}-{suffix}");
        let unit_dir = Self::user_unit_dir();
        std::fs::create_dir_all(&unit_dir)?;

        let timer_path = unit_dir.join(format!("{job_name}.timer"));
        let service_path = unit_dir.join(format!("{job_name}.service"));

        std::fs::write(&timer_path, self.generate_timer_unit(&job_name, &time))?;
        std::fs::write(
            &service_path,
            self.generate_service_unit(&job_name, command, work_dir),
        )?;

        Self::reload_daemon()?;

        run_checked(
            Command::new("systemctl").args([
                "--user",
                "enable",
                "--now",
                &format!("{job_name}.timer"),
            ]),
            &format!("Failed to enable {suffix} timer"),
        )?;

        Ok(TimerId(job_name))
    }
}

impl CryoTimer for SystemdTimer {
    fn schedule_wake(&self, time: NaiveDateTime, command: &str, work_dir: &str) -> Result<TimerId> {
        self.schedule_job("wake", time, command, work_dir)
    }

    fn schedule_fallback(
        &self,
        time: NaiveDateTime,
        action: &FallbackAction,
        work_dir: &str,
    ) -> Result<TimerId> {
        let command = format!(
            "cryochamber fallback-exec {} {} \"{}\"",
            action.action, action.target, action.message
        );
        self.schedule_job("fallback", time, &command, work_dir)
    }

    fn cancel(&self, id: &TimerId) -> Result<()> {
        let _ = Command::new("systemctl")
            .args(["--user", "stop", &format!("{}.timer", id.0)])
            .output();
        let _ = Command::new("systemctl")
            .args(["--user", "disable", &format!("{}.timer", id.0)])
            .output();

        let unit_dir = Self::user_unit_dir();
        let _ = std::fs::remove_file(unit_dir.join(format!("{}.timer", id.0)));
        let _ = std::fs::remove_file(unit_dir.join(format!("{}.service", id.0)));

        Self::reload_daemon()?;
        Ok(())
    }

    fn verify(&self, id: &TimerId) -> Result<TimerStatus> {
        let output = Command::new("systemctl")
            .args(["--user", "is-active", &format!("{}.timer", id.0)])
            .output()?;

        if output.status.success() {
            Ok(TimerStatus::Scheduled {
                next_fire: NaiveDateTime::default(),
            })
        } else {
            Ok(TimerStatus::NotFound)
        }
    }
}
