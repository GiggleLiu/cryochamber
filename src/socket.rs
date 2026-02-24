use serde::{Deserialize, Serialize};

/// Request from CLI to daemon via Unix socket.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Request {
    Hibernate {
        wake: Option<String>,
        complete: bool,
        exit_code: u8,
        summary: Option<String>,
    },
    Note {
        text: String,
    },
    Alert {
        action: String,
        target: String,
        message: String,
    },
    Reply {
        text: String,
    },
}

/// Response from daemon to CLI.
#[derive(Debug, Serialize, Deserialize)]
pub struct Response {
    pub ok: bool,
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serialize_hibernate_request() {
        let req = Request::Hibernate {
            wake: Some("2026-03-08T09:00".to_string()),
            complete: false,
            exit_code: 0,
            summary: Some("Done".to_string()),
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: Request = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, Request::Hibernate { .. }));
    }

    #[test]
    fn test_serialize_note_request() {
        let req = Request::Note { text: "progress update".to_string() };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("progress update"));
    }

    #[test]
    fn test_serialize_response_ok() {
        let resp = Response { ok: true, message: "Hibernating".to_string() };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("true"));
    }

    #[test]
    fn test_serialize_alert_request() {
        let req = Request::Alert {
            action: "email".to_string(),
            target: "user@example.com".to_string(),
            message: "stuck".to_string(),
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: Request = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, Request::Alert { .. }));
    }

    #[test]
    fn test_serialize_reply_request() {
        let req = Request::Reply { text: "done with phase 1".to_string() };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("done with phase 1"));
    }
}
