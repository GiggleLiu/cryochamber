# 配置

每个 chamber 都通过其目录下的 `cryo.toml` 文件进行配置。`cryo init` 会创建一个带合理默认值的配置。

## `cryo.toml`

```toml
# cryo.toml — cryochamber 项目配置
agent = "opencode"               # 智能体命令 (opencode, claude, codex, pi, kimi, ...)
max_session_duration = 3600      # 会话超时秒数 (0 = 不超时)
watch_dirs = ["messages/inbox"]  # 监听以实现响应式唤醒的目录 ([] 表示禁用)
zulip_poll_interval = 5          # Zulip 同步轮询间隔（秒）

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

> **回复窗口不在此处配置。**休眠成功后会话为后续消息保持打开的时长由智能体在每次休眠时自行选择：`cryo-agent hibernate --linger <秒数>`（省略 = 300 秒，上限 86400；`0` 表示立即入睡）。休眠驻留期间会话计时暂停，且每一轮后续消息都会重新发放工作预算，因此较长的 linger 可能让同一会话远远超过 `max_session_duration`。

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
public = false
owner_name = "human"
public_hosts = []
# console_dir = "/absolute/path/to/console/dist"   # 可选覆盖，见下文
```

对于项目自有的 chamber 集合，把 `chamber_root` 设为项目路径，例如 `/path/to/project/.cryo/chambers`。

未知的键会被拒绝：像 `console-dir` 这样的拼写错误会让 `cryohub start` 报错并指出该键，而不是被静默忽略。

| 字段 | 默认值 | 说明 |
|-------|---------|-------------|
| `host` | `"127.0.0.1"` | 全局仪表盘服务的绑定地址。 |
| `port` | `8765` | 全局仪表盘服务的 TCP 端口。 |
| `chamber_root` | `~/.cryo/chambers` | 从仪表盘 UI 创建的 chamber 的默认位置。 |
| `public` | `true` | 是否对每个 `/api` 路由强制 bearer token 鉴权。默认开启；在此默认之前写下的、缺少该键的配置文件同样按 `true` 加载，而显式的 `public = false` 会保持开放模式。只有 `cryohub start --no-public` 会清除——普通的 `cryohub start` 保持这里保存的值。 |
| `owner_name` | `"human"` | 公开模式下 owner 发送的消息所标记的发送者名字。客户端提供的 `from` 会被忽略。 |
| `public_hosts` | `[]` | 在回环地址和 `host` 之外额外接受的 `Host` 头值。当反向代理转发公网主机名时需要设置。 |
| `console_dir` | *（未设置——使用内嵌版本）* | 从此目录提供 [Agent Console](../agent-console.md)，而不是使用 `cryohub` 二进制中内嵌的构建。必须是指向 vite `dist/` 的绝对路径。仅用于开发和自定义构建。 |

### 提供 Agent Console

[Agent Console](../agent-console.md) 是 hub 的网页界面——没有其他仪表盘——并且它**内嵌在二进制中**：`cryohub start` 无需任何配置即可提供它。

hub 对 `/` 和任何客户端路由返回控制台的 `index.html`，从 `/assets/` 提供带不可变缓存的哈希资源，并保持 `/api` 不受影响。控制台来源之外的任何内容都不可访问——`../` 路径或指向覆盖目录之外的符号链接都会得到 404。即使在 `--public` 下，控制台自身的页面也不需要鉴权，因为它们就是登录页；每个 `/api` 路由都在 bearer token 之后。

只有在需要提供不同构建时才设置 `console_dir`（`make console-build` 会生成 `console/dist/`）。它必须是**绝对路径**：hub 会基于服务进程的工作目录规范化它，而工作目录由 launchd/systemd 决定。`cryohub status` 会报告当前生效的来源。如果覆盖目录中没有 `index.html`——或者二进制在构建时没有内嵌控制台且没有覆盖——hub 会对页面返回一个简短的设置页（HTTP 503），而不是空白的 404；API 全程正常工作。

### 反向代理之后

hub 会拒绝 `Host` 头既不是回环地址也不是已配置主机的请求——这正是阻止恶意页面通过 DNS 重绑定操控回环服务的机制。保留公网主机名的代理（Caddy 的默认行为）因此需要放行该名字：

```toml
public_hosts = ["agents.example.com"]
```

另一种做法是让代理改写它——在 Caddy 中，在 `reverse_proxy` 块内加 `header_up Host 127.0.0.1`。

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
