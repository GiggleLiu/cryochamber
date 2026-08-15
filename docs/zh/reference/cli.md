# CLI 参考

所有 cryochamber 二进制及其命令。`cryo.toml` 和 `cryohub.toml` 的字段见 [配置](./configuration.md)。

每个二进制都接受 `--version`（打印版本并退出）和 `--help`。

## 操作员 CLI（`cryo`）

除非另有说明，请在 chamber 目录内运行这些命令。

<table>
<thead>
<tr><th>类别</th><th>命令</th><th>作用</th></tr>
</thead>
<tbody>
<tr class="group"><td rowspan="6">生命周期</td><td><code>cryo init [--agent &lt;cmd&gt;]</code></td><td>初始化目录：写入 <code>cryo.toml</code>、<code>plan.md</code>、<code>NOTES.md</code> 和 <code>README.md</code>。已有文件会被保留。</td></tr>
<tr><td><code>cryo start [--agent &lt;cmd&gt;]</code></td><td>启动守护进程。读取 <code>cryo.toml</code> 并把覆盖项写入 <code>timer.json</code>。</td></tr>
<tr><td><code>cryo start --max-session-duration 3600</code></td><td>为本次运行覆盖会话超时时间。</td></tr>
<tr><td><code>cryo status</code></td><td>显示守护进程是否在运行、当前会话编号以及下一次唤醒时间。</td></tr>
<tr><td><code>cryo restart</code></td><td>重启正在运行的守护进程。若它已安装为操作系统服务，则重启现有服务而不重写或移除它。</td></tr>
<tr><td><code>cryo cancel</code></td><td>停止守护进程并移除运行时状态。</td></tr>
<tr class="group"><td rowspan="2">日志</td><td><code>cryo watch [--all] [--viewpoint cryo|agent]</code></td><td>实时跟踪日志。<code>--all</code> 从头显示日志。<code>--viewpoint cryo</code>（默认）跟踪结构化事件日志；<code>--viewpoint agent</code> 跟踪智能体原始输出（<code>cryo-agent.log</code>）。</td></tr>
<tr><td><code>cryo log</code></td><td>打印完整会话日志。</td></tr>
<tr class="group"><td rowspan="2">消息</td><td><code>cryo send "&lt;message&gt;" [--from &lt;name&gt;] [--subject &lt;text&gt;]</code></td><td>向智能体的收件箱发送消息；守护进程的收件箱监视器会唤醒智能体。<code>--from</code> 设置发送者（默认 <code>human</code>），<code>--subject</code> 设置主题（默认根据正文生成）。</td></tr>
<tr><td><code>cryo receive</code></td><td>读取智能体发送到发件箱（outbox）的消息。</td></tr>
<tr class="group"><td rowspan="2">维护</td><td><code>cryo clean [--force]</code></td><td>移除日志、状态、消息等运行时文件。</td></tr>
<tr><td><code>cryo ps [--kill-all]</code></td><td>列出（或终止）本机上所有正在运行的 cryo 守护进程。可从任意目录运行。</td></tr>
</tbody>
</table>

## Hub（`cryohub`）{#hub-cryohub}

| 命令 | 作用 |
|---------|--------------|
| `cryohub start [--host <ip>] [--port <n>]` | 安装一个重启后依然存活的服务。`--host` 和 `--port` 同时更新已保存的 hub 配置。 |
| `cryohub start --foreground` | 在当前终端运行 hub，而不是安装服务。 |
| `cryohub stop` | 卸载全局 hub 服务。 |
| `cryohub restart` | 重启已安装的全局 hub 服务，无需重新安装。 |
| `cryohub status` | 显示全局 hub 的 URL、chamber 根目录、配置文件路径、日志路径和服务状态。同时列出旧版本遗留的、以当前目录为作用域的 hub 服务。 |
| `cryohub start --public` | 对所有 `/api` 路由强制启用 bearer token 鉴权。没有 owner token 时拒绝启动。该标志会写入安装的服务单元，因此重启后依然保持鉴权。 |
| `cryohub token owner` | 打印 owner token，首次运行时创建。幂等——重复运行打印同一个密钥。 |
| `cryohub token create --name <名称> --chambers <id,...>` | 创建一个限定到这些 chamber id 的具名邀请。打印 token 及其 `#invite=` 链接片段；这是密钥唯一一次显示的时机。 |
| `cryohub token list` | 列出邀请及其作用域、创建时间和吊销状态。绝不打印 token 字符串。 |
| `cryohub token revoke <名称>` | 按名称吊销邀请。立即生效，包括已经打开的 SSE 流。若没有同名的有效邀请则失败。 |

