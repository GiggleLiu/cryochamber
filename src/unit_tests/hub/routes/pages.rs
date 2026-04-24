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
fn shell_uses_distinct_class_when_agent_session_is_running() {
    assert!(
        SHELL_HTML.contains("if (entry.agent_running && entry.running) return 'running-active';"),
        "the rail should surface active agent sessions with a distinct status class"
    );
    assert!(
        SHELL_HTML.contains("return entry.running ? '●' : '○';"),
        "the glyph should stay the same and let the animation carry the distinction"
    );
}

#[test]
fn shell_pulses_active_session_even_when_chamber_plan_is_complete() {
    // A chamber whose plan was previously marked complete can still be
    // running a fresh session (e.g. a follow-up wake from a TODO). The rail
    // should pulse on `running-active` instead of staying the steady
    // `complete` colour, otherwise active work is invisible to the operator.
    let active_idx = SHELL_HTML
        .find("if (entry.agent_running && entry.running) return 'running-active';")
        .expect("active session check must exist");
    let complete_idx = SHELL_HTML
        .find("if (entry.completed) return 'complete';")
        .expect("complete check must exist");
    assert!(
        active_idx < complete_idx,
        "agent_running check must precede the completed check so an active session animation overrides the steady complete colour"
    );
}

#[test]
fn shell_css_animates_running_active_status_dot() {
    assert!(
        WEB_CSS.contains(".status-dot.running-active"),
        "web CSS should define a dedicated running-active status-dot rule"
    );
    assert!(
        WEB_CSS.contains("@keyframes cryo-pulse"),
        "web CSS should define the rail pulse animation"
    );
}

#[test]
fn shell_css_disables_running_active_animation_for_reduced_motion() {
    assert!(
        WEB_CSS.contains("@media (prefers-reduced-motion: reduce)"),
        "web CSS should respect reduced-motion preferences"
    );
    assert!(
        WEB_CSS.contains(".status-dot.running-active { animation: none; }"),
        "running-active pulse should stop when reduced motion is requested"
    );
}

#[test]
fn shell_css_does_not_keep_orphan_event_log_selectors() {
    assert!(
        !WEB_CSS.contains(".event-log"),
        "event-log selectors are orphaned; current shell has no matching markup"
    );
}

#[test]
fn shell_emits_session_markers_between_messages_of_different_sessions() {
    // Operators asked to see which wake/session produced each message.
    // The server now tags every message with `session: N` and the thread
    // emits a `.session-marker` divider whenever the number changes.
    assert!(
        SHELL_HTML.contains("function buildSessionMarker"),
        "shell should build a session marker element"
    );
    assert!(
        SHELL_HTML.contains("`Session ${session}`"),
        "session marker should display the session number"
    );
    assert!(
        SHELL_HTML.contains("if (sess !== state.session)"),
        "appendMessagesInto should diff by session, not just day"
    );
    assert!(
        WEB_CSS.contains(".session-marker"),
        "session marker CSS must exist"
    );
}

#[test]
fn shell_gives_thread_min_height_so_overflow_can_scroll() {
    // Flex children default to `min-height: auto`, which forces the
    // thread to grow past the viewport instead of scrolling internally —
    // that's why clicks on the drawer or new messages failed to "jump to
    // bottom". Bound the height explicitly.
    let start = WEB_CSS.find(".thread {").expect(".thread rule missing");
    let after = &WEB_CSS[start..];
    let end = after.find('}').expect(".thread rule unterminated");
    let rule = &after[..end];
    assert!(
        rule.contains("min-height: 0"),
        "`.thread` must set min-height: 0 so overflow-y: auto actually scrolls: {rule}"
    );
}

#[test]
fn shell_preserves_notes_scroll_across_sse_refreshes() {
    // Every status SSE tick calls renderNotes. Blowing away the DOM each
    // time resets the `<pre>`'s scrollTop to 0, so a reader mid-way down
    // the notes keeps getting yanked back to the top. The update path
    // must reuse the existing node and preserve scrollTop.
    assert!(
        SHELL_HTML.contains("if (view.notesEl && box.contains(view.notesEl))"),
        "renderNotes should reuse the existing notes <pre> on update"
    );
    assert!(
        SHELL_HTML.contains("view.notesEl.scrollTop = prevScroll"),
        "renderNotes must restore scrollTop after updating text"
    );
    assert!(
        SHELL_HTML.contains("if (view.notesEl.textContent === nextContent) return"),
        "renderNotes should skip when content is unchanged"
    );
}

#[test]
fn shell_renders_notes_as_readable_prose() {
    // The notes panel used to inherit `.log` styling (11px mono, dim
    // ink, dark log background) which made prose read like a log tail.
    // Option 1: keep the raw text but style the container as a reading
    // surface — sans-serif body, ink-on-panel, generous line-height.
    let start = WEB_CSS.find(".notes {").expect(".notes rule missing");
    let after = &WEB_CSS[start..];
    let end = after.find('}').expect(".notes rule unterminated");
    let rule = &after[..end];
    assert!(
        rule.contains("font-family: var(--sans)"),
        "notes should use the sans-serif body font, not mono: {rule}"
    );
    assert!(
        rule.contains("background: var(--panel)"),
        "notes should sit on the panel surface, not the dark log bg: {rule}"
    );
    assert!(
        rule.contains("white-space: pre-wrap"),
        "notes must preserve newlines from NOTES.md: {rule}"
    );
}

#[test]
fn shell_folds_older_thread_messages_into_history_details() {
    // The message thread used to paginate older messages behind a "Load
    // older" button. Switch to a `<details>` fold so the thread's older
    // messages collapse the same way as completed chambers and done
    // TODOs — one "history" pattern across the dashboard.
    assert!(
        SHELL_HTML.contains("class = 'thread-history'") || SHELL_HTML.contains("'thread-history'"),
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
