---
name: writing-skills
description: Use when creating new skills, editing existing skills, or verifying skills work before deployment
---

# Writing Skills - Industrial Process

## Critical Discovery

**Skills MUST be in `.claude/skills/` directory, NOT `skills/` directory.**

Claude Code only recognizes skills in:
- `.claude/skills/` (project-level)
- `~/.claude/skills/` (user-level)

## Skill Creation Checklist

### 1. Location
- [ ] Create in `.claude/skills/{skill-name}/SKILL.md`
- [ ] NOT in `skills/` or other locations

### 2. Frontmatter (Required)
```yaml
---
name: skill-name
description: Use when [triggering condition]
---
```

Rules:
- `name`: lowercase, hyphens only (no spaces/special chars)
- `description`: Start with "Use when..." (max 1024 chars total)

### 3. Content Structure
```markdown
# Skill Title

## Phase 1: [First Step]
Clear, actionable instructions

## Phase 2: [Second Step]
More instructions

## Rules
- Bullet points
- Clear constraints
```

### 4. Verification
- [ ] Restart Claude Code or reload skills
- [ ] Check skill appears in available skills list
- [ ] Test with `/skill-name` command

## Industrial Process (Like Brainstorming)

### Phase 1: Identify Need
Before creating a skill, ask:
- Is this technique non-obvious?
- Will I reference this across projects?
- Does it apply broadly (not project-specific)?

If NO to any → Don't create skill, use CLAUDE.md instead

### Phase 2: Draft Structure
1. Create `.claude/skills/{name}/SKILL.md`
2. Add frontmatter with name + description
3. Write phases/sections with clear steps
4. Add rules and constraints

### Phase 3: Test
1. Reload skills (restart Claude Code)
2. Verify skill appears in list
3. Test with `/skill-name`
4. Check if instructions are clear

### Phase 4: Iterate
When you discover:
- Instructions unclear → Update and clarify
- Missing edge cases → Add rules
- Better structure → Refactor

**Auto-update this file when you find improvements**

## Common Mistakes

❌ Wrong location: `skills/` instead of `.claude/skills/`
❌ Missing frontmatter
❌ Name with spaces/special chars
❌ Description doesn't start with "Use when"
❌ Too vague instructions

✅ Correct: Clear phases, specific rules, proper location
