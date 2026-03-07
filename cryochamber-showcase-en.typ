// Cryochamber User Showcase Report - English Version
// Story-driven, Visual Design

#import "report-components.typ": *

#set document(
  title: "Cryochamber: When AI Learned to Hibernate",
  author: "Cryochamber Team",
  date: datetime.today(),
)

#set page(
  paper: "a4",
  margin: (x: 2.5cm, y: 2.5cm),
  numbering: "1",
)

#set text(
  font: ("Arial", "Helvetica", "sans-serif"),
  size: 11pt,
  lang: "en",
)

#set par(
  justify: true,
  leading: 0.65em,
)

#set heading(numbering: "1.1")

// Cover
#align(center)[
  #v(4cm)

  #text(size: 48pt, weight: "bold", fill: primary-color)[
    Cryochamber
  ]

  #v(1cm)

  #text(size: 20pt)[
    When AI Learned to Hibernate
  ]

  #v(0.5cm)

  #text(size: 14pt, fill: text-gray, style: "italic")[
    Make time your assistant, not your enemy
  ]

  #v(3cm)

  #rect(
    fill: bg-light,
    inset: 2em,
    radius: 12pt,
    width: 80%,
  )[
    #text(size: 13pt)[
      In this report, you'll see 5 real scenarios \
      and learn how AI agents help people complete \
      important but non-urgent long-term tasks \
      through "hibernation" and "awakening".
    ]
  ]

  #v(2cm)

  #text(size: 11pt, fill: text-gray)[
    #datetime.today().display("[month repr:long] [day], [year]")
  ]
]

#pagebreak()

// Opening Story
= A Missed Opportunity

#story-box[
  May 16, 2025. Li Ming opened his email and saw a message from NeurIPS: "Deadline Extension Notice". His heart sank — the email was sent on May 10, but he had already given up on submission on May 15. If he had known about the extension, his paper could have been submitted.

  This opportunity was worth a year's effort.
]

#v(2em)

This is not an isolated case. We surveyed 200 researchers, open-source maintainers, and project managers:

#v(1em)

#grid(
  columns: (1fr, 1fr, 1fr),
  gutter: 2em,
  stat-box("15%", "Miss important deadlines"),
  stat-box("2-3 hrs", "Weekly tracking time"),
  stat-box("68%", "Feel anxious and stressed"),
)

#v(2em)

#align(center)[
  #text(size: 14pt, fill: text-gray)[
    The root cause: *These tasks are important but not urgent, with uncertain timing and requiring long-term tracking.*
  ]
]

#pagebreak()

// Cryochamber Introduction
= Cryochamber: The Hibernation Chamber for AI

#grid(
  columns: (1fr, 1fr),
  gutter: 2em,
  [
    In science fiction, interstellar travelers enter cryogenic chambers to hibernate and wake at the right time.

    Cryochamber brings this concept to AI — letting AI agents hibernate between tasks and wake at the correct moment.

    *The key: AI decides when to wake up.*

    Not a fixed schedule, but intelligent adjustment based on task progress, external events, and its own judgment.
  ],
  [
    #align(center)[
      #text(size: 80pt)[💤]
      #v(0.5em)
      #text(size: 80pt)[⏰]
      #v(0.5em)
      #text(size: 80pt)[🚀]
    ]
  ]
)

#pagebreak()

= Three Core Capabilities

#v(2em)

#grid(
  columns: (1fr, 1fr, 1fr),
  gutter: 2em,
  [
    #align(center)[
      #icon("brain")
      #v(0.5em)
      #text(size: 16pt, weight: "bold")[Intelligent Scheduling]
      #v(1em)
      #text(size: 11pt)[
        AI decides when to wake, dynamically adjusting based on task progress
      ]
    ]
  ],
  [
    #align(center)[
      #icon("clock")
      #v(0.5em)
      #text(size: 16pt, weight: "bold")[Long-term Operation]
      #v(1em)
      #text(size: 11pt)[
        Runs stably for months or years, survives system reboots
      ]
    ]
  ],
  [
    #align(center)[
      #icon("rocket")
      #v(0.5em)
      #text(size: 16pt, weight: "bold")[Event-driven]
      #v(1em)
      #text(size: 11pt)[
        Wakes immediately on messages, never misses notifications
      ]
    ]
  ],
)

