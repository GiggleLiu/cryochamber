// ─────────────────────────────────────────────────────────────────────────────
//  Cryochamber — 产品使用指南（中文版）
// ─────────────────────────────────────────────────────────────────────────────
#import "@preview/cetz:0.4.0"

#set page(paper: "a4", margin: (top: 2.5cm, bottom: 2.5cm, left: 2.4cm, right: 2.4cm))
#set text(size: 10.5pt, font: ("SimSun", "New Computer Modern"))
#set heading(numbering: "1.1.")
#set par(justify: true, leading: 0.75em)

// ── 颜色方案 ──────────────────────────────────────────────────────────────────
#let ocean   = rgb("#1a73a7")
#let teal    = rgb("#2bb5a0")
#let amber   = rgb("#e08b2a")
#let rose    = rgb("#c0392b")
#let sage    = rgb("#5a9a6f")
#let slate   = rgb("#5d6d7e")
#let linen   = rgb("#fdf6ec")
#let icebg   = rgb("#edf4fb")

// ── 提示框 ────────────────────────────────────────────────────────────────────
#let callout(color: ocean, icon: "ℹ", title: "", body) = block(
  stroke: (left: 3pt + color),
  inset: (left: 10pt, top: 6pt, bottom: 6pt, right: 8pt),
  radius: 2pt,
  fill: color.lighten(92%),
  width: 100%,
)[
  #if title != "" { text(weight: "bold", fill: color)[#icon  #title \ ] }
  #body
]

// ── 命令行代码块 ──────────────────────────────────────────────────────────────
#let cmd(body) = block(
  fill: rgb("#1e1e2e"), radius: 4pt, inset: 10pt, width: 100%,
)[#text(fill: rgb("#cdd6f4"), font: "Courier New", size: 9pt)[#body]]

// ── 封面 ──────────────────────────────────────────────────────────────────────
#align(center)[
  #v(2cm)
  #text(size: 28pt, weight: "bold", fill: ocean)[Cryochamber]
  #v(0.3cm)
  #text(size: 15pt, fill: slate)[_AI 智能体的冬眠调度器_]
  #v(0.5cm)
  #line(length: 60%, stroke: 1.5pt + ocean)
  #v(0.4cm)
  #text(size: 12pt)[*产品使用指南*]
  #v(0.2cm)
  #text(size: 10pt, fill: slate)[面向运维者、开发者及感兴趣的读者]
  #v(3cm)
]

#pagebreak()

// ── 目录 ──────────────────────────────────────────────────────────────────────
#outline(title: "目录", depth: 2, indent: 1.5em)

#pagebreak()

// ─────────────────────────────────────────────────────────────────────────────
= Cryochamber 是什么？
// ─────────────────────────────────────────────────────────────────────────────

*Cryochamber*（冰室）是一个用 Rust 编写的开源 AI 智能体调度器。它解决了一个根本性问题：_大型任务的执行时间往往超过单次 LLM 上下文窗口的承载能力。_

现代 AI 智能体（Claude、OpenCode、GPT-4……）能写代码、浏览网页、开发软件——但它们会超时、触达速率限制，或者需要暂停、隔夜继续。Cryochamber 为它们提供了一个持久化的运行框架：

#grid(columns: (1fr, 1fr), gutter: 16pt)[
  #callout(color: rose, icon: "⚠", title: "问题所在")[
    - 智能体在任务中途超时
    - 两次运行之间没有记忆
    - 需要人工手动重启
    - API 速率限制阻断夜间工作
  ]
][
  #callout(color: sage, icon: "✓", title: "Cryochamber 提供的能力")[
    - 通过 TODO 列表实现定时唤醒
    - 跨会话的持久化日志与备注
    - 人类与智能体之间的消息通信
    - 崩溃后自动重试
  ]
]

这个名字来自一个比喻：智能体在两次会话之间被放入*冰室（cryochamber）*——状态被冻结，资源被释放，并在恰当的时刻被唤醒。

// ─────────────────────────────────────────────────────────────────────────────
= 核心概念
// ─────────────────────────────────────────────────────────────────────────────

== 会话（Session）
*会话*是智能体进程的一次运行。每个会话都有编号、任务描述、开始时间戳和结束结果（`complete`、`exit <N>`、`timeout`……）。会话在 `cryo.log` 中以 `--- CRYO SESSION N ---` / `--- CRYO END ---` 标记分隔。

== 守护进程（Daemon）
*守护进程*是一个由操作系统服务层管理的长期后台进程（macOS 用 launchd、Linux 用 systemd、Windows 用 SCM）。它负责：
- 等待直到最早的待办 TODO 时间；
- 为每个会话启动智能体子进程；
- 强制执行可配置的会话超时；
- 以指数退避策略重试崩溃的智能体（5秒 → 15秒 → 60秒）；
- 监听 `messages/inbox/` 以实现响应式唤醒。

