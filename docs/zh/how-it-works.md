# 工作原理

五分钟了解 chamber 的各个部件。

## Chamber 就是一个目录

`cryo init` 会创建三个文件：

- **`plan.md`** —— 智能体的使命：目标、任务和规则。智能体在每次会话开始时都会重新阅读它。
- **`cryo.toml`** —— chamber 配置：运行哪个智能体命令、会话超时、收件箱监听。参见 [配置](./reference/configuration.md)。
- **`NOTES.md`** —— 智能体跨会话的记忆。它直接读取并向该文件追加内容。

守护进程运行时，旁边会出现运行时状态：日志、`todo.json`，以及 `messages/inbox/` 和 `messages/outbox/`。

## 计划就是纯 Markdown

一个监听 GitHub 仓库新版本的 chamber：

```markdown
# Release watcher

## Goal
Tell me when acme/widgets publishes a new release.

## Tasks
1. Run `gh release list --repo acme/widgets --limit 1` and compare
   the version with the one recorded in NOTES.md.
2. If it changed: send me the release notes with `cryo-agent send`,
   then record the new version in NOTES.md.
3. Schedule the next check with `cryo-agent todo add` — every 2 hours
   on weekdays, once a day on weekends.
4. Hibernate with `cryo-agent hibernate --summary "..."`.
```

不需要代码，也不需要 cron 表达式——智能体自己判断情况（这里是工作日还是周末）并决定下一次唤醒时机。

## 会话循环

```text
daemon wakes agent        <- earliest TODO due, or inbox message
    │
    v
agent reads plan.md + NOTES.md
    │
    v
does the work
    │
    v
cryo-agent send "..."                    <- a visible message, never silent
    │
    v
cryo-agent todo add "..." --at <when>    <- declares the next wake
    │
    v
cryo-agent hibernate                     <- daemon sleeps until that wake
    │
    └────────────── back to the top ──────────────┘
```

一次唤醒、一次智能体运行、再回到休眠——这就是一个**会话**：

1. 当最早到期（due）的 TODO 到来时，守护进程唤醒智能体；若有收件箱消息到达，则立即唤醒。
2. 智能体阅读 `plan.md` 和 `NOTES.md`，然后开始干活。
3. 它用 `cryo-agent send` 至少发送一条可见消息。如果它在发送前退出，守护进程会写入一条兜底消息——会话绝不会静默。
4. 它用 `cryo-agent todo add "..." --at <time>` 声明自己的下一次唤醒。守护进程的下一次唤醒永远是「最早到期的 TODO」——没有 TODO，就不唤醒。
5. 它调用 `cryo-agent hibernate` 并退出。守护进程一直休眠到下一个触发时机。`hibernate --complete` 则彻底结束计划。

## 与智能体对话

从终端（`cryo send "..."`）或 Cryohub 仪表盘发送消息。消息会落入 `messages/inbox/`，并在默认的 `watch_dirs` 配置下立即唤醒智能体。智能体的回复会出现在 `messages/outbox/` 以及仪表盘的消息历史中。

## 下一步

- [CLI 参考](./reference/cli.md)——所有 `cryo`、`cryohub`、`cryo-agent` 和 `cryo-zulip` 命令。
- [配置](./reference/configuration.md)——所有 `cryo.toml` 和 `cryohub.toml` 字段。
