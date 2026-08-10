# 配置

每个 chamber 都通过其目录下的 `cryo.toml` 文件进行配置。`cryo init` 会创建一个带合理默认值的配置。

## `cryo.toml`

```toml
# cryo.toml — cryochamber 项目配置
agent = "opencode"               # 智能体命令 (opencode, claude, codex, pi, kimi, ...)
max_session_duration = 3600      # 会话超时秒数 (0 = 不超时)
watch_dirs = ["messages/inbox"]  # 监听以实现响应式唤醒的目录 ([] 表示禁用)
zulip_poll_interval = 5          # Zulip 同步轮询间隔（秒）
# reply_window = 600             # 保持会话 10 分钟，用于承接后续消息（不设置 = 300；0 表示禁用）

# 注入到每个智能体会话的 Provider 环境变量（可选）。
[provider]
name = "anthropic"               # 显示名称，在 `cryo status` 中展示
env = { ANTHROPIC_API_KEY = "sk-ant-..." }  # 派生智能体时设置的环境变量
```

| 字段 | 默认值 | 说明 |
|-------|---------|-------------|
| `agent` | `"opencode"` | 要运行的智能体命令。使用 `"claude"` 表示 Claude Code，`"codex"` 表示 Codex，`"pi"` 表示 Pi，`"kimi"` 表示 Kimi Code，或 `PATH` 上的任何可执行文件。 |
| `max_session_duration` | `3600` | 会话超时秒数。`0` 表示禁用超时。 |
| `watch_dirs` | `["messages/inbox"]` | 守护进程监听新文件的目录列表，用于响应式唤醒智能体。路径相对于 chamber 目录解释，除非是绝对路径。设为 `[]` 可完全禁用响应式唤醒。 |
| `zulip_poll_interval` | `5` | `cryo-zulip sync` 轮询 Zulip 的间隔（秒）。`cryo-zulip sync --interval N` 可单次覆盖它。 |
| `reply_window` | `300` | 可选。回复窗口秒数。智能体休眠后，守护进程会把会话驻留这么久；在窗口内到达的消息会被退回同一个会话（同一 LLM 上下文）处理，而不是派生一个全新会话。TODO 到期或窗口结束则放行休眠。不设置该字段时为 300（5 分钟）；显式设为 `0` 则禁用窗口，休眠一经放行会话立即结束。上限为 `86400`。与该窗口无关，只要收件箱有未读邮件，休眠一律被拒绝。 |

> **`reply_window` 与 `max_session_duration`**：休眠被驻留期间，会话时长计时是暂停的；每一条把休眠退回会话的消息还会重新发放一轮工作时间预算。因此一个较长的窗口可能让同一个会话远远超过 `max_session_duration` 才结束——真正结束它的是窗口到期或 TODO 到期。除非你确实想要长时间的对话式会话，否则请把窗口设得短一些（分钟级，而非小时级）。

## `[provider]`

Cryochamber 只支持一个活动的 provider 配置。`[provider]` 表携带一个显示用 `name` 和一个 `env` 映射，包含注入到每个派生智能体会话的环境变量——智能体模型的 API 密钥就放在这里。

```toml
[provider]
name = "anthropic"
env = { ANTHROPIC_API_KEY = "sk-ant-...", OPENCODE_MODEL = "claude-sonnet-4-20250514" }
```

配置好之后，`cryo status` 会显示 provider 名称。

> **安全提示**：`[provider].env` 下的值是机密。`cryo init` 会写入一个忽略 `.cryo/` 的 chamber `.gitignore`，但 `cryo.toml` 本身**不会**被 gitignore——如果你提交或推送这个 chamber，请让 API 密钥远离版本控制。要么把 `cryo.toml` 加入你自己的 `.gitignore`，要么不把密钥写进 `cryo.toml`，而是在 `cryo start` 之前把它们导出到环境变量中。

### 遗留 `[[providers]]`（已弃用）

旧配置使用 `[[providers]]` 数组。出于向后兼容它仍会被接受，但只会使用第一项——provider 轮换已被移除。加载使用 `[[providers]]` 的配置会打印弃用警告，下一次 `save` 会把它重写为规范的单一 `[provider]` 形式。请迁移到 `[provider]`。

见下文 [`cryohub.toml`](#cryohubtoml)。

## `cryohub.toml`

Cryohub 设置位于 `$XDG_CONFIG_HOME/cryo/cryohub.toml`；若未设置 `XDG_CONFIG_HOME`，则位于 `~/.config/cryo/cryohub.toml`。默认的本地仪表盘 URL 是 `http://127.0.0.1:8765`。仪表盘的「新建 Chamber」按钮会在配置的 `chamber_root` 下创建 chamber，默认是 `~/.cryo/chambers`。

```toml
host = "127.0.0.1"
port = 8765
chamber_root = "/Users/alice/.cryo/chambers"
```

对于项目自有的 chamber 集合，把 `chamber_root` 设为项目路径，例如 `/path/to/project/.cryo/chambers`。

| 字段 | 默认值 | 说明 |
|-------|---------|-------------|
| `host` | `"127.0.0.1"` | 全局仪表盘服务的绑定地址。 |
| `port` | `8765` | 全局仪表盘服务的 TCP 端口。 |
| `chamber_root` | `~/.cryo/chambers` | 从仪表盘 UI 创建的 chamber 的默认位置。 |

## 从命令行覆盖配置

传给 `cryo start` 的旗标会覆盖本次会话的 `cryo.toml`。这些覆盖项存储在 `timer.json`（运行时状态）中，不会修改 `cryo.toml`。

```bash
cryo start --agent claude
cryo start --max-session-duration 3600
```

## 配置与状态

| 文件 | 用途 | 跨运行保留 |
|------|---------|----------------------|
| `cryo.toml` | 项目配置。提交进 git。 | 是 |
| `timer.json` | 运行时状态：会话编号、PID 锁、CLI 覆盖项。 | 否 |
