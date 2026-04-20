//! HTML shell + static assets.

use axum::response::Html;

const SHELL_HTML: &str = include_str!("../../../templates/web_shell.html");
const WEB_CSS: &str = include_str!("../../../templates/web.css");
const LOGO_SVG: &str = include_str!("../../../docs/logo/logo.svg");

pub async fn get_index() -> Html<&'static str> {
    Html(SHELL_HTML)
}

pub async fn get_css() -> ([(&'static str, &'static str); 1], &'static str) {
    ([("content-type", "text/css")], WEB_CSS)
}

pub async fn get_logo() -> ([(&'static str, &'static str); 1], &'static str) {
    ([("content-type", "image/svg+xml; charset=utf-8")], LOGO_SVG)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_wires_search_and_history_interactions() {
        assert!(
            SHELL_HTML.contains("railSearch.addEventListener('input'"),
            "rail search input should filter the chamber list"
        );
        assert!(
            SHELL_HTML.contains("window.addEventListener('popstate'"),
            "browser back/forward should restore the selected chamber"
        );
        assert!(
            SHELL_HTML.contains("selectChamber(urlId, { push: false })"),
            "deep-link initialization should not push duplicate history entries"
        );
    }

    #[test]
    fn shell_guards_async_detail_and_application_errors() {
        assert!(
            SHELL_HTML.contains("detailSeq"),
            "detail loads need a sequence guard for stale async responses"
        );
        assert!(
            SHELL_HTML.contains("if (seq !== state.detailSeq || state.currentId !== id) return"),
            "stale detail fetches should not overwrite the current pane"
        );
        assert!(
            SHELL_HTML.contains("if (data && data.ok === false)"),
            "fetchJSON should surface JSON application errors, not just HTTP errors"
        );
    }

    #[test]
    fn shell_disables_lifecycle_buttons_while_request_is_pending() {
        assert!(
            SHELL_HTML.contains("lifecyclePending"),
            "shell should track pending lifecycle actions per chamber"
        );
        assert!(
            SHELL_HTML.contains("setLifecyclePending"),
            "lifecycle requests should update pending state before and after fetch"
        );
        assert!(
            SHELL_HTML.contains("b.disabled = !!pending"),
            "rendered lifecycle buttons should be disabled while a lifecycle request is pending"
        );
    }

    #[test]
    fn shell_does_not_render_restart_button() {
        assert!(
            !SHELL_HTML.contains("btn('restart'"),
            "restart remains a backend operation but should not be exposed as a hub button"
        );
    }

    #[test]
    fn shell_does_not_render_wake_button() {
        assert!(
            !SHELL_HTML.contains("btn('wake'"),
            "wake remains a backend operation but should not be exposed as a hub button"
        );
    }

    #[test]
    fn shell_renders_cryochamber_messages_as_system_notices() {
        assert!(
            SHELL_HTML.contains("m.from === 'cryochamber'"),
            "messages from cryochamber should render as centered system notices, not agent bubbles"
        );
        assert!(
            !SHELL_HTML.contains("row.textContent = `·"),
            "system notices should not start with a decorative dot"
        );
        assert!(
            WEB_CSS.contains(".sys-notice") && WEB_CSS.contains("align-self: center"),
            "system notices should be centered gray text like timestamp separators"
        );
    }

    #[test]
    fn shell_refreshes_rail_after_log_events() {
        assert!(
            SHELL_HTML.contains("function scheduleRailRefresh"),
            "log events should have a debounced rail refresh path"
        );
        assert!(
            SHELL_HTML.contains("scheduleRailRefresh(d.chamber_id)"),
            "log SSE events should refresh the rail because completion can arrive as log output"
        );
    }

    #[test]
    fn shell_only_renders_reset_for_stopped_chambers() {
        assert!(
            SHELL_HTML.contains("!entry.running && !entry.config_error"),
            "reset should only be exposed for stopped, config-valid chambers"
        );
    }

    #[test]
    fn shell_prioritizes_completed_status_over_running() {
        assert!(
            SHELL_HTML.contains("if (entry.completed) return 'complete';"),
            "the rail dot should show completion when a chamber is complete, even if shutdown state is briefly mixed"
        );
        assert!(
            SHELL_HTML.contains("if (entry.completed) return '✓';"),
            "the header status glyph should show completion when a chamber is complete"
        );
    }

    #[test]
    fn shell_places_sync_controls_in_right_drawer() {
        assert!(
            SHELL_HTML.contains("data-tab=\"sync\""),
            "sync controls should render as a right-drawer tab"
        );
        assert!(
            SHELL_HTML.contains("id=\"panel-sync\""),
            "sync controls should render into a right-drawer panel"
        );
        assert!(
            !SHELL_HTML.contains("function buildSyncStrip"),
            "sync controls should not have a center-column strip"
        );
        assert!(
            SHELL_HTML.contains("view.syncEl = document.getElementById('panel-sync');"),
            "sync controls should render through the right-drawer panel"
        );
    }
}
