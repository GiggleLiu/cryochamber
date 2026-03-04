# Reminder Agent

A personal reminder agent that wakes up daily at 9AM to review deadlines and send timely reminders.

Users add tasks via inbox messages. The agent checks the TODO list each morning,
sends reminders at appropriate lead times (1 week for projects, 1 day for general tasks),
and tracks which reminders have been sent to avoid spam.

Demonstrates: daily scheduling, TODO-driven wake, inbox message parsing,
`cryo-agent note` for cross-session state, two-way messaging.

## Quick Start

```bash
cd examples/reminder-agent
cryo init && cryo start
```

## Adding Tasks

Send a message to the agent's inbox:

```bash
cryo send "Please remind me to submit project report by 2026-03-07"
```

The agent will parse the deadline, add it to the TODO list, and send reminders
at 1 week and 1 day before the due date.

## What You'll See

```
Session 1 (Feb 28, 9AM):
  → Parses inbox message, adds TODO: "Submit project report" at 2026-03-07
  → Sends 1-week reminder with advice
  → Hibernates until tomorrow 9AM

Session 2 (Mar 1, 9AM):
  → No new reminders needed (already sent 7-day notice)
  → Hibernates until tomorrow 9AM

...

Session 7 (Mar 6, 9AM):
  → Sends 1-day reminder: "Project report due tomorrow!"
  → Hibernates until tomorrow 9AM

Session 8 (Mar 7, 9AM):
  → Sends urgent day-of reminder
  → Daemon auto-completes the TODO after session
```

## Configuration

- **Schedule**: daily at 9AM, with extra wakes before imminent deadlines
- **Watch inbox**: enabled (react to new tasks immediately)
- **Retries**: 5 (resilient to transient agent failures)
