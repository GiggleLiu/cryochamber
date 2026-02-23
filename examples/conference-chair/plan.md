# Conference Program Chair

Manage a CS conference from call-for-papers through author notification.

## Goal

Automate the full program chair workflow: send CFP, monitor submissions, assign reviewers, chase late reviews, compile decisions, notify authors. The timeline spans ~3 months and adapts to human delays.

## Tasks

1. Send call for papers to mailing lists. Set soft deadline (2 weeks) and hard deadline (4 weeks).
2. At soft deadline: check submission count. If below target (40), extend deadline and send reminders.
3. After deadline: close submissions. Read abstracts, assign 3 reviewers per paper by keyword matching. Send review invitations with 3-week window.
4. At 2-week mark: check review completion rate. Send reminders to outstanding reviewers.
5. If reviews still missing within 5 days of deadline: send urgent reminders, assign emergency backup reviewers.
6. When all reviews are in: compile scores, flag borderline papers (scores 5.0–6.5), write summary report for PC meeting.
7. After PC meeting: read decisions from shared document, send personalized accept/reject notifications with reviewer feedback.

## Configuration

- Submission portal: https://easychair.example.org/conf2026
- Reviewer pool: reviewers.csv (name, email, keywords)
- Mailing lists: announce@conf.org, ml-news@lists.example
- Target submissions: 40
- Notification email: chair@conf.org

## Notes

- Deadline extensions cascade: pushing the submission deadline pushes reviewer assignment, which pushes the review deadline, which pushes notification.
- Escalation is progressive: gentle reminder → urgent reminder → backup assignment. Interval between checks should shrink as deadlines approach.
- If a reviewer declines, reassign immediately rather than waiting for the next scheduled check.
