# Create a chamber

A chamber is a directory with three files: `plan.md` for what the agent does, `cryo.toml` for chamber config, and `NOTES.md` for the agent's cross-session memory. You have three ways to make one.

## Option A: Use the `make-plan` skill (recommended)

If your AI agent supports custom skills, the bundled `make-plan` skill walks you through `plan.md` and `cryo.toml` interactively.

1. Install the skill in your agent. Point your agent's skill installer at:

   ```text
   <repo>/.claude/skills/make-plan
   ```

2. Open your agent in the directory where you want the chamber. Prompt it:

   > Invoke the `make-plan` skill to create a new cryochamber project here.

3. Answer the agent's questions. When the skill finishes, the directory contains `plan.md`, `cryo.toml`, and `NOTES.md`.

## Option B: Scaffold by hand with `cryo init`

For a blank chamber:

```bash
mkdir -p ~/.cryo/chambers/my-chamber
cd ~/.cryo/chambers/my-chamber
cryo init
```

Then edit `plan.md` to describe the goal and tasks, and optionally edit `cryo.toml` to change the agent or session timeout. See [Configuration](../reference/configuration.md).

## Option C: Copy an example

```bash
cp -r <repo>/examples/chambers/mr-lazy ~/.cryo/chambers/my-chamber
```

The bundled examples are `mr-lazy`, `chess-by-mail`, and `personal-assistant`.

## What's in a chamber

- `plan.md` - the goal, tasks, and any persistent state the agent should track. The agent reads this every session.
- `cryo.toml` - chamber config: agent command, session timeout, inbox watcher, sync polling intervals, provider environment. See [Configuration](../reference/configuration.md).
- `NOTES.md` - the agent's persistent memory. The agent reads and appends directly; there is no IPC command for it.

For details on what these files mean and how a session uses them, see [Concepts](../explanation/concepts.md).

## Next: run it

Once your chamber has `plan.md` and `cryo.toml`, follow the [Tutorial](../tutorial.md) from `cryo start` onward, or jump straight into [monitoring](./monitor-chambers.md).
