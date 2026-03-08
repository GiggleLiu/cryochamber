---
name: review-implementation
description: Use after implementing a feature or any code change to verify completeness and correctness before committing
---

# Review Implementation

Dispatches a quality review subagent with fresh context (no implementation history bias):
- **Quality reviewer** -- DRY, KISS, HC/LC, HCI, test quality

## Invocation

- `/review-implementation` -- auto-detect from git diff
- `/review-implementation generic` -- code quality only (explicit)

## Step 1: Detect What Changed

Determine what files were added or modified:

```bash
# Check for NEW files (not just modifications)
git diff --name-only --diff-filter=A HEAD~1..HEAD
# Also check against main for branch-level changes
git diff --name-only --diff-filter=A main..HEAD
```

Extract the changed file paths and a summary of what changed.

## Step 2: Prepare Subagent Context

Get the git SHAs for the review range:

```bash
BASE_SHA=$(git merge-base main HEAD)  # or HEAD~N for batch reviews
HEAD_SHA=$(git rev-parse HEAD)
```

Get the diff summary and changed file list:

```bash
git diff --stat $BASE_SHA..$HEAD_SHA
git diff --name-only $BASE_SHA..$HEAD_SHA
```

## Step 3: Dispatch Quality Reviewer

Dispatch using `Task` tool with `subagent_type="superpowers:code-reviewer"`:

- Read `quality-reviewer-prompt.md` from this skill directory
- Fill placeholders:
  - `{DIFF_SUMMARY}` -> output of `git diff --stat`
  - `{CHANGED_FILES}` -> list of changed files
  - `{PLAN_STEP}` -> description of what was implemented (or "standalone review")
  - `{BASE_SHA}`, `{HEAD_SHA}` -> git range
- Prompt = filled template

## Step 4: Collect and Address Findings

When the subagent returns:

1. **Parse results** -- identify ISSUE items from the report
2. **Fix automatically** -- clear issues, Important+ quality issues
3. **Report to user** -- ambiguous issues, Minor quality items, anything you're unsure about
4. **Present consolidated report**

## Step 5: Present Consolidated Report

```
## Review: [Feature/Fix/Generic] [Name]

### Build Status
- `make test`: PASS / FAIL
- `make clippy`: PASS / FAIL

### Code Quality (from quality reviewer)
- DRY: OK / ...
- KISS: OK / ...
- HC/LC: OK / ...

### HCI (if CLI changed)
...

### Test Coverage
- Coverage: [X.X%] -- OK (>=95%) / CRITICAL (<95%)

### Test Quality
...

### Fixes Applied
- [list of issues automatically fixed by main agent]

### Remaining Items (needs user decision)
- [list of issues that need user input]
```

## Integration

### Copilot Review (after PR creation)

After creating a PR (from any flow), run `make copilot-review` to request GitHub Copilot code review on the PR.

### Standalone

Invoke directly via `/review-implementation` for any code change.
