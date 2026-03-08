---
name: memorise
description: Use at session start or when you need to recover project context without asking the user
---

# Memorise - Industrial Context Recovery

## 1. Automatic Context Recovery

When starting any task, you MUST:

1. Read `CURRENT_CONTEXT.md` for current state
2. Read all files in `docs/decisions/` for design decisions
3. Read relevant code comments for implementation details
4. Restore complete memory without requiring user input

**Never ask**: "What were we working on?" or "Can you remind me?"

## 2. Recording Key Information

When working, maintain `CURRENT_CONTEXT.md` with this structure:

```markdown
# Current Context

## Current Thing (正在做的事)
- Task: [what you're doing now]
- Status: [in progress/blocked/waiting]
- Next step: [immediate next action]

## Conclusions (已得出的结论)
- [Key finding 1]
- [Key finding 2]
- [Decision made and why]

## Open Questions
- [Unresolved issue 1]
- [Unresolved issue 2]
```

Update this file:
- **Before** starting new work
- **After** completing a task
- **When** discovering important findings

## 2. Self-Healing Mechanism

When you discover:
- Previous conclusions were wrong
- Solutions no longer work
- Rules are outdated

You MUST:

1. Automatically locate old documents/comments
2. Fix the content
3. Overwrite the original file
4. Report: location + old content + new content + reason

## 3. Process Self-Iteration

You MUST:

1. Keep this protocol in `.claude/skills/memorise/SKILL.md`
2. When you find process improvements, update this file
3. Always execute using the latest version

## 4. Mandatory Output Format

Every update MUST use this format:

```
📁 File: path/to/file
🔧 Type: New/Fix/Optimize
📝 Content: [what changed]
✅ Reason: [why changed]
```

## 5. Execution Rules

- **No redundant questions**: Context is in files, not user memory
- **Auto-fix errors**: Don't report bugs, fix them
- **Update documentation**: When code changes, update docs immediately
- **Verify changes**: Run tests after fixes

## 6. Context Files

- `CURRENT_CONTEXT.md` - Current work state
- `docs/decisions/*.md` - Design decisions
- Code comments - Implementation rationale
- This file - Execution protocol
