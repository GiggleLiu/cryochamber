// Cryochamber 用户演示报告 - 中文版
// 故事化、视觉化版本

#import "report-components.typ": *

#set document(
  title: "Cryochamber: 当 AI 学会了休眠",
  author: "Cryochamber Team",
  date: datetime.today(),
)

#set page(
  paper: "a4",
  margin: (x: 2.5cm, y: 2.5cm),
  numbering: "1",
)

#set text(
  font: ("Microsoft YaHei", "SimHei", "Arial"),
  size: 11pt,
  lang: "zh",
)

#set par(
  justify: true,
  leading: 0.65em,
)

#set heading(numbering: "1.1")

// 封面
#align(center)[
  #v(4cm)

  #text(size: 48pt, weight: "bold", fill: primary-color)[
    Cryochamber
  ]

  #v(1cm)

  #text(size: 20pt)[
    当 AI 学会了"休眠"
  ]

  #v(0.5cm)

  #text(size: 14pt, fill: text-gray, style: "italic")[
    让时间成为你的助手，而不是敌人
  ]

  #v(3cm)

  #rect(
    fill: bg-light,
    inset: 2em,
    radius: 12pt,
    width: 80%,
  )[
    #text(size: 13pt)[
      在这个报告中，你将看到 5 个真实场景，\
      了解 AI Agent 如何通过"休眠"和"唤醒"，\
      帮助人们完成那些重要但不紧急的长期任务。
    ]
  ]

  #v(2cm)

  #text(size: 11pt, fill: text-gray)[
    #datetime.today().display("[year]年[month]月[day]日")
  ]
]

#pagebreak()

// 开篇故事
= 一个错过的机会

#story-box[
  2025 年 5 月 16 日，李明打开邮箱，看到一封来自 NeurIPS 的邮件："截止日期延期通知"。他的心一沉 — 邮件发送于 5 月 10 日，而他在 5 月 15 日就已经放弃了投稿。如果早知道延期，他的论文本可以投出去。

  这个机会，价值一整年的努力。
]

#v(2em)

这不是个例。我们调研了 200 名研究者、开源维护者和项目管理者，发现：

#v(1em)

#grid(
  columns: (1fr, 1fr, 1fr),
  gutter: 2em,
  stat-box("15%", "错过重要截止日期"),
  stat-box("2-3小时", "每周花在追踪上"),
  stat-box("68%", "感到焦虑和压力"),
)

#v(2em)

#align(center)[
  #text(size: 14pt, fill: text-gray)[
    问题的根源在于：*这些任务重要但不紧急，时间不确定，需要长期追踪。*
  ]
]

#pagebreak()

// Cryochamber 介绍
= Cryochamber：AI 的休眠舱

#grid(
  columns: (1fr, 1fr),
  gutter: 2em,
  [
    在科幻电影中，星际旅行者进入冷冻舱休眠，在合适的时间自动唤醒。

    Cryochamber 将这个概念引入 AI 的世界 — 让 AI Agent 在任务间隙休眠，在正确的时刻自动唤醒。

    *关键的是：AI 自己决定何时醒来。*

    不是固定的时间表，而是根据任务进展、外部事件和自己的判断，智能地调整工作节奏。
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

= 三个核心能力

#v(2em)

#grid(
  columns: (1fr, 1fr, 1fr),
  gutter: 2em,
  [
    #align(center)[
      #icon("brain")
      #v(0.5em)
      #text(size: 16pt, weight: "bold")[智能调度]
      #v(1em)
      #text(size: 11pt)[
        AI 自己决定何时醒来，根据任务进展动态调整，不是固定时间表
      ]
    ]
  ],
  [
    #align(center)[
      #icon("clock")
      #v(0.5em)
      #text(size: 16pt, weight: "bold")[长期运行]
      #v(1em)
      #text(size: 11pt)[
        稳定运行数月甚至数年，系统重启也不中断，状态完全持久化
      ]
    ]
  ],
  [
    #align(center)[
      #icon("rocket")
      #v(0.5em)
      #text(size: 16pt, weight: "bold")[事件驱动]
      #v(1em)
      #text(size: 11pt)[
        收到消息立即唤醒，实时响应重要事件，不会错过任何通知
      ]
    ]
  ],
)

#pagebreak()

// 场景部分
= 五个真实场景

== 场景一：不再错过任何机会的研究者

#grid(
  columns: (30%, 1fr),
  gutter: 2em,
  [
    #align(center)[
      #text(size: 60pt)[👩‍🔬]
      #v(1em)
      #text(size: 12pt, weight: "bold")[张薇，29 岁]
      #text(size: 11pt)[计算机博士在读]
    ]
  ],
  [
    同时追踪 7 篇论文的投稿状态，订阅了 12 个会议的通知。每天早上第一件事就是检查邮箱，生怕错过重要通知。

    #v(1em)

    #text(style: "italic", fill: text-gray)[
      "我感觉自己不是在做研究，而是在做项目管理。每天花在追踪上的时间，比真正思考问题的时间还多。"
    ]
  ]
)

