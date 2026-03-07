---
name: auto-user-test
description: Automatically test user flows end-to-end, detect issues, and fix them autonomously until the flow works for a real user
trigger: Use when you need to verify user experience, test complete workflows, or ensure features work from user perspective
---

# Auto User Test

Systematically test user flows by actually executing them, detect friction points and bugs, then fix them autonomously.

## Phase 1: Scope Selection

Ask user to choose test scope:

**Options:**
1. Full onboarding flow (new user → first success)
2. Core feature workflow (specific feature end-to-end)
3. Installation & setup only
4. Messaging channel integration (GitHub/Zulip)
5. Service lifecycle (install → run → uninstall)
6. Custom scope (user specifies)

For options 2-6, present relevant sub-options based on the codebase.

## Phase 2: Flow Execution

Execute the chosen flow AS A REAL USER WOULD:

1. **Start fresh**: Use temporary directories, clean state
2. **Follow actual steps**: Run real commands, don't simulate
3. **Capture everything**: stdout, stderr, file changes, errors
4. **Note friction**: confusing messages, missing docs, unclear errors
5. **Stop on blockers**: bugs that prevent continuation

### Execution Rules

- Use `Bash` tool for all user commands
- Read actual output, don't assume success
- Check file contents after operations
- Verify services actually start
- Test error cases (missing deps, wrong input)

## Phase 3: Issue Detection

For each issue found, categorize:

- **Blocker**: prevents flow completion
- **Friction**: works but confusing/tedious
- **Polish**: minor UX improvement

## Phase 4: Autonomous Fix

For each issue:

1. **Diagnose root cause**: read relevant code
2. **Design minimal fix**: smallest change that resolves it
3. **Implement fix**: edit code/docs
4. **Verify fix**: re-run the affected step
5. **Continue flow**: proceed to next step

### Fix Priority

1. Blockers first (flow must complete)
2. Major friction (user would struggle)
3. Polish (time permitting)

## Phase 5: Verification Loop

After all fixes:

1. **Clean slate**: fresh temp directory
2. **Re-run entire flow**: start to finish
3. **If new issues found**: return to Phase 4
4. **If flow completes**: proceed to Phase 6

## Phase 6: User Action Guide

Provide:

1. **Summary**: what was tested, issues found/fixed
2. **Remaining issues**: anything not auto-fixable
3. **Next steps**: what user should do (review changes, test manually, commit)
4. **Changed files**: list with brief explanation

## Implementation Notes

### For Full Onboarding Flow

```bash
# Create temp test directory
cd $(mktemp -d)

# Follow README steps exactly
cargo install --path /path/to/cryochamber
cryo init
# ... continue through first successful session
```

### For Core Feature Workflow

Example: Testing `cryo send` → agent receives → `cryo-agent reply`

```bash
# Setup
cryo init
cryo start

# Test flow
echo "test message" > messages/inbox/test.txt
cryo wake
# Wait and verify agent processed it
cryo log | grep "test message"
```

### For Service Lifecycle

```bash
# Install
cryo start
# Verify running
cryo status
# Verify survives reboot simulation
cryo restart
cryo status
# Clean uninstall
cryo cancel
# Verify fully removed
cryo status  # should show not running
```

## Anti-Patterns

❌ Don't simulate: "The user would run X and it would work"
✅ Actually run: Execute X and verify output

❌ Don't assume: "This should work"
✅ Verify: Check actual behavior

❌ Don't skip errors: "This error is probably fine"
✅ Investigate: Understand and fix every error

❌ Don't batch fixes: Fix everything then test
✅ Incremental: Fix → verify → continue

## Success Criteria

- [ ] Entire flow completes without errors
- [ ] No confusing messages or unclear steps
- [ ] Documentation matches actual behavior
- [ ] A new user could follow the flow independently
- [ ] All changes committed with clear messages

## Permission Strategy

This skill requires autonomous execution. When invoked:

1. User grants blanket approval for:
   - Reading any project files
   - Running test commands in temp directories
   - Editing code/docs to fix issues
   - Creating commits for fixes

2. User will review final changes before merge

3. No mid-flow permission requests (breaks automation)
