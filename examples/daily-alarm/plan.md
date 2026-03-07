# Daily Alarm

## Goal

Wake up at 13:00 every day and send a greeting message.

## Tasks

1. Check current time: `cryo-agent time`
2. Send a greeting: `cryo-agent send "Hello! It's time for your daily check-in at $(cryo-agent time)"`
3. Schedule next wake at 13:00 tomorrow:
   - Get next 13:00: `cryo-agent time --daily 13:00`
   - Add TODO: `cryo-agent todo add "Daily greeting" --at <time>`
4. Hibernate: `cryo-agent hibernate --summary "Sent daily greeting"`

## Notes

- Use `--daily 13:00` to always wake at 13:00, regardless of current time
- This creates a true daily recurring schedule