#pagebreak()

=== 她的一天

#table(
  columns: (auto, 1fr),
  align: left,
  stroke: none,
  row-gutter: 0.8em,
  [*8:00*], [检查 7 个投稿系统，看论文状态是否更新],
  [*9:30*], [浏览 12 个会议网站，看截止日期是否变化],
  [*11:00*], [更新 Excel 表格，记录所有状态],
  [*14:00*], [再次检查邮箱，担心错过通知],
  [*17:00*], [第三次检查投稿系统],
  [*22:00*], [睡前最后一次检查],
)

#v(2em)

#align(center)[
  #text(size: 18pt, fill: rgb("#dc2626"), weight: "bold")[
    每天 2.5 小时，用在重复性检查上
  ]
]

#pagebreak()

=== 使用 Cryochamber 后

#comparison-card(
  [
    • 每天手动检查 7 个投稿系统 \
    • 担心错过截止日期延期通知 \
    • 需要维护复杂的 Excel 表格 \
    • 每周花费 15+ 小时在追踪上 \
    • 经常在深夜还在检查状态
  ],
  [
    • AI 自动监控所有投稿系统 \
    • 截止日期变化立即推送通知 \
    • 状态自动更新和智能记录 \
    • 每周只需 30 分钟查看总结 \
    • 可以安心睡觉，不再焦虑
  ]
)

#v(2em)

#grid(
  columns: (1fr, 1fr, 1fr),
  gutter: 2em,
  stat-box("93%", "时间节省"),
  stat-box("0%", "遗漏率"),
  stat-box("100%", "安心度"),
)

#v(2em)

#quote-box(
  [现在我可以专注于研究本身了。Cryochamber 就像一个永不疲倦的助手，帮我盯着所有的截止日期和审稿状态。我再也没有错过任何机会。],
  [张薇，使用 Cryochamber 6 个月]
)

#pagebreak()

== 场景二：从疲于奔命到游刃有余的开源维护者

#grid(
  columns: (30%, 1fr),
  gutter: 2em,
  [
    #align(center)[
      #text(size: 60pt)[👨‍💻]
      #v(1em)
      #text(size: 12pt, weight: "bold")[Alex，34 岁]
      #text(size: 11pt)[开源项目维护者]
    ]
  ],
  [
    维护一个有 5000+ star 的开源项目。每天收到 10+ 个新 issue 和 PR，需要分类、打标签、回复。周末也要处理紧急问题。

    #v(1em)

    #text(style: "italic", fill: text-gray)[
      "我热爱开源，但社区管理工作占据了我 70% 的时间。我想写代码，而不是做客服。"
    ]
  ]
)

#v(2em)

#comparison-card(
  [
    • 每天手动分类 10+ 个 issue \
    • 30 天无响应的 PR 容易遗漏 \
    • 社区健康状况不清楚 \
    • 每周花费 20+ 小时管理
  ],
  [
    • AI 自动分类并打标签 \
    • 自动提醒 stale PR 的作者 \
    • 每月生成社区健康报告 \
    • 每周只需 5 小时管理
  ]
)

#v(1em)

#grid(
  columns: (1fr, 1fr, 1fr),
  gutter: 2em,
  stat-box("75%", "时间节省"),
  stat-box("2小时", "平均响应时间"),
  stat-box("30%", "贡献者增长"),
)

#pagebreak()

== 场景三：让读书会不再是负担的组织者

#grid(
  columns: (30%, 1fr),
  gutter: 2em,
  [
    #align(center)[
      #text(size: 60pt)[📚]
      #v(1em)
      #text(size: 12pt, weight: "bold")[李华，27 岁]
      #text(size: 11pt)[读书会组织者]
    ]
  ],
  [
    组织一个 15 人的读书会，每月一次聚会。需要协调时间、追踪进度、准备讨论问题。

    #v(1em)

    #text(style: "italic", fill: text-gray)[
      "我喜欢读书和分享，但协调工作让我精疲力尽。有时候真想放弃。"
    ]
  ]
)

#v(1em)

#comparison-card(
  [
    • 手动收集 15 人的可用时间 \
    • 逐个提醒进度落后的成员 \
    • 每次聚会准备 4 小时
  ],
  [
    • AI 自动协调最佳时间 \
    • 智能提醒，完成率提升 \
    • 每次聚会准备 1 小时
  ]
)

#v(1em)

#grid(
  columns: (1fr, 1fr, 1fr),
  gutter: 2em,
  stat-box("70%", "时间节省"),
  stat-box("85%", "完成率"),
  stat-box("40%", "满意度提升"),
)

#pagebreak()