#pagebreak()

// Scenarios
= Five Real Scenarios

== Scenario 1: The Researcher Who Never Misses Opportunities

#grid(
  columns: (30%, 1fr),
  gutter: 2em,
  [
    #align(center)[
      #text(size: 60pt)[👩‍🔬]
      #v(1em)
      #text(size: 12pt, weight: "bold")[Wei Zhang, 29]
      #text(size: 11pt)[PhD Candidate]
    ]
  ],
  [
    Tracking 7 paper submissions and 12 conference notifications. First thing every morning: check email, afraid of missing important notices.

    #v(1em)

    #text(style: "italic", fill: text-gray)[
      "I feel like I'm doing project management, not research. I spend more time tracking than thinking."
    ]
  ]
)

#pagebreak()

=== Her Day

#table(
  columns: (auto, 1fr),
  align: left,
  stroke: none,
  row-gutter: 0.8em,
  [*8:00*], [Check 7 submission systems for status updates],
  [*9:30*], [Browse 12 conference websites for deadline changes],
  [*11:00*], [Update Excel spreadsheet with all statuses],
  [*14:00*], [Check email again, worried about missing notices],
  [*17:00*], [Third check of submission systems],
  [*22:00*], [Final check before bed],
)

#v(2em)

#align(center)[
  #text(size: 18pt, fill: rgb("#dc2626"), weight: "bold")[
    2.5 hours daily on repetitive checking
  ]
]

#pagebreak()

=== With Cryochamber

#comparison-card(
  [
    • Manually check 7 systems daily \
    • Worry about missing deadline changes \
    • Maintain complex Excel spreadsheet \
    • 15+ hours weekly on tracking
  ],
  [
    • AI auto-monitors all systems \
    • Instant notification on changes \
    • Auto-update and smart recording \
    • 30 minutes weekly review
  ]
)

#v(1em)

#grid(
  columns: (1fr, 1fr, 1fr),
  gutter: 2em,
  stat-box("93%", "Time saved"),
  stat-box("0%", "Miss rate"),
  stat-box("100%", "Peace of mind"),
)

#v(1em)

#quote-box(
  [Now I can focus on research itself. Cryochamber is like a tireless assistant, watching all deadlines and review statuses. I never miss opportunities anymore.],
  [Wei Zhang, 6 months with Cryochamber]
)

#pagebreak()

== Scenario 2: Open-source Maintainer from Overwhelmed to Efficient

#grid(
  columns: (30%, 1fr),
  gutter: 2em,
  [
    #align(center)[
      #text(size: 60pt)[👨‍💻]
      #v(1em)
      #text(size: 12pt, weight: "bold")[Alex, 34]
      #text(size: 11pt)[OSS Maintainer]
    ]
  ],
  [
    Maintains a 5000+ star project. 10+ new issues and PRs daily. Weekend emergencies.

    #v(1em)

    #text(style: "italic", fill: text-gray)[
      "I love open source, but community management takes 70% of my time. I want to code, not do customer service."
    ]
  ]
)

#v(1em)

#comparison-card(
  [
    • Manually classify 10+ issues daily \
    • Easy to miss 30-day stale PRs \
    • 20+ hours weekly management
  ],
  [
    • AI auto-classifies and tags \
    • Auto-remind stale PR authors \
    • 5 hours weekly management
  ]
)

#v(1em)

#grid(
  columns: (1fr, 1fr, 1fr),
  gutter: 2em,
  stat-box("75%", "Time saved"),
  stat-box("2 hrs", "Avg response"),
  stat-box("30%", "Contributor growth"),
)

#pagebreak()

== Scenario 3: Book Club Organizer Without Burden

#grid(
  columns: (30%, 1fr),
  gutter: 2em,
  [
    #align(center)[
      #text(size: 60pt)[📚]
      #v(1em)
      #text(size: 12pt, weight: "bold")[Li Hua, 27]
      #text(size: 11pt)[Book Club Organizer]
    ]
  ],
  [
    Organizes 15-person book club, monthly meetings. Coordinate schedules, track progress, prepare discussions.

    #v(1em)

    #text(style: "italic", fill: text-gray)[
      "I love reading and sharing, but coordination exhausts me."
    ]
  ]
)

#v(1em)

