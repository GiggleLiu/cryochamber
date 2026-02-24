# Multi-Binary Split Design

Split the single `cryo` binary into three separate binaries for conceptual clarity:
three distinct audiences get three distinct entry points.

## Binaries

| Binary | Audience | Commands |
|--------|----------|----------|
| **cryo** | Human operator | `init`, `start`, `status`, `ps`, `restart`, `cancel`, `log`, `watch`, `send`, `receive`, `fallback-exec`, `daemon` (hidden) |
| **cryo-agent** | AI agent | `hibernate`, `note`, `reply`, `alert` |
| **cryo-gh** | Human operator | `init`, `pull`, `push`, `sync`, `status` |

## File Layout

```
Cargo.toml             — three [[bin]] entries, one [lib]
src/
  lib.rs               — shared library (all pub modules)
  bin/
    cryo.rs            — operator CLI (~500 lines)
    cryo_agent.rs      — agent IPC CLI (~60 lines)
    cryo_gh.rs         — GitHub sync CLI (~150 lines)
```

Delete `src/main.rs`. Its code is distributed across the three bin files.

## Cargo.toml

```toml
[[bin]]
name = "cryo"
path = "src/bin/cryo.rs"

[[bin]]
name = "cryo-agent"
path = "src/bin/cryo_agent.rs"

[[bin]]
name = "cryo-gh"
path = "src/bin/cryo_gh.rs"
```

## Shared Helpers

Functions currently in `main.rs` that are used by multiple binaries move to the library:

| Helper | Destination |
|--------|-------------|
| `work_dir()` | `lib.rs` top-level |
| `state_path()` | `state` module |
| `log_path()` | `log` module |
| `send_signal()` | new `process` module |
| `terminate_pid()` | new `process` module |
| `spawn_daemon()` | new `process` module |

`gh_sync_path()` stays local to `cryo_gh.rs`.

## Protocol Updates

The agent protocol text in `src/protocol.rs` references `cryo hibernate`, `cryo note`,
`cryo reply`, `cryo alert`. All change to `cryo-agent`.

## Documentation Updates

- `CLAUDE.md` — update architecture to reflect three-binary structure
- `README.md` — update usage examples
- `cryo-skill.md` — update agent-facing command references
- `examples/` — update any plans referencing agent commands

## spawn_daemon

`cryo start` calls `spawn_daemon()` which uses `std::env::current_exe()` and passes
`"daemon"` as an arg. This continues to work because `cryo start` resolves to the `cryo`
binary, and `cryo daemon` is still a (hidden) subcommand of `cryo`.

## Installation

`cargo install --path .` installs all three binaries at once.
`cargo install --path . --bin cryo-agent` installs just one.
