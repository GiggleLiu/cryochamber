use super::*;

#[test]
fn shell_omits_rail_search_and_keeps_history_interactions() {
    assert!(
        !SHELL_HTML.contains("id=\"rail-search\""),
        "rail search box should not render"
    );
    assert!(
        !SHELL_HTML.contains("railSearch.addEventListener('input'")
            && !SHELL_HTML.contains("document.getElementById('rail-search')"),
        "rail search wiring should be removed"
    );
    assert!(
        !WEB_CSS.contains(".rail-search"),
        "rail search CSS should be removed with the control"
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
        "there is no wake operation; the hub must not render a wake button"
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
fn shell_renders_daily_digests_in_log_tab() {
    assert!(
        SHELL_HTML.contains("status.daily_digests"),
        "Log tab should render daily digests from chamber status"
    );
    assert!(
        SHELL_HTML.contains("daily-digest"),
        "Log tab should have digest rows separate from the raw log tail"
    );
    assert!(
        WEB_CSS.contains(".digest-list") && WEB_CSS.contains(".daily-digest"),
        "daily digest rows should have dedicated styling"
    );
}

#[test]
fn shell_daily_digest_rows_omit_latest_session_label() {
    assert!(
        !SHELL_HTML.contains("latest #"),
        "daily digest rows should not render a latest-session label"
    );
    assert!(
        !SHELL_HTML.contains("digest-latest"),
        "daily digest rows should not reserve a third latest-session column"
    );
    assert!(
        WEB_CSS.contains("grid-template-columns: max-content minmax(0, 1fr);"),
        "daily digest rows should reserve columns for date and counts only"
    );
}

#[test]
fn shell_periodically_refreshes_chamber_registry() {
    assert!(
        SHELL_HTML.contains("async function refreshChamberIndex"),
        "shell should have a registry refresh path separate from runtime-only chamber loading"
    );
    assert!(
        SHELL_HTML.contains("fetchJSON('/api/chambers/refresh', { method: 'POST' })"),
        "registry refresh should call the discovery endpoint"
    );
    assert!(
        SHELL_HTML
            .contains("setInterval(() => { refreshChamberIndex({ silent: true }); }, 15_000);"),
        "shell should periodically re-read registry so externally started chambers appear"
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
fn shell_renders_archive_for_stopped_chambers_and_unarchive_when_archived() {
    // #59: stopped chambers can be archived (folded out of the active grid);
    // archived chambers expose only Unarchive.
    assert!(
        SHELL_HTML.contains("btn(t('lc.archive')"),
        "stopped chambers should expose an Archive button"
    );
    assert!(
        SHELL_HTML.contains("btn(t('lc.unarchive')"),
        "archived chambers should expose an Unarchive button"
    );
    assert!(
        SHELL_HTML.contains("t('rail.archived', { n: archived.length })"),
        "archived chambers should fold into an Archived section in the rail"
    );
}

#[test]
fn shell_new_chamber_modal_collects_provider_and_api_key() {
    assert!(
        SHELL_HTML.contains("id=\"modal-provider-details\""),
        "new chamber modal should keep API key provider config in a folded section"
    );
    assert!(
        SHELL_HTML.contains("id=\"modal-provider-input\""),
        "new chamber modal should render a provider combobox input"
    );
    assert!(
        SHELL_HTML.contains("list=\"modal-provider-options\""),
        "provider input should use a datalist so selecting an option fills the input"
    );
    assert!(
        SHELL_HTML.contains("id=\"modal-provider-options\""),
        "new chamber modal should render provider datalist options"
    );
    assert!(
        SHELL_HTML.contains("id=\"modal-model-input\""),
        "new chamber modal should render a model combobox input"
    );
    assert!(
        SHELL_HTML.contains("list=\"modal-model-options\""),
        "model input should use a datalist so selecting an option fills the input"
    );
    assert!(
        SHELL_HTML.contains("id=\"modal-model-options\""),
        "new chamber modal should render model datalist options"
    );
    assert!(
        !SHELL_HTML.contains("id=\"modal-provider-select\""),
        "provider dropdown should not be a separate control from custom provider input"
    );
    assert!(
        !SHELL_HTML.contains("id=\"modal-model-select\""),
        "model dropdown should not be a separate control from custom model input"
    );
    assert!(
        SHELL_HTML.contains("id=\"modal-api-key-input\""),
        "new chamber modal should request an API key"
    );
    assert!(
        SHELL_HTML.contains("api_key_provider"),
        "new chamber request should include the selected API key provider"
    );
    assert!(
        SHELL_HTML.contains("api_key"),
        "new chamber request should include the API key"
    );
    assert!(
        SHELL_HTML.contains("model"),
        "new chamber request should include the selected or custom model"
    );
    assert!(
        SHELL_HTML.contains("https://models.dev/api.json"),
        "new chamber modal should fetch the curl-friendly models.dev API"
    );
    assert!(
        SHELL_HTML.contains("providersFromModelsDev"),
        "new chamber modal should derive provider/model options from the browser-fetched response"
    );
    assert!(
        !SHELL_HTML.contains("POPULAR_MODEL_CATALOG"),
        "provider/model dropdown should not be backed by a hardcoded catalog"
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
        SHELL_HTML.contains("t('rail.completed', { n: completed.length })"),
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
fn shell_renders_new_chamber_button_and_modal() {
    assert!(
        SHELL_HTML.contains("id=\"chamber-new\""),
        "rail head should expose a new-chamber button"
    );
    assert!(
        SHELL_HTML.contains("id=\"modal-backdrop\""),
        "shell should include the new-chamber modal backdrop"
    );
    assert!(
        SHELL_HTML.contains("id=\"modal-create\""),
        "shell should include a create action inside the modal"
    );
}

#[test]
fn shell_wires_new_chamber_modal_to_api_and_selection() {
    assert!(
        SHELL_HTML.contains("fetch('/api/chambers/new'"),
        "modal create action should POST to the new chamber API"
    );
    assert!(
        SHELL_HTML.contains("await loadChambers();"),
        "successful chamber creation should refresh the rail from the server"
    );
    assert!(
        SHELL_HTML.contains("await selectChamber(body.id"),
        "successful chamber creation should select the new chamber"
    );
    assert!(
        SHELL_HTML.contains("showErr(t('modal.err.name_empty'))"),
        "empty chamber names should be rejected inline before hitting the network"
    );
}

#[test]
fn shell_css_styles_new_chamber_modal() {
    assert!(
        WEB_CSS.contains(".rail-add"),
        "web CSS should style the rail add button"
    );
    assert!(
        WEB_CSS.contains(".modal-backdrop"),
        "web CSS should style the modal backdrop"
    );
    assert!(
        WEB_CSS.contains(".modal-btn-primary"),
        "web CSS should style the modal primary action"
    );
    assert!(
        WEB_CSS.contains(".modal-provider-details"),
        "web CSS should style the folded provider section"
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
        SHELL_HTML.contains("t('marker.session', { n: session })"),
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
fn shell_preserves_rendered_markdown_scroll_across_sse_refreshes() {
    // Every status SSE tick re-renders Plan and Notes tab bodies. Both go
    // through `setRenderedMarkdown`, which must skip when content is
    // unchanged and save + restore the scroll container's scrollTop when
    // it does write. Otherwise a reader mid-way down the document gets
    // yanked back to the top on every refresh.
    assert!(
        SHELL_HTML.contains("function setRenderedMarkdown"),
        "Plan/Notes rendering should funnel through setRenderedMarkdown"
    );
    assert!(
        SHELL_HTML
            .contains("if (view[lastKey] === html && !el.classList.contains('empty')) return"),
        "setRenderedMarkdown should skip rewrite when html unchanged"
    );
    assert!(
        SHELL_HTML.contains("prev = scroller.scrollTop;"),
        "setRenderedMarkdown should snapshot the scroller's scrollTop"
    );
    assert!(
        SHELL_HTML.contains("scroller.scrollTop = prev;"),
        "setRenderedMarkdown must restore scrollTop after writing"
    );
    assert!(
        SHELL_HTML.contains("if (stickToBottom && wasAtBottom)"),
        "setRenderedMarkdown should follow the tail when caller asks for stickToBottom"
    );
    assert!(
        SHELL_HTML.contains("scroller.scrollTop = scroller.scrollHeight;"),
        "stickToBottom branch must scroll to the new bottom after writing"
    );
}

#[test]
fn shell_opens_notes_at_bottom_by_default() {
    // Agent's Notes is appended to over time; opening at the top would
    // bury the latest entry beneath an SSE-refreshed wall of history.
    // renderNotes must call setRenderedMarkdown with stickToBottom set
    // so the first render lands at the bottom and subsequent updates
    // follow the tail when the operator is already there.
    assert!(
        SHELL_HTML.contains("function renderNotes")
            && SHELL_HTML[SHELL_HTML.find("function renderNotes").unwrap()..]
                .lines()
                .take(12)
                .any(|l| l.contains("stickToBottom: true")),
        "renderNotes must opt into the stickToBottom behaviour of setRenderedMarkdown"
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
        SHELL_HTML.contains("t('thread.history', { n: historyCount })"),
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
fn shell_rail_omits_preview_and_task_lines() {
    // The left rail should stay compact: title/status on the first row,
    // wake time underneath. Session previews and `Task:` copy belong in
    // the detail pane, not in every rail item.
    assert!(
        !SHELL_HTML.contains("prev.className = 'chamber-preview'"),
        "rail rows should not render last-message preview lines"
    );
    assert!(
        !SHELL_HTML.contains("`Task: ${c.task}`"),
        "rail rows should not render task lines"
    );
    assert!(
        SHELL_HTML.contains("applyWake(when, c.next_wake_display);"),
        "rail rows should still render the wake time line"
    );
}

#[test]
fn shell_mutes_rail_wake_time_for_stopped_chambers() {
    assert!(
        SHELL_HTML.contains("when.className = c.running ? 'when' : 'when muted';"),
        "rail wake time should be muted when the chamber is stopped"
    );
    assert!(
        WEB_CSS.contains(".chamber-meta .when.muted") && WEB_CSS.contains("var(--ink-faint)"),
        "muted rail wake CSS should use subdued text color"
    );
}

#[test]
fn shell_uses_session_summary_in_header_with_hover_title() {
    assert!(
        SHELL_HTML.contains("status.session_summary"),
        "detail header should prefer session summary over legacy task"
    );
    assert!(
        SHELL_HTML.contains("view.headerTaskLine.title = displaySummary"),
        "truncated session summary should expose full text on hover"
    );
    assert!(
        WEB_CSS.contains("text-overflow: ellipsis"),
        "header summary should be visually truncatable"
    );
}

#[test]
fn shell_renders_session_summary_without_repeated_labels() {
    assert!(
        SHELL_HTML.contains("function formatSessionSummary(session, summary)"),
        "header should normalize session summaries before display"
    );
    assert!(
        SHELL_HTML.contains("formatSessionSummary(status.session, summary)"),
        "header should prefix summaries once as `Session N: ...`"
    );
    assert!(
        SHELL_HTML.contains("view.headerMeta.style.display = displaySummary ? 'none' : '';"),
        "standalone session meta should be hidden when summary already carries session context"
    );
    assert!(
        !SHELL_HTML.contains("Last session"),
        "header should not repeat the `Last session` label before a session-prefixed summary"
    );
}

#[test]
fn shell_wake_chip_uses_relative_first_copy() {
    assert!(
        SHELL_HTML.contains("applyWake(txt, status.next_wake, t('wake.next_prefix'));"),
        "detail wake chip should read like `next wake in 23h 11m`, not `next wake · ...`"
    );
    assert!(
        !SHELL_HTML.contains("'next wake · '"),
        "wake prefix should not use a separator before relative time"
    );
}

#[test]
fn shell_hides_detail_wake_chip_when_chamber_is_stopped() {
    assert!(
        SHELL_HTML.contains("if (entry.running && status.next_wake)"),
        "detail wake chip should only show for running chambers"
    );
}

#[test]
fn shell_detail_header_truncates_title_and_gives_summary_room() {
    assert!(
        SHELL_HTML.contains("name.className = 'name';"),
        "header chamber name should have a dedicated selector for truncation"
    );
    let head_start = WEB_CSS.find(".thread-head {").expect("thread-head rule");
    let head = &WEB_CSS[head_start..WEB_CSS[head_start..].find('}').unwrap() + head_start];
    assert!(
        head.contains("display: grid") && head.contains("grid-template-areas"),
        "thread header should use grid so the summary can span the header: {head}"
    );
    let name_start = WEB_CSS
        .find(".thread-head .title .name {")
        .expect("title name rule");
    let name_rule = &WEB_CSS[name_start..WEB_CSS[name_start..].find('}').unwrap() + name_start];
    assert!(
        name_rule.contains("white-space: nowrap") && name_rule.contains("text-overflow: ellipsis"),
        "chamber title should stay on one truncated line: {name_rule}"
    );
    let task_start = WEB_CSS.find(".thread-head .task {").expect("task rule");
    let task_rule = &WEB_CSS[task_start..WEB_CSS[task_start..].find('}').unwrap() + task_start];
    assert!(
        task_rule.contains("width: 100%") && task_rule.contains("max-width: none"),
        "session summary should use the available header width: {task_rule}"
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

#[test]
fn shell_supports_chinese_language_toggle() {
    // A top-bar toggle switches the dashboard between English and Simplified
    // Chinese. The choice persists in localStorage and is honoured on load.
    assert!(
        SHELL_HTML.contains("id=\"lang-btn\""),
        "top bar should render a language toggle button"
    );
    assert!(
        WEB_CSS.contains(".lang-btn"),
        "language toggle button should be styled"
    );
    assert!(
        SHELL_HTML.contains("localStorage.setItem('hub:lang'"),
        "switching language should persist the choice in localStorage"
    );
    assert!(
        SHELL_HTML.contains("localStorage.getItem('hub:lang')"),
        "the persisted language should be read back on load"
    );
    assert!(
        SHELL_HTML.contains("window.location.reload();"),
        "language switch should reload so every string re-renders"
    );
    // Both dictionaries must be present, with zh keyed distinctly from en.
    assert!(
        SHELL_HTML.contains("en: {") && SHELL_HTML.contains("zh: {"),
        "i18n dictionary should define both English and Chinese tables"
    );
    // The navigator-language default path must fall back to Chinese for a
    // zh* browser and English otherwise.
    assert!(
        SHELL_HTML.contains("startsWith('zh') ? 'zh' : 'en'"),
        "first-visit language should follow the browser locale"
    );
    // Static chrome is translated declaratively, not only via JS calls.
    assert!(
        SHELL_HTML.contains("data-i18n=\"rail.title\""),
        "static labels should carry data-i18n attributes for translation"
    );
    assert!(
        SHELL_HTML.contains("applyStaticI18n()"),
        "static translations should be applied on load"
    );
}