== IPC 通信
智能体子进程通过平台 IPC 通道（Unix 域套接字 / Windows 命名管道）与守护进程通信，使用 `cryo-agent` CLI。这使智能体完全解耦——它只需执行 shell 命令即可。

== TODO 列表
智能体通过 `todo.json` 控制自己的唤醒调度。添加一个带有未来 `--at` 时间的 TODO，守护进程就会休眠到该时间后重新启动智能体。若没有待处理的 TODO，守护进程将一直空闲。

== 收件箱 / 发件箱（Inbox / Outbox）
`messages/` 目录中有一个简单的基于文件的消息总线。人类通过 `cryo send` 向 `inbox/` 写入消息；智能体在会话开始时读取这些消息。智能体将回复写入 `outbox/`；人类通过 `cryo receive` 读取。GitHub Discussions 和 Zulip 等渠道可以将此消息总线镜像到互联网上。

// ─────────────────────────────────────────────────────────────────────────────
= 系统架构
// ─────────────────────────────────────────────────────────────────────────────

#figure(
  cetz.canvas(length: 1cm, {
    import cetz.draw: *

    // ── 样式定义 ──────────────────────────────────────────────────────────
    let box-stroke   = (paint: ocean,  thickness: 1.2pt)
    let dash-stroke  = (paint: slate,  thickness: 0.8pt, dash: "dashed")
    let arrow        = (end: "straight")
    let bkg          = icebg
    let human-fill   = amber.lighten(75%)
    let daemon-fill  = ocean.lighten(82%)
    let agent-fill   = teal.lighten(80%)
    let fs-fill      = sage.lighten(80%)
    let ch-fill      = rose.lighten(82%)

    // ── 人类操作者（左上） ────────────────────────────────────────────────
    rect((0, 7), (2.8, 8.4), fill: human-fill, stroke: box-stroke, name: "human")
    content("human.center", align(center)[*人类*\ 操作者])

    // ── cryo CLI（顶部中央） ──────────────────────────────────────────────
    rect((4, 7), (7.2, 8.4), fill: daemon-fill, stroke: box-stroke, name: "cli")
    content("cli.center", align(center)[*cryo* CLI\ `init / start / send / receive`])

    // ── 守护进程（中央） ─────────────────────────────────────────────────
    rect((4, 4.6), (7.2, 6.6), fill: daemon-fill, stroke: box-stroke, name: "daemon")
    content("daemon.center", align(center)[*守护进程*\ 事件循环\ IPC 服务器])

    // ── 智能体子进程（右侧） ─────────────────────────────────────────────
    rect((8.8, 4.6), (12, 6.6), fill: agent-fill, stroke: box-stroke, name: "agent")
    content("agent.center", align(center)[*智能体子进程*\ (opencode / claude /\ 任意 CLI 工具)])

    // ── cryo-agent CLI（右下） ────────────────────────────────────────────
    rect((8.8, 2.2), (12, 3.8), fill: agent-fill, stroke: box-stroke, name: "cryo-agent")
    content("cryo-agent.center", align(center)[*cryo-agent* CLI\ `hibernate / note / todo`])

    // ── 文件系统（底部中央） ─────────────────────────────────────────────
    rect((3.5, 0.3), (8.2, 2.2), fill: fs-fill, stroke: box-stroke, name: "fs")
    content("fs.center", align(center)[
      *项目目录（文件系统）*\
      `cryo.toml  plan.md  cryo.log`\
      `messages/inbox/   messages/outbox/`
    ])

    // ── 消息渠道（左下） ─────────────────────────────────────────────────
    rect((0, 2.2), (2.8, 3.8), fill: ch-fill, stroke: box-stroke, name: "channels")
    content("channels.center", align(center)[*消息渠道*\ GitHub Discussions\ Zulip])

    // ── 箭头 ──────────────────────────────────────────────────────────────
    // 人类 → cryo CLI
    line("human.east", "cli.west",
         mark: arrow, stroke: (paint: ocean, thickness: 1.2pt))
    content(("human.east", 50%, "cli.west"), [  `cryo`\ 命令  ],
            fill: white, frame: "rect", stroke: none, padding: 0.1)

    // cryo CLI → 守护进程
    line("cli.south", "daemon.north",
         mark: arrow, stroke: (paint: ocean, thickness: 1.2pt))
    content(("cli.south", 50%, "daemon.north"), [],
            fill: white, frame: "rect", stroke: none, padding: 0.04)

    // 守护进程 → 智能体（启动）
    line("daemon.east", "agent.west",
         mark: arrow, stroke: (paint: teal, thickness: 1.4pt))
    content(("daemon.east", 50%, "agent.west"), [启动],
            fill: white, frame: "rect", stroke: none, padding: 0.08)

    // 智能体 → cryo-agent
    line("agent.south", "cryo-agent.north",
         mark: arrow, stroke: (paint: teal, thickness: 1.2pt))
    content(("agent.south", 50%, "cryo-agent.north"), [调用],
            fill: white, frame: "rect", stroke: none, padding: 0.08)

    // cryo-agent → 守护进程（IPC）
    line("cryo-agent.west", (5.6, 2.8), "daemon.south",
         mark: arrow, stroke: (paint: slate, thickness: 1pt, dash: "dashed"))
    content(((5.6, 2.8)), [IPC 套接字],
            fill: white, frame: "rect", stroke: none, padding: 0.08)

    // 守护进程 ↔ 文件系统
    line("daemon.south", "fs.north",
         mark: (start: "straight", end: "straight"),
         stroke: (paint: sage, thickness: 1.2pt))
    content(("daemon.south", 50%, "fs.north"), [读/写],
            fill: white, frame: "rect", stroke: none, padding: 0.08)

    // 消息渠道 ↔ 文件系统（同步）
    line("channels.east", "fs.west",
         mark: (start: "straight", end: "straight"),
         stroke: (paint: rose, thickness: 1.2pt))
    content(("channels.east", 50%, "fs.west"), [同步],
            fill: white, frame: "rect", stroke: none, padding: 0.08)

    // 人类 → 消息渠道
    line("human.south", "channels.north",
         mark: (start: "straight", end: "straight"),
         stroke: (paint: amber, thickness: 1.2pt))
    content(("human.south", 50%, "channels.north"), [消息],
            fill: white, frame: "rect", stroke: none, padding: 0.08)
  }),
  caption: [Cryochamber 组件架构图。],
)

