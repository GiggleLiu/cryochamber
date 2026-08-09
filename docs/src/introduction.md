# Cryochamber

**Cryochamber** is a hibernation chamber for AI agents (Claude, OpenCode, Codex, Pi, Kimi Code). It hibernates an AI agent between sessions and wakes it at the right time, not on a fixed schedule. The agent reads its plan, completes a task, and decides when to wake next. Cryochamber empowers AI agents to run tasks that span days, weeks, or even years, like interstellar travelers in stasis.

The goal is to automate long-running activities that are too irregular for cron. A conference deadline slips because submissions are low. A space probe's next burn window depends on orbital mechanics. A code review depends on when the author pushes fixes. Cryochamber lets an AI agent reason about *when* to wake and *what* to do next, with a persistent daemon that manages the lifecycle.

## Where to start

- **New here?** Follow the [Tutorial](./tutorial.md), about ten minutes from `cargo install` to a running chamber.
- **Already running chambers?** See [Create a chamber](./how-to/create-chamber.md) for setup recipes, [Monitor and message a chamber](./how-to/monitor-chambers.md) for Cryohub and remote messaging, the [CLI reference](./reference/cli.md) for commands, and [Configuration](./reference/configuration.md) for `cryo.toml` and `cryohub.toml`.
- **Want the mental model?** [Concepts](./explanation/concepts.md) explains chambers, sessions, and the message and TODO lifecycles. [Architecture](./explanation/architecture.md) is for contributors reading the source.
