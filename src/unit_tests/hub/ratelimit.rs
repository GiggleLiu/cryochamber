use super::*;
use std::time::{Duration, Instant};

fn deny_secs(d: &Decision) -> u64 {
    match d {
        Decision::Deny { retry_after } => retry_after.as_secs(),
        Decision::Allow => panic!("expected Deny"),
    }
}

#[test]
fn burst_then_deny_then_refill() {
    let rl = RateLimiter::new(5, 10); // 10/min = one token every 6 s
    let t0 = Instant::now();
    for i in 0..5 {
        assert!(
            matches!(rl.check_at("k", t0), Decision::Allow),
            "burst call {i}"
        );
    }
    let d = rl.check_at("k", t0);
    assert_eq!(deny_secs(&d), 6, "an empty bucket needs one refill period");

    // 5 s later: still short of one token — and short by exactly the second
    // that has not yet accrued, which pins the accrual as continuous rather
    // than a whole-period step.
    let d = rl.check_at("k", t0 + Duration::from_secs(5));
    assert_eq!(
        deny_secs(&d),
        1,
        "credit accrues every instant, not in steps"
    );
    // 6 s later: exactly one token back.
    assert!(matches!(
        rl.check_at("k", t0 + Duration::from_secs(6)),
        Decision::Allow
    ));
    assert!(matches!(
        rl.check_at("k", t0 + Duration::from_secs(6)),
        Decision::Deny { .. }
    ));
}

#[test]
fn refill_is_capped_at_capacity() {
    let rl = RateLimiter::new(5, 10);
    let t0 = Instant::now();
    for _ in 0..5 {
        rl.check_at("k", t0);
    }
    // An hour later the bucket is full again — but not fuller.
    let later = t0 + Duration::from_secs(3600);
    for _ in 0..5 {
        assert!(matches!(rl.check_at("k", later), Decision::Allow));
    }
    assert!(matches!(rl.check_at("k", later), Decision::Deny { .. }));
}

#[test]
fn keys_are_independent() {
    let rl = RateLimiter::new(1, 10);
    let t0 = Instant::now();
    assert!(matches!(rl.check_at("a", t0), Decision::Allow));
    assert!(matches!(rl.check_at("a", t0), Decision::Deny { .. }));
    assert!(matches!(rl.check_at("b", t0), Decision::Allow));
}

#[test]
fn write_key_names_owner_and_invite_and_exempts_open_mode() {
    use crate::hub::tokens::Role;
    assert_eq!(write_key(Some(&Role::Owner)).as_deref(), Some("owner"));
    let inv = Role::Invite {
        name: "Alice".into(),
        chambers: vec![],
    };
    assert_eq!(write_key(Some(&inv)).as_deref(), Some("invite:Alice"));
    assert_eq!(write_key(None), None);
}

#[tokio::test]
async fn too_many_requests_carries_retry_after_and_error_body() {
    let resp = too_many_requests(Duration::from_millis(6500));
    assert_eq!(resp.status(), axum::http::StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(
        resp.headers().get(axum::http::header::RETRY_AFTER).unwrap(),
        "7",
        "Retry-After is whole seconds, rounded up"
    );
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(v["error"], "rate limited");
}

#[test]
fn retry_after_never_tells_a_client_to_retry_immediately() {
    // A sub-second wait rounds up to 1, never to 0: `Retry-After: 0` invites a
    // client to spin, which is the behavior the limiter exists to stop.
    let resp = too_many_requests(Duration::from_millis(1));
    assert_eq!(
        resp.headers().get(axum::http::header::RETRY_AFTER).unwrap(),
        "1"
    );
    let resp = too_many_requests(Duration::ZERO);
    assert_eq!(
        resp.headers().get(axum::http::header::RETRY_AFTER).unwrap(),
        "1"
    );
}