该图展示了五个主要参与者：

#table(
  columns: (auto, 1fr),
  stroke: 0.5pt + luma(180),
  inset: 7pt,
  fill: (col, row) => if row == 0 { ocean.lighten(88%) } else { white },
  [*参与者*], [*职责*],
  [`cryo` CLI], [操作者的入口点——初始化项目、启动/停止守护进程、查看日志、发送消息。],
  [守护进程], [长期后台进程。管理会话生命周期、IPC 服务器和唤醒逻辑。],
  [智能体子进程], [实际运行的 AI 工具（如 `opencode run`）。完全可替换——任何 CLI 程序均可。],
  [`cryo-agent` CLI], [智能体的工具箱。通过 IPC 发送命令：冬眠、添加备注、安排待办、发送消息。],
  [消息渠道], [可选的桥接器（GitHub Discussions、Zulip），将收件箱/发件箱镜像到互联网服务。],
)

// ─────────────────────────────────────────────────────────────────────────────
= 会话生命周期
// ─────────────────────────────────────────────────────────────────────────────

#figure(
  cetz.canvas(length: 1cm, {
    import cetz.draw: *

    let st = (paint: ocean, thickness: 1.2pt)
    let arrow = (end: "stealth")
    let state-fill = ocean.lighten(85%)
    let act-fill   = teal.lighten(82%)
    let err-fill   = rose.lighten(82%)

    // 状态节点
    let states = (
      ("idle",     (5.5, 8.2), [*空闲 / 休眠*]),
      ("check",    (5.5, 6.2), [*检查唤醒条件*]),
      ("spawn",    (5.5, 4.2), [*启动智能体*]),
      ("running",  (5.5, 2.2), [*智能体运行中*]),
      ("exited",   (9.5, 2.2), [*智能体已退出*]),
      ("success",  (9.5, 0.2), [*记录成功\ 安排下次*]),
      ("crash",    (1.5, 2.2), [*崩溃 /\ 超时*]),
      ("retry",    (1.5, 0.2), [*退避重试\  (5→15→60 秒)*]),
    )

    for (name, pos, label) in states {
      let fill = if name == "crash" or name == "retry" { err-fill }
                 else if name == "spawn" or name == "running" { act-fill }
                 else { state-fill }
      let (x, y) = pos
      rect((x - 1.8, y - 0.65), (x + 1.8, y + 0.65),
           fill: fill, stroke: st, radius: 4pt, name: name)
      content(name + ".center", align(center, label))
    }

    // 状态转换
    // 空闲 → 检查
    line("idle.south", "check.north", mark: arrow, stroke: st)
    content(("idle.south", 55%, "check.north"),
      [唤醒时间\ 或收件箱事件], fill: white, frame: "rect", stroke: none, padding: 0.08)

    // 检查 → 启动（满足条件）
    line("check.south", "spawn.north", mark: arrow, stroke: st)
    content(("check.south", 55%, "spawn.north"),
      [就绪], fill: white, frame: "rect", stroke: none, padding: 0.08)

    // 检查 → 空闲（不满足条件）
    bezier("check.east", "idle.east",
           (9, 6.2), (9, 8.2),
           mark: arrow, stroke: (paint: slate, thickness: 0.9pt, dash: "dashed"))
    content((9.3, 7.2), [未到时间], fill: white, frame: "rect", stroke: none, padding: 0.08)

    // 启动 → 运行中
    line("spawn.south", "running.north", mark: arrow, stroke: st)

    // 运行中 → 已退出（正常退出）
    line("running.east", "exited.west", mark: arrow, stroke: (paint: sage, thickness: 1.2pt))
    content(("running.east", 50%, "exited.west"),
      [冬眠\ 或 exit 0], fill: white, frame: "rect", stroke: none, padding: 0.08)

    // 已退出 → 成功
    line("exited.south", "success.north", mark: arrow, stroke: (paint: sage, thickness: 1.2pt))

    // 成功 → 空闲
    bezier("success.east", "idle.east",
           (12, 0.2), (12, 8.2),
           mark: arrow, stroke: (paint: sage, thickness: 1pt))
    content((12.6, 4.2), [下次唤醒], fill: white, frame: "rect", stroke: none, padding: 0.08)

    // 运行中 → 崩溃（非零退出）
    line("running.west", "crash.east", mark: arrow, stroke: (paint: rose, thickness: 1.2pt))
    content(("running.west", 50%, "crash.east"),
      [非零\ 退出码], fill: white, frame: "rect", stroke: none, padding: 0.08)

    // 崩溃 → 退避重试
    line("crash.south", "retry.north", mark: arrow, stroke: (paint: rose, thickness: 1.2pt))

    // 退避重试 → 启动
    bezier("retry.north", "spawn.west",
           (1.5, 1.4), (3.7, 4.2),
           mark: arrow, stroke: (paint: rose, thickness: 1pt, dash: "dashed"))
    content((2.3, 2.8), [重试], fill: white, frame: "rect", stroke: none, padding: 0.08)

    // 退避重试 → 空闲（超过最大重试次数）
    bezier("retry.south", "idle.west",
           (1.5, -1.2), (-1.2, 8.2),
           mark: arrow, stroke: (paint: rose, thickness: 0.9pt, dash: "dotted"))
    content((-1.5, 3.5), text(size: 8pt)[耗尽\ → 告警], fill: white, frame: "rect", stroke: none, padding: 0.08)
  }),
  caption: [守护进程会话生命周期状态机。],
)

