# Implementation Plan: Daily Wake Time Support

## Goal
Add `--daily HH:MM` flag to `cryo-agent time` command for daily recurring schedules.

## Tasks

### 1. Modify Time command struct
**File**: `src/bin/cryo_agent.rs:57-60`

Add `--daily` optional flag to `Time` command:
```rust
Time {
    /// Offset from now (e.g. "+30 minutes", "+2 hours", "+1 day")
    offset: Option<String>,
    /// Daily time (e.g. "13:00")
    #[arg(long)]
    daily: Option<String>,
},
```

### 2. Update cmd_time function
**File**: `src/bin/cryo_agent.rs:161-195`

Modify function signature and logic:
```rust
fn cmd_time(offset: Option<&str>, daily: Option<&str>) -> Result<()>
```

Add daily time calculation logic before existing offset logic:
- Parse HH:MM format
- Get today's date with specified time
- If current time >= target time, add 1 day
- Return formatted timestamp

### 3. Update main() to pass daily parameter
**File**: `src/bin/cryo_agent.rs:135`

Change:
```rust
Commands::Time { offset, daily } => cmd_time(offset.as_deref(), daily.as_deref()),
```

### 4. Add tests
**File**: `tests/cli_edge_tests.rs` or new test file

Test cases:
- `cryo-agent time --daily 13:00` before 13:00 → today 13:00
- `cryo-agent time --daily 13:00` after 13:00 → tomorrow 13:00
- Invalid format error handling

## Acceptance Criteria
- `cryo-agent time --daily 13:00` returns next occurrence of 13:00
- Existing `cryo-agent time "+13 hours"` still works
- Tests pass
