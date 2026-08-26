# Cryochamber

**Cryochamber 是 AI 智能体（Claude、OpenCode、Codex、Pi、Kimi Code）的休眠舱。** 它在两次会话之间让智能体休眠，并在合适的时机唤醒它——而不是按照固定的时间表。智能体阅读自己的计划，完成任务，然后自行决定下一次何时唤醒。这让 AI 智能体可以运行跨越数天、数周甚至数年的任务，就像星际旅行者在冷冻休眠中一样。

![智能体制定计划、休眠、在合适的时间被唤醒，并选择下一次唤醒时机](images/cryochamber-concept.jpg)

## 为什么不用 cron？

Cron 按固定时间表唤醒，不管有没有事情要做。Cryochamber 把调度决定交给智能体：

- **省 token。** 由 cron 驱动的智能体每次 tick 都会烧掉一整次会话，即使什么都没变化。Cryochamber 的智能体会一直休眠，直到有理由唤醒——它自己安排的 TODO，或收件箱里的一条消息。
- **省心。** 使用 cron 时，人类必须预先猜对时间表：太快浪费钱，太慢错过事情。在这里，智能体根据实际情况推理——推迟的截止日期、等待作者推送修复的评审、国际象棋对手的节奏——然后选择自己的下一次唤醒时间。
- **应对紧急情况。** 当有事情需要关注时，智能体可以把唤醒安排在几分钟后，而收件箱消息可以立即唤醒它。Cron 在关键时刻无法加速。

## 两分钟上手

> **平台支持：** 仅支持 macOS 和 Linux。

```bash
cargo install cryochamber
mkdir my-chamber && cd my-chamber
cryo init          # 生成 plan.md 和 cryo.toml（或让 make-plan 技能引导你）
cryo start         # 启动守护进程，安装为操作系统服务
cryohub start      # 在浏览器中打开打印出的仪表盘 URL
```

然后编辑 `plan.md` 来描述智能体的目标和任务。可运行的示例 chamber（`mr-lazy`、`chess-by-mail` 等）位于 GitHub 上的 [`examples/chambers/`](https://github.com/GiggleLiu/cryochamber/tree/main/examples/chambers)。

## 亲眼看看它如何工作

```bash
cryohub start    # 打印本地仪表盘 URL——在浏览器中打开
```

![Agent Console 显示 chamber 的对话，包含智能体报告、表格和图像](images/agent-console.png)

[Cryohub](./reference/cli.md#hub-cryohub) 提供 **[Agent Console](./agent-console.md)**：每个 chamber 一条平铺的对话，chamber 状态、TODO、笔记、日志尾部与生命周期控制都在一步之遥，手机和桌面浏览器都能用。它内嵌在 `cryohub` 二进制中——无需安装。可以通过邀请链接把单个 chamber 分享给他人，或用 [`cryo-zulip`](./reference/cli.md#zulip-sync-cryo-zulip) 把它桥接到 Zulip。

## Chamber 的保证

- **每次唤醒都会产生一条可见消息。** 如果智能体未回复就退出，守护进程会写入一条兜底消息——会话绝不会静默。
- **每条收件箱消息都会得到回复。** 即使智能体会话中途崩溃，发送方仍会收到回复。
- **每个 TODO 都会被兑现。** 失败的会话会被重新安排为带指数退避的可见重试。
- **任何内容都不会被消费两次。** 已被领取的消息和 TODO 绝不会悄悄回到待处理状态。

## 下一步

- [快速上手](./getting-started.md)——从零到一个运行中的 hub：安装 Pi、建一个照看主机的第一个 chamber、分享给朋友。
- [工作原理](./how-it-works.md)——五分钟讲解：chamber 文件和会话循环。
- [Agent Console](./agent-console.md)——网页与手机界面：登录、邀请、公网部署。
- [CLI 参考](./reference/cli.md)——所有 `cryo`、`cryohub`、`cryo-agent` 和 `cryo-zulip` 命令。
- [配置](./reference/configuration.md)——所有 `cryo.toml` 和 `cryohub.toml` 字段。