守护进程无限循环执行。关键行为：

- *正常退出*：智能体调用 `cryo-agent hibernate`；守护进程记录会话并从 `todo.json` 安排下次唤醒。
- *崩溃/超时*：守护进程以指数退避策略重试。在达到 `max_retries` 次连续失败后，触发*备用告警*（死亡开关）写入发件箱。
- *响应式唤醒*：`messages/inbox/` 中出现新文件，或执行 `cryo wake` 命令，均可提前结束休眠。

// ─────────────────────────────────────────────────────────────────────────────
= 快速上手
// ─────────────────────────────────────────────────────────────────────────────

== 安装

#cmd[
  ```
  cargo install cryochamber        # 从 crates.io 安装
  # 或者从源码安装：
  git clone https://github.com/GiggleLiu/cryochamber
  cd cryochamber && cargo install --path .
  ```
]

安装完成后会得到四个可执行文件：`cryo`、`cryo-agent`、`cryo-gh`、`cryo-zulip`。

== 创建项目

#cmd[
  ```
  mkdir my-ai-project && cd my-ai-project
  echo "# 计划\n在此编写任务计划。" > plan.md
  cryo init --agent "opencode run"
  ```
]

`cryo init` 会创建：
- `cryo.toml` — 项目配置文件
- `plan.md` — （已存在）每次会话开始时由智能体读取
- `CLAUDE.md` / `AGENTS.md` — 注入智能体上下文的 Cryochamber 协议

== 启动守护进程

#cmd[
  ```
  cryo start                        # 使用 cryo.toml 中的智能体
  cryo start --agent "claude -p"    # 临时覆盖智能体命令
  ```
]

在 macOS/Linux 上，这会安装一个能在重启后继续运行的 *launchd* / *systemd* 服务。
设置 `CRYO_NO_SERVICE=1` 可跳过服务安装，以普通后台进程方式运行。

