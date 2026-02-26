# FAQ

## What happens if my computer sleeps or reboots?

**Sleep:** The daemon process is suspended along with everything else. When your machine wakes up, the daemon resumes and detects that the scheduled wake time has passed. It runs the session immediately and includes a "DELAYED WAKE" notice in the agent's prompt with the original scheduled time and how late the session is.

**Reboot:** The daemon is installed as an OS service (launchd on macOS, systemd on Linux) and restarts automatically after reboot. Set `CRYO_NO_SERVICE=1` before `cryo start` to disable this and use a plain background process instead.

## How do I manually wake a sleeping daemon?

Use `cryo wake` to send a message to the daemon's inbox. You can include a message: `cryo wake "Please check the latest PR"`. If inbox watching is enabled (the default), the daemon wakes immediately. You can also use `cryo send --wake` for the same effect. If inbox watching is disabled, `cryo wake` sends a SIGUSR1 signal to force the daemon awake. If no daemon is running, the message is queued for the next `cryo start`.
