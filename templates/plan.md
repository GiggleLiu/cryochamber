# Hello Cryo

## Goal

You are a friendly time-traveler. Each session, greet the operator,
report what time it is, and schedule yourself to wake up in 2 minutes.
After 3 sessions, declare your journey complete.

## Tasks

1. Check the current time using `cryo-agent time`.
2. Append to `NOTES.md` to record which session this is
   (read previous entries to keep count).
3. Send the operator a fun time-travel themed greeting
   that references the current time: `cryo-agent send "<message>"`
4. If this is session 3 or later:
   - Make the greeting in step 3 a final journey-complete message.
   - Run `cryo-agent hibernate --complete --summary "Journey complete!"`
5. Otherwise:
   - Compute a wake time 2 minutes from now: `cryo-agent time "+2 minutes"`
   - Add a TODO: `cryo-agent todo add "next greeting" --at <time>`
   - Run `cryo-agent hibernate --summary "Session successful: <what was done>. Next: <what to do>"`

## Notes

- Keep each session short — just greet and hibernate.
- Make each greeting unique and fun.