== 监控进度

#cmd[
  ```
  cryo status          # 守护进程状态、会话编号、下次唤醒时间
  cryo watch           # 实时跟踪 cryo.log（类似 tail -f）
  cryo watch --all     # 从日志开头显示
  cryo log             # 输出完整结构化日志
  ```
]

== 发送与接收消息

#cmd[
  ```
  cryo send "请在下次会话前审查 PR"
  cryo receive                     # 从发件箱读取智能体的回复
  ```
]

`cryo send` 会在 `messages/inbox/` 中放置一条 Markdown 消息。守护进程会唤醒智能体（如果正在休眠），智能体在下次会话开始时读取该消息。

== 停止

#cmd[
  ```
  cryo cancel          # 停止守护进程并移除服务
  cryo clean --force   # 停止 + 删除所有运行时文件
  ```
]

// ─────────────────────────────────────────────────────────────────────────────
= 智能体协议
// ─────────────────────────────────────────────────────────────────────────────

守护进程启动智能体时，会在提示词顶部注入一个 *Cryochamber 协议块*。该块指导智能体使用 `cryo-agent` 子命令进行结构化通信。

#callout(color: amber, icon: "📋", title: "会话工作流（智能体视角）")[
  #set list(marker: "→")
  - *第一步 — 定向*：读取 `plan.md`，执行 `cryo-agent todo list`，检查收件箱。
  - *第二步 — 工作*：完成任务。用 `cryo-agent reply` 回复收件箱消息。
  - *第三步 — 记录*：`cryo-agent note "完成了什么，下一步是什么"`。
  - *第四步 — 调度*：`cryo-agent todo add "下一个任务" --at <时间>`，时间用 `cryo-agent time "+2 hours"` 计算。
  - *第五步 — 冬眠*：`cryo-agent hibernate --summary "…"`（必须是最后一条命令）。
]

#table(
  columns: (auto, 1fr),
  stroke: 0.5pt + luma(180),
  inset: 7pt,
  fill: (col, row) => if row == 0 { teal.lighten(82%) } else { white },
  [*cryo-agent 命令*], [*功能说明*],
  [`hibernate --summary "…"`], [结束会话。守护进程记录结果并安排下次唤醒。],
  [`hibernate --complete`], [标记整个项目已完成。],
  [`hibernate --exit 1 --summary "…"`], [标记失败 / 阻塞状态。],
  [`note "文本"`], [向会话日志追加一条备注。下次会话可通过 `parse_latest_session_notes` 读取。],
  [`send "消息"`], [向 `messages/outbox/` 写入一条消息。],
  [`reply "消息"`], [回复当前收件箱消息。],
  [`receive`], [读取 `messages/inbox/`（本地操作，无需守护进程）。],
  [`alert <动作> <目标> "消息"`], [设置死亡开关：若智能体未按时唤醒，则对 `<目标>` 触发 `<动作>`。],
  [`todo add "任务" --at <ISO>`], [添加带时间的待办事项。守护进程在最早的待办时间唤醒。],
  [`todo list`], [列出所有待处理的待办事项。],
  [`todo done <id>`], [将某个待办标记为已完成。],
  [`time`], [打印当前 ISO8601 本地时间。],
  [`time "+2 hours"`], [计算并打印未来某时刻的 ISO8601 时间。],
)

// ─────────────────────────────────────────────────────────────────────────────
= 人类 ↔ 智能体通信
// ─────────────────────────────────────────────────────────────────────────────