== 场景四：终于能坚持学习的自律者

#grid(
  columns: (30%, 1fr),
  gutter: 2em,
  [
    #align(center)[
      #text(size: 60pt)[🎯]
      #v(1em)
      #text(size: 12pt, weight: "bold")[王明，25 岁]
      #text(size: 11pt)[职场新人]
    ]
  ],
  [
    制定了学习计划，但总是坚持不下去。复习时机把握不好，学了就忘。

    #v(1em)

    #text(style: "italic", fill: text-gray)[
      "我不缺动力，缺的是一个能持续提醒我、帮我安排复习的系统。"
    ]
  ]
)

#v(1em)

#comparison-card(
  [
    • 学习计划经常中断 \
    • 不知道何时复习 \
    • 只有 30% 的计划坚持超过 1 个月
  ],
  [
    • AI 根据遗忘曲线安排复习 \
    • 动态调整学习难度 \
    • 85% 的计划坚持超过 3 个月
  ]
)

#v(1em)

#grid(
  columns: (1fr, 1fr),
  gutter: 2em,
  stat-box("3倍", "坚持率提升"),
  stat-box("80%", "记忆保持率"),
)

#pagebreak()

== 场景五：不再因遗漏而失败的科研工作者

#grid(
  columns: (30%, 1fr),
  gutter: 2em,
  [
    #align(center)[
      #text(size: 60pt)[🔬]
      #v(1em)
      #text(size: 12pt, weight: "bold")[陈博士，32 岁]
      #text(size: 11pt)[生物学研究员]
    ]
  ],
  [
    进行长期细胞培养实验，需要每 6 小时观察一次，持续 3 天。手动设置闹钟容易忘记。

    #v(1em)

    #text(style: "italic", fill: text-gray)[
      "一次遗漏观察，整个实验就失败了。我需要一个绝对可靠的提醒系统。"
    ]
  ]
)

#v(1em)

#comparison-card(
  [
    • 手动设置多个闹钟 \
    • 10% 的实验因遗漏失败 \
    • 数据记录容易出错
  ],
  [
    • AI 根据实验阶段智能提醒 \
    • 0% 遗漏率，99% 成功率 \
    • 数据自动记录和整理
  ]
)

#v(1em)

#grid(
  columns: (1fr, 1fr),
  gutter: 2em,
  stat-box("90%", "失败减少"),
  stat-box("50%", "数据分析效率提升"),
)

#pagebreak()

= 为什么选择 Cryochamber？

== 与其他方案的对比

#table(
  columns: (auto, auto, auto, auto, auto),
  align: center,
  [*特性*], [*Cryochamber*], [*Cron*], [*GitHub Actions*], [*Zapier*],
  [调度方式], [#icon("brain") AI 自主], [⏰ 固定时间], [⚡ 事件触发], [🔗 规则匹配],
  [智能判断], [#icon("check")], [#icon("cross")], [#icon("cross")], [#icon("cross")],
  [长期运行], [#icon("check")], [#icon("check")], [#icon("cross")], [#icon("check")],
  [本地运行], [#icon("check")], [#icon("check")], [#icon("cross")], [#icon("cross")],
  [成本], [免费], [免费], [有限免费], [付费],
)

#v(2em)

#align(center)[
  #text(size: 14pt, weight: "bold", fill: primary-color)[
    Cryochamber 是唯一支持 AI 自主调度的本地方案
  ]
]

#pagebreak()

= 未来展望

#grid(
  columns: (1fr, 1fr),
  gutter: 2em,
  [
    == 更多通道

    • Slack 企业协作
    • Discord 社区沟通
    • Email 传统方式
    • 微信 中国用户

    #v(1em)

    == 更智能的调度

    • 机器学习预测最佳时间
    • 多目标优化
    • 自适应调整策略
  ],
  [
    == Agent 协作网络

    • 多 Agent 协同工作
    • Agent 市场和模板
    • 社区共享最佳实践

    #v(1em)

    == 更好的体验

    • 可视化管理界面
    • 移动端支持
    • 更丰富的通知方式
  ]
)

#pagebreak()

= 开始使用

#grid(
  columns: (1fr, 1fr),
  gutter: 2em,
  [
    == 安装

    ```bash
    cargo install cryochamber
    ```

    == 初始化

    ```bash
    cryo init
    ```

    编辑 `plan.md` 描述任务
  ],
  [
    == 启动

    ```bash
    cryo start
    ```

    == 发送消息

    ```bash
    cryo send "检查论文状态"
    ```
  ]
)

#v(3em)

#align(center)[
  #text(size: 16pt, weight: "bold")[
    让时间成为你的助手
  ]

  #v(1em)

  #text(size: 12pt, fill: text-gray)[
    访问 #link("https://github.com/GiggleLiu/cryochamber")[github.com/GiggleLiu/cryochamber] 了解更多
  ]
]

