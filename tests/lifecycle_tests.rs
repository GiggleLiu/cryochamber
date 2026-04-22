use cryochamber::config::{self, CryoConfig};
use cryochamber::lifecycle::{prepare_start, StartOptions};

fn init_chamber(dir: &std::path::Path) {
    config::save_config(&config::config_path(dir), &CryoConfig::default()).unwrap();
    std::fs::write(dir.join("plan.md"), "test plan").unwrap();
}

#[test]
fn prepare_start_rejects_missing_config() {
    let dir = tempfile::tempdir().unwrap();

    let err = prepare_start(dir.path(), StartOptions::default()).unwrap_err();

    assert!(err.to_string().contains("cryo.toml"));
}

#[test]
fn prepare_start_uses_cli_overrides_in_state_and_effective_agent() {
    let dir = tempfile::tempdir().unwrap();
    init_chamber(dir.path());

    let prepared = prepare_start(
        dir.path(),
        StartOptions {
            agent_override: Some("mock".to_string()),
            max_retries_override: Some(9),
            max_session_duration_override: Some(120),
        },
    )
    .unwrap();

    assert_eq!(prepared.effective_agent, "mock");
    assert_eq!(prepared.state.session_number, 0);
    assert_eq!(prepared.state.pid, None);
    assert_eq!(prepared.state.agent_override.as_deref(), Some("mock"));
    assert_eq!(prepared.state.max_retries_override, Some(9));
    assert_eq!(prepared.state.max_session_duration_override, Some(120));
}

#[test]
fn prepare_start_rejects_locked_state() {
    let dir = tempfile::tempdir().unwrap();
    init_chamber(dir.path());
    let state = cryochamber::state::CryoState {
        session_number: 7,
        pid: Some(std::process::id()),
        retry_count: 0,
        agent_override: None,
        max_retries_override: None,
        max_session_duration_override: None,
        last_report_time: None,
        provider_index: None,
        instance_id: None,
        pending_fallback: None,
        in_flight_fallback: None,
        previous_session_crashed: false,
    };
    cryochamber::state::save_state(&cryochamber::state::state_path(dir.path()), &state).unwrap();

    let err = prepare_start(dir.path(), StartOptions::default()).unwrap_err();

    assert!(err.to_string().contains("already running"));
}