#figure(
  cetz.canvas(length: 1cm, {
    import cetz.draw: *

    let bst  = (paint: ocean,  thickness: 1.2pt)
    let chst = (paint: rose,   thickness: 1.1pt)
    let fst  = (paint: sage,   thickness: 1.1pt)
    let arrow = (end: "straight")

    // ── 节点 ──────────────────────────────────────────────────────────────
    rect((0,  3.2), (2.6, 4.4), fill: amber.lighten(75%), stroke: bst, name: "human")
    content("human.center", align(center)[*人类*])

    rect((4.2, 3.2), (7.4, 4.4), fill: ocean.lighten(82%), stroke: bst, name: "daemon")
    content("daemon.center", align(center)[*守护进程*])

    rect((9.2, 3.2), (12, 4.4), fill: teal.lighten(80%), stroke: bst, name: "agent")
    content("agent.center", align(center)[*智能体*])

    // 收件箱目录
    rect((4.2, 1.2), (7.4, 2.4), fill: sage.lighten(80%), stroke: fst, name: "inbox")
    content("inbox.center", align(center)[`messages/inbox/`])

    // 发件箱目录
    rect((4.2, 5.2), (7.4, 6.4), fill: sage.lighten(80%), stroke: fst, name: "outbox")
    content("outbox.center", align(center)[`messages/outbox/`])

    // 输入渠道
    rect((0, 1.2), (2.6, 2.4), fill: rose.lighten(82%), stroke: chst, name: "ch-in")
    content("ch-in.center", align(center)[消息渠道\ (GitHub/Zulip)])

    // 输出渠道
    rect((0, 5.2), (2.6, 6.4), fill: rose.lighten(82%), stroke: chst, name: "ch-out")
    content("ch-out.center", align(center)[消息渠道\ (GitHub/Zulip)])

    // ── 箭头 ──────────────────────────────────────────────────────────────
    // 人类 → 收件箱（直接）
    line("human.south", (1.3, 2.4), "inbox.west",
         mark: arrow, stroke: bst)
    content(((1.3, 2.8)), [`cryo send`], fill: white, frame: "rect", stroke: none, padding: 0.08)

    // 渠道 → 收件箱（同步拉取）
    line("ch-in.east", "inbox.west",
         mark: arrow, stroke: chst)
    content(("ch-in.east", 50%, "inbox.west"), [拉取], fill: white, frame: "rect", stroke: none, padding: 0.08)

    // 人类 → 渠道（发布消息）
    line("human.south", "ch-in.north",
         mark: arrow, stroke: chst)

    // 收件箱 → 守护进程（监听/唤醒）
    line("inbox.north", "daemon.south",
         mark: arrow, stroke: fst)
    content(("inbox.north", 60%, "daemon.south"), [唤醒 +\ 读取], fill: white, frame: "rect", stroke: none, padding: 0.08)

    // 守护进程 → 智能体（注入提示词）
    line("daemon.east", "agent.west",
         mark: arrow, stroke: bst)
    content(("daemon.east", 50%, "agent.west"), [提示词], fill: white, frame: "rect", stroke: none, padding: 0.08)

    // 智能体 → 发件箱
    line("agent.south", (10.6, 2.4), "outbox.east",
         mark: arrow, stroke: (paint: teal, thickness: 1.1pt))
    content(((10.6, 2.8)), [`cryo-agent send`], fill: white, frame: "rect", stroke: none, padding: 0.08)

    // 发件箱 → 人类
    line("outbox.west", (1.3, 5.8), "human.north",
         mark: arrow, stroke: bst)
    content(((1.5, 5.1)), [`cryo receive`], fill: white, frame: "rect", stroke: none, padding: 0.08)

    // 发件箱 → 渠道（推送）
    line("outbox.west", "ch-out.east",
         mark: arrow, stroke: chst)
    content(("outbox.west", 50%, "ch-out.east"), [推送], fill: white, frame: "rect", stroke: none, padding: 0.08)

    // 渠道 → 人类
    line("ch-out.north", "human.south",
         mark: arrow, stroke: chst)
  }),
  caption: [人类 ↔ 智能体通信流程图。],
)

== 直接消息（本地）

最简单的方式——无需任何外部账号：

#cmd[
  ```
  # 人类发送消息
  cryo send "请为认证模块添加单元测试"

  # 人类读取智能体的回复
  cryo receive
  ```
]

== GitHub Discussions 同步

#cmd[
  ```
  cryo-gh init --repo owner/repo --discussion 42
  cryo-gh sync          # 启动后台同步守护进程
  cryo-gh status        # 查看同步配置
  cryo-gh unsync        # 停止后台同步
  ```
]

GitHub Discussion 中的新评论在每个同步周期被拉取到 `messages/inbox/`。会话摘要则作为新评论推送回去。

== Zulip 同步

#cmd[
  ```
  cryo-zulip init --config ~/.zuliprc --stream "dev"
  cryo-zulip sync       # 启动后台同步守护进程
  cryo-zulip status     # 查看同步状态
  ```
]

在配置的 Zulip 流中发布的消息会出现在收件箱中；智能体的回复则被推送回该流。

// ─────────────────────────────────────────────────────────────────────────────
= 配置参考
// ─────────────────────────────────────────────────────────────────────────────

项目配置保存在 `cryo.toml` 中：

#cmd[
  ```toml
  agent = "opencode run"           # 默认智能体命令
  max_retries = 3                  # 触发备用告警前的崩溃重试次数
  max_session_duration = 1800      # 会话超时时间（秒，0 表示不限制）
  watch_inbox = true               # 有新收件箱消息时通过 inotify 响应式唤醒
  fallback_alert = "outbox"        # "outbox" | "notify" | "none"
  report_interval_hours = 24       # 0 = 禁用周期性报告
  report_time = "09:00"            # 每日报告的时间点

  # 配置多个 Provider 实现自动轮换：
  [[providers]]
  name = "primary"
  env  = { OPENAI_API_KEY = "sk-…" }

  [[providers]]
  name = "fallback"
  env  = { ANTHROPIC_API_KEY = "sk-ant-…" }
  ```
]