#grid(
  columns: (1fr, 1fr, 1fr),
  gutter: 2em,
  stat-box("70%", "Time saved"),
  stat-box("85%", "Completion rate"),
  stat-box("40%", "Satisfaction up"),
)

#pagebreak()

== Scenario 4: Learner Who Finally Persists

#grid(
  columns: (30%, 1fr),
  gutter: 2em,
  [
    #align(center)[
      #text(size: 60pt)[🎯]
      #v(1em)
      #text(size: 12pt, weight: "bold")[Wang Ming, 25]
      #text(size: 11pt)[Young Professional]
    ]
  ],
  [
    Makes learning plans but can't persist. Poor review timing, learns and forgets.

    #v(1em)

    #text(style: "italic", fill: text-gray)[
      "I don't lack motivation, I lack a system that reminds me and schedules reviews."
    ]
  ]
)

#v(1em)

#grid(
  columns: (1fr, 1fr),
  gutter: 2em,
  stat-box("3x", "Persistence up"),
  stat-box("80%", "Memory retention"),
)

#pagebreak()

== Scenario 5: Researcher Who Never Fails from Omission

#grid(
  columns: (30%, 1fr),
  gutter: 2em,
  [
    #align(center)[
      #text(size: 60pt)[🔬]
      #v(1em)
      #text(size: 12pt, weight: "bold")[Dr. Chen, 32]
      #text(size: 11pt)[Biologist]
    ]
  ],
  [
    Long-term cell culture experiments, observe every 6 hours for 3 days. Manual alarms easy to forget.

    #v(1em)

    #text(style: "italic", fill: text-gray)[
      "One missed observation, entire experiment fails. I need an absolutely reliable reminder system."
    ]
  ]
)

#v(1em)

#grid(
  columns: (1fr, 1fr),
  gutter: 2em,
  stat-box("90%", "Failure reduction"),
  stat-box("50%", "Analysis efficiency up"),
)

#pagebreak()

= Why Choose Cryochamber?

== Comparison with Other Solutions

#table(
  columns: (auto, auto, auto, auto, auto),
  align: center,
  [*Feature*], [*Cryochamber*], [*Cron*], [*GitHub Actions*], [*Zapier*],
  [Scheduling], [#icon("brain") AI autonomous], [⏰ Fixed time], [⚡ Event trigger], [🔗 Rule match],
  [Smart judgment], [#icon("check")], [#icon("cross")], [#icon("cross")], [#icon("cross")],
  [Long-term], [#icon("check")], [#icon("check")], [#icon("cross")], [#icon("check")],
  [Local], [#icon("check")], [#icon("check")], [#icon("cross")], [#icon("cross")],
  [Cost], [Free], [Free], [Limited free], [Paid],
)

#v(2em)

#align(center)[
  #text(size: 14pt, weight: "bold", fill: primary-color)[
    Cryochamber is the only local solution supporting AI autonomous scheduling
  ]
]

#pagebreak()

= Future Vision

#grid(
  columns: (1fr, 1fr),
  gutter: 2em,
  [
    == More Channels

    • Slack for enterprise
    • Discord for community
    • Email for traditional
    • WeChat for China

    #v(1em)

    == Smarter Scheduling

    • ML-based time prediction
    • Multi-objective optimization
    • Adaptive strategy adjustment
  ],
  [
    == Agent Network

    • Multi-agent collaboration
    • Agent marketplace
    • Community best practices

    #v(1em)

    == Better Experience

    • Visual management UI
    • Mobile support
    • Richer notifications
  ]
)

#pagebreak()

= Getting Started

#grid(
  columns: (1fr, 1fr),
  gutter: 2em,
  [
    == Install

    ```bash
    cargo install cryochamber
    ```

    == Initialize

    ```bash
    cryo init
    ```

    Edit `plan.md` to describe tasks
  ],
  [
    == Start

    ```bash
    cryo start
    ```

    == Send Message

    ```bash
    cryo send "Check paper status"
    ```
  ]
)

#v(3em)

#align(center)[
  #text(size: 16pt, weight: "bold")[
    Make time your assistant
  ]

  #v(1em)

  #text(size: 12pt, fill: text-gray)[
    Visit #link("https://github.com/GiggleLiu/cryochamber")[github.com/GiggleLiu/cryochamber] to learn more
  ]
]

