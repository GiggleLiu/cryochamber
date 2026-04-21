//! HTML shell + static assets.

use axum::response::Html;

const SHELL_HTML: &str = include_str!("../../../templates/web_shell.html");
const WEB_CSS: &str = include_str!("../../../templates/web.css");
const LOGO_SVG: &str = include_str!("../../../docs/logo/logo.svg");
const MARKED_JS: &str = include_str!("../../../templates/vendor/marked.min.js");
const PURIFY_JS: &str = include_str!("../../../templates/vendor/purify.min.js");

pub async fn get_index() -> Html<&'static str> {
    Html(SHELL_HTML)
}

pub async fn get_css() -> ([(&'static str, &'static str); 1], &'static str) {
    ([("content-type", "text/css")], WEB_CSS)
}

pub async fn get_logo() -> ([(&'static str, &'static str); 1], &'static str) {
    ([("content-type", "image/svg+xml; charset=utf-8")], LOGO_SVG)
}

pub async fn get_marked_js() -> ([(&'static str, &'static str); 1], &'static str) {
    (
        [("content-type", "application/javascript; charset=utf-8")],
        MARKED_JS,
    )
}

pub async fn get_purify_js() -> ([(&'static str, &'static str); 1], &'static str) {
    (
        [("content-type", "application/javascript; charset=utf-8")],
        PURIFY_JS,
    )
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
    fn shell_hides_start_button_on_completed_plan() {
        // A plan that's already flagged complete has nothing to resume — the
        // Start button would be a dead end. Reset stays available so the
        // operator can archive and begin a new plan.
        assert!(
            SHELL_HTML.contains("!entry.config_error && !entry.completed"),
            "start button should be suppressed when the chamber's plan is complete"
        );
    }

    #[test]
    fn shell_folds_completed_chambers_in_rail() {
        // Completed chambers should collapse into a `<details>` "Completed"
        // section in the sidebar, mirroring the TODO panel's History fold.
        assert!(
            SHELL_HTML.contains("class = \"chamber-history\"")
                || SHELL_HTML.contains("'chamber-history'"),
            "completed chambers should be placed inside a `.chamber-history` <details>"
        );
        assert!(
            SHELL_HTML.contains("`Completed (${completed.length})`"),
            "the fold summary should announce how many completed chambers are hidden"
        );
    }

    #[test]
    fn shell_folds_on_completion_even_while_daemon_still_running() {
        // Completion must win over the running flag in the rail split. A
        // chamber that just received `hibernate --complete` often still has
        // its daemon winding down; since `statusClass` and the header glyph
        // already prioritise `completed`, the fold must too — otherwise the
        // rail shows it as active while every other visual signal disagrees.
        assert!(
            SHELL_HTML.contains("if (c.completed) completed.push(c);"),
            "the fold split must group by completion alone, not (completed && !running)"
        );
        assert!(
            !SHELL_HTML.contains("c.completed && !c.running"),
            "the old (completed && !running) guard must be gone to keep the rail consistent"
        );
    }

    #[test]
    fn shell_opens_completed_fold_when_selection_is_inside() {
        // If the active chamber has just been marked complete, the fold would
        // hide it and the operator loses visual confirmation of what's
        // selected. The rail must auto-open the fold in that case.
        assert!(
            SHELL_HTML.contains("selectedIsInside"),
            "rail should detect when the selected chamber is inside the completed fold"
        );
        assert!(
            SHELL_HTML.contains("details.open = true"),
            "rail should force the completed fold open when the selection is inside"
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
    fn shell_renders_notes_as_sanitized_markdown() {
        // NOTES.md used to render inside `<pre class="log notes">`, same
        // styling as the raw log tail. We now parse it with `marked` and
        // sanitize with DOMPurify before injecting as HTML, so prose reads
        // like prose and agent-written `<script>` tags can't execute.
        assert!(
            SHELL_HTML.contains("/assets/marked.min.js")
                && SHELL_HTML.contains("/assets/purify.min.js"),
            "shell should load vendored marked + DOMPurify scripts"
        );
        assert!(
            SHELL_HTML.contains("window.marked.parse(raw"),
            "renderNotes should parse via marked"
        );
        assert!(
            SHELL_HTML.contains("window.DOMPurify.sanitize"),
            "renderNotes must pass parsed HTML through DOMPurify"
        );
        assert!(
            SHELL_HTML.contains("notes-rendered"),
            "notes body should use the styled `.notes-rendered` container"
        );
        assert!(
            WEB_CSS.contains(".notes-rendered") && WEB_CSS.contains(".notes-rendered h1"),
            "notes markdown styling block must exist in CSS"
        );
    }

    #[test]
    fn shell_folds_older_thread_messages_into_history_details() {
        // The message thread used to paginate older messages behind a "Load
        // older" button. Switch to a `<details>` fold so the thread's older
        // messages collapse the same way as completed chambers and done
        // TODOs — one "history" pattern across the dashboard.
        assert!(
            SHELL_HTML.contains("class = 'thread-history'")
                || SHELL_HTML.contains("'thread-history'"),
            "thread should wrap older messages in a `.thread-history` <details>"
        );
        assert!(
            SHELL_HTML.contains("`History (${historyCount} older)`"),
            "fold summary should announce how many older messages are hidden"
        );
        assert!(
            !SHELL_HTML.contains("load-older") && !SHELL_HTML.contains("loadOlder"),
            "load-older button logic should be gone now that history is a fold"
        );
        assert!(
            WEB_CSS.contains(".thread-history") && WEB_CSS.contains(".thread-history-body"),
            "thread-history CSS should mirror todo-history / chamber-history"
        );
    }

    #[test]
    fn shell_stacks_task_and_wake_time_on_separate_rail_lines() {
        // Task and wake used to share one line via `justify-content:
        // space-between`, which made the wake time read as noise glued to the
        // task text. Stack them so the wake time sits on its own line under
        // the task — easier to parse at a glance.
        let start = WEB_CSS
            .find(".chamber-meta {")
            .expect(".chamber-meta rule missing");
        let after = &WEB_CSS[start..];
        let end = after.find('}').expect(".chamber-meta rule unterminated");
        let rule = &after[..end];
        assert!(
            rule.contains("flex-direction: column"),
            "`.chamber-meta` should be a column flex: {rule}"
        );
        assert!(
            !rule.contains("space-between"),
            "`.chamber-meta` should no longer use space-between: {rule}"
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