#table(
  columns: (auto, auto, 1fr),
  stroke: 0.5pt + luma(180),
  inset: 7pt,
  fill: (col, row) => if row == 0 { ocean.lighten(88%) } else { white },
  [*配置项*], [*默认值*], [*说明*],
  [`agent`], [`opencode run`], [启动智能体子进程的 Shell 命令。],
  [`max_retries`], [`3`], [连续崩溃后放弃并发送备用告警前的重试次数。],
  [`max_session_duration`], [`0`], [每次会话的最大秒数。`0` 表示不限制。],
  [`watch_inbox`], [`true`], [使用文件系统事件，在新收件箱消息到达时立即唤醒守护进程。],
  [`fallback_alert`], [`"outbox"`], [重试耗尽时的处理方式：写入发件箱、发送桌面通知或不做任何操作。],
  [`report_interval_hours`], [`0`], [周期性会话统计桌面通知的间隔。`0` 表示禁用。],
  [`report_time`], [`"09:00"`], [每日报告对齐的时钟时间。],
  [`providers`], [`[]`], [AI Provider 配置列表，失败时轮换。每条记录可设置环境变量。],
)

// ─────────────────────────────────────────────────────────────────────────────
= 运行时文件参考
// ─────────────────────────────────────────────────────────────────────────────

#table(
  columns: (auto, 1fr),
  stroke: 0.5pt + luma(180),
  inset: 7pt,
  fill: (col, row) => if row == 0 { sage.lighten(70%) } else { white },
  [*文件 / 目录*], [*用途*],
  [`cryo.toml`], [项目配置。手动编辑。由 `cryo init` 创建。],
  [`plan.md`], [每次智能体会话中注入的任务描述。],
  [`CLAUDE.md` / `AGENTS.md`], [面向智能体的 Cryochamber 协议（由 `cryo init` 自动生成）。],
  [`timer.json`], [运行时状态：会话编号、PID 锁、重试计数、CLI 覆盖值。由守护进程写入。],
  [`cryo.log`], [仅追加的结构化事件日志。可通过 `cryo log` / `cryo watch` 读取。],
  [`cryo-agent.log`], [智能体子进程的原始标准输出/标准错误。],
  [`todo.json`], [待处理的待办事项。驱动唤醒调度。],
  [`messages/inbox/`], [智能体的收件消息。由 `cryo send` 和渠道同步写入。],
  [`messages/outbox/`], [智能体的回复。由 `cryo receive` 读取，并由渠道同步推送。],
  [`messages/inbox/archive/`], [已处理（已读）收件箱消息的归档。],
  [`.cryo/cryo.sock`], [智能体 ↔ 守护进程 IPC 的 Unix 域套接字（仅 Unix 系统）。],
  [`gh-sync.json`], [GitHub Discussion 同步状态（最后游标、已推送会话）。由 `cryo-gh init` 创建。],
  [`zulip-sync.json`], [Zulip 同步状态（最后消息 ID、已推送会话）。由 `cryo-zulip init` 创建。],
)

// ─────────────────────────────────────────────────────────────────────────────
= 完整示例演练
// ─────────────────────────────────────────────────────────────────────────────

以下追踪一个长期编码任务的完整一个轮次。

#callout(color: ocean, icon: "1", title: "操作者初始化项目")[
  #cmd[
    ```
    mkdir chess-engine && cd chess-engine
    cat > plan.md << 'EOF'
    # 象棋引擎
    用 Rust 实现一个兼容 UCI 协议的象棋引擎。
    本次会话聚焦于 alpha-beta 剪枝。
    EOF
    cryo init --agent "opencode run"
    cryo start
    ```
  ]
  守护进程启动；智能体立即唤醒（会话 1）。
]

#callout(color: teal, icon: "2", title: "智能体工作（会话 1）")[
  #cmd[
    ```
    # 在智能体进程内部 —— 由 LLM 发出的 cryo-agent 命令
    cryo-agent todo add "实现走法生成" --at 2026-03-07T09:00
    cryo-agent note "棋盘表示已完成。下一步：走法生成。"
    cryo-agent hibernate --summary "Board 结构体已完成。已安排明天的走法生成任务。"
    ```
  ]
  守护进程记录 `--- CRYO END ---`，读取 TODO，并休眠至 09:00。
]

