# Daily Wake Time Design

## Problem

The current `cryo-agent time` command only supports relative offsets ("+13 hours"), which calculates from the current time. This doesn't support daily recurring schedules like "wake at 13:00 every day".

Example: If agent hibernates at 23:42 and uses `cryo-agent time "+13 hours"`, it wakes at 12:42 the next day, not 13:00.

## Solution

Add `--daily HH:MM` flag to `cryo-agent time` command to calculate the next occurrence of a specific time.

### Behavior

```bash
cryo-agent time --daily 13:00
```

- If current time is before 13:00 today → return today 13:00
- If current time is 13:00 or after → return tomorrow 13:00

### Implementation

Modify `cmd_time()` in `src/bin/cryo_agent.rs`:

1. Add `--daily` flag to `Time` command struct
2. Parse `HH:MM` format
3. Calculate next occurrence using chrono
4. Return in `%Y-%m-%dT%H:%M` format (same as existing)

### Agent Usage

In protocol/plan, agent calls:
```bash
cryo-agent time --daily 13:00
```

Output: `2026-03-07T13:00`

Then uses this in TODO:
```bash
cryo-agent todo add "Daily task" --at "2026-03-07T13:00"
```

## Trade-offs

- Simple implementation (single function change)
- Agent-friendly API
- Reuses existing TODO/wake infrastructure
- No changes to daemon or TODO format