## 智能体 IPC（`cryo-agent`）

这些命令由被派生的 AI 智能体用来通过 Unix socket 与守护进程通信。它们不是操作员接口。

<table>
<thead>
<tr><th>类别</th><th>命令</th><th>作用</th></tr>
</thead>
<tbody>
<tr class="group"><td rowspan="4">休眠</td><td><code>cryo-agent hibernate --summary "..."</code></td><td>结束会话；还有更多工作要做。只要收件箱存在未读邮件，就会被拒绝（非零退出）——智能体必须先 <code>receive</code>、回复，再重试，因此会话绝不会在还有邮件等着它时结束。若没有任何待办 TODO 声明下次唤醒，同样会被拒绝。调用成功后最多可能阻塞智能体通过 <code>--linger &lt;秒数&gt;</code> 请求的回复窗口时长（省略 = 300，上限 86400；<code>0</code> 表示立即入睡）。</td></tr>
<tr><td><code>cryo-agent hibernate --complete</code></td><td>结束会话；计划已完成。此外，只要有 TODO 到期就会被拒绝。它永远不会被回复窗口驻留。</td></tr>
<tr><td><code>cryo-agent hibernate --exit 1</code></td><td>报告失败的会话。守护进程会把已消费的 TODO 标记为完成，并新增一个带编号的重试 TODO。失败报告永远不会被拒绝，也不会被驻留。</td></tr>
<tr class="group"><td rowspan="4">TODO</td><td><code>cryo-agent todo add "text" --at &lt;TIME&gt;</code></td><td>通过 TODO 安排下一次唤醒。<code>--at</code> 接受相对偏移（<code>+30 minutes</code>）、ISO 8601 时间戳（<code>2026-04-25T10:00</code>；容忍秒和空格分隔符），或仅日期（<code>2026-04-25</code>，表示午夜）。</td></tr>
<tr><td><code>cryo-agent todo list</code></td><td>列出所有 TODO 项。</td></tr>
<tr><td><code>cryo-agent todo done &lt;id&gt;</code></td><td>将某个 TODO 项标记为完成。</td></tr>
<tr><td><code>cryo-agent todo remove &lt;id&gt;</code></td><td>移除某个 TODO 项。</td></tr>
<tr class="group"><td rowspan="5">消息</td><td><code>cryo-agent send "message"</code></td><td>向发件箱写入一条给人类的消息。</td></tr>
<tr><td><code>cryo-agent send --stdin</code></td><td>从 stdin 原样读取发件箱消息正文，包括末尾换行；适用于多行或对 shell 敏感的文字。</td></tr>
<tr><td><code>cryo-agent send --question "msg"</code></td><td>将该消息标记为等待人类回复的问题。</td></tr>
<tr><td><code>cryo-agent receive</code></td><td>领取（claim）当前来自人类的收件箱批次。</td></tr>
<tr><td><code>cryo-agent dialog [--last N | --all | --since &lt;iso&gt;]</code></td><td>渲染对话记录（默认：最近 20 条消息）。<code>--last N</code> 显示最近 N 条，<code>--all</code> 显示所有已归档消息，<code>--since &lt;iso&gt;</code> 显示某个 ISO 8601 时刻之后的消息；三者互斥。副作用是会归档任何待处理的收件箱批次，与 <code>receive</code> 一样履行回复义务。</td></tr>
<tr class="group"><td rowspan="3">时间</td><td><code>cryo-agent time</code></td><td>以 ISO 8601 格式打印当前本地时间。</td></tr>
<tr><td><code>cryo-agent time "+30 minutes"</code></td><td>计算相对偏移。单位：<code>minutes</code>、<code>hours</code>、<code>days</code>、<code>weeks</code>。</td></tr>
<tr><td><code>cryo-agent time "2026-04-25T10:00"</code></td><td>校验并规范化 ISO 8601 时间戳。</td></tr>
</tbody>
</table>

## Zulip 同步（`cryo-zulip`）{#zulip-sync-cryo-zulip}

| 命令 | 作用 |
|---------|--------------|
| `cryo-zulip init --config <zuliprc> --stream <name> [--topic <topic>] [--history]` | 校验凭据、解析 stream，并写入 `zulip-sync.json`。 |
| `cryo-zulip sync [--interval N]` | 启动后台同步守护进程。默认间隔来自 `cryo.toml`，否则回退为 5 秒。 |
| `cryo-zulip unsync` | 停止同步守护进程。 |
| `cryo-zulip pull` | 单次拉取。 |
| `cryo-zulip push` | 单次推送。 |
| `cryo-zulip status` | 显示同步配置。 |