#callout(color: amber, icon: "3", title: "人类在休眠期间发送消息")[
  #cmd[
    ```
    cryo send "另外请加上 perft 测试方便调试"
    ```
  ]
  守护进程的收件箱监听器触发，提前唤醒智能体（会话 2 开始）。
]

#callout(color: sage, icon: "4", title: "操作者检查进度")[
  #cmd[
    ```
    cryo status
    # → 会话：3 | 下次唤醒：2026-03-08T09:00 | 智能体：opencode run
    cryo receive
    # → 智能体消息："走法生成已完成。Perft(5) = 4865609 ✓"
    ```
  ]
]

// ─────────────────────────────────────────────────────────────────────────────
= Provider 轮换
// ─────────────────────────────────────────────────────────────────────────────

当 `cryo.toml` 中定义了多个 `[[providers]]` 时，守护进程会在连续失败后轮换使用它们。适用场景包括：

- 速率限制容错（轮换到不同的 API Key 或模型）
- 多模型实验（失败时从 GPT-4 切换到 Claude）
- 预算控制（优先用低成本模型，失败后切到高成本模型）

#figure(
  cetz.canvas(length: 1cm, {
    import cetz.draw: *
    let st = (paint: ocean, thickness: 1.1pt)
    let arrow = (end: "stealth")

    let providers = ("Provider A", "Provider B", "Provider C")
    for (i, name) in providers.enumerate() {
      let x = float(i) * 4.0
      rect((x, 0), (x + 3.2, 1.2),
           fill: ocean.lighten(82%), stroke: st, name: "p" + str(i))
      content("p" + str(i) + ".center", align(center)[#name])
    }

    // Provider 之间的箭头
    line("p0.east", "p1.west", mark: arrow, stroke: (paint: rose, thickness: 1.2pt))
    content(("p0.east", 50%, "p1.west"), [失败], fill: white, frame: "rect", stroke: none, padding: 0.06)

    line("p1.east", "p2.west", mark: arrow, stroke: (paint: rose, thickness: 1.2pt))
    content(("p1.east", 50%, "p2.west"), [失败], fill: white, frame: "rect", stroke: none, padding: 0.06)

    // 回绕
    bezier("p2.south", "p0.south",
           (11.2, -1.4), (0, -1.4),
           mark: arrow, stroke: (paint: rose, thickness: 1pt, dash: "dashed"))
    content((5.6, -1.8), [回绕（全部耗尽 → 触发备用告警）],
            fill: white, frame: "rect", stroke: none, padding: 0.08)
  }),
  caption: [连续失败时的 Provider 轮换顺序。],
)

// ─────────────────────────────────────────────────────────────────────────────
= 开发者指南
// ─────────────────────────────────────────────────────────────────────────────

#callout(color: slate, icon: "🔧", title: "开发工作流")[
  #cmd[
    ```
    # 运行所有测试
    cargo test

    # 代码检查（CI 中警告即错误）
    cargo clippy --all-targets -- -D warnings

    # 测量覆盖率
    make coverage

    # 端到端运行内置的 mr-lazy 示例
    make example DIR=examples/mr-lazy
    make example-cancel DIR=examples/mr-lazy
    ```
  ]
]

贡献者的关键入口点：

#table(
  columns: (auto, 1fr),
  stroke: 0.5pt + luma(180),
  inset: 7pt,
  fill: (col, row) => if row == 0 { slate.lighten(72%) } else { white },
  [*文件*], [*如果你想……请从这里开始*],
  [`src/daemon.rs`], [理解或修改会话事件循环、重试逻辑、唤醒检测。],
  [`src/bin/cryo.rs`], [添加或修改面向操作者的 CLI 命令。],
  [`src/bin/cryo_agent.rs`], [添加或修改面向智能体的 IPC 命令。],
  [`src/platform/`], [移植到新操作系统或修复平台特定行为。],
  [`src/channel/zulip.rs`], [理解 Zulip HTTP 客户端或添加新的消息渠道。],
  [`src/agent.rs`], [修改智能体提示词的构建方式或子进程的启动方式。],
  [`templates/`], [编辑 `cryo init` 注入的默认协议、计划或配置模板。],
  [`tests/`], [添加集成测试。CLI 测试使用 `assert_cmd`；HTTP 测试使用内联最小化 mock 服务器。],
)

#v(1cm)
#line(length: 100%, stroke: 0.5pt + luma(200))
#v(0.3cm)
#align(center)[
  #text(size: 9pt, fill: slate)[
    Cryochamber 采用 MIT 许可证。源码：#link("https://github.com/GiggleLiu/cryochamber") \
    文档：#link("https://giggleliu.github.io/cryochamber")
  ]
]

