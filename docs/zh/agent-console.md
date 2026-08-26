# Agent Console

**Agent Console** 是 `cryohub` 提供的网页界面——一个手机优先、可安装的应用，用来阅读和操控 hub 所知的每一个 chamber。一个 chamber 就是一条平铺的对话：一侧是智能体的报告和提问，另一侧是你的回复，chamber 的控制项一步之遥。

它**内嵌在 `cryohub` 二进制中**，无需安装：启动 hub，打开它打印的 URL 即可。

```bash
cryohub start        # http://127.0.0.1:8765
```

![Agent Console 显示 chamber 的对话，包含智能体报告、表格和图像](images/agent-console.png)

## 登录

`cryohub start` 默认以**公开模式**运行：每个 `/api` 路由都需要 bearer token，控制台会显示登录页。首次启动会创建 owner token 并打印出来——这一行就是你的登录凭据，请保存好：

```
Owner token (save it — or reprint later with `cryohub token owner`):
3f9c…
```

两种 token 可以打开控制台：

- **Owner token。** 由首次 `cryohub start` 打印，之后随时可用 `cryohub token owner` 再次打印（幂等——始终是同一个密钥）。粘贴到 *Access token* 中。这就是你本人：对每个 chamber 拥有完全控制权。
- **邀请链接。** 持有 owner token 的人铸造一条仅限某个 chamber 的链接发给你。打开链接*就是*登录——token 位于 `#invite=` 片段中，被存储后会在任何其他代码运行之前从地址栏中抹去。

没有账户、密码或邮箱。token 即身份。

`cryohub start --no-public` 可以退出公开模式：没有登录环节，能访问 `127.0.0.1` 的人就能使用 hub。开放模式下分享与邀请链接无法工作。

## Owner 视图与访客视图

| | Owner | 邀请持有者 |
|---|---|---|
| 项目列表 | 全部 chamber，含状态点、下次唤醒、待回答问题标记、已完成 / 已归档分组 | 仅被邀请的 chamber，平铺 |
| 对话 | 阅读、发送、上传文件、打开附件 | 相同，仅限被邀请的 chamber |
| **⋯ Chamber 控制**（启动、停止、重启、重置、归档；Todos · Plan · Notes · Settings · Log 标签页） | 有 | 从不显示 |
| **Invite**（铸造链接、People with access、Remove） | 有 | 从不显示 |
| **+ 新建 chamber**、*Refresh chambers*、*Show completed & archived* | 有 | 从不显示 |

这张表只是 UI 层面的决定，真正的边界在 hub：每个路由都按**默认拒绝**分类。访客直接调用 owner 路由——chamber 状态、todo、生命周期、同步、token 管理——无论应用画了什么都会得到 `403`；访客的实时事件流也从不携带日志行或其他 chamber 的消息。

## 创建 chamber

仅 owner 可见的 **+ 新建 chamber** 面板会在一次操作中创建并启动 chamber。它使用 `cryohub.toml` 中主机级的 `default_agent`；如需更改，请先在控制台的 **Settings** 面板中修改。hub 会在创建任何目录之前检查该命令的可执行文件是否可用。如果脚手架创建成功、但 daemon 启动失败，新 chamber 仍会保留，控制台会显示启动错误，修复后可从 Chamber 控制中再次启动。

## 邀请他人加入某个 chamber

1. 用 owner token 登录，打开该 chamber，点击标题栏的 **Invite**。
2. 可选地为对方命名（留空则依次为 `guest-1`、`guest-2`……；名字在整个 hub 内唯一），然后 **Copy invite link**。链接会被铸造、限定在这一个 chamber，并一步复制到剪贴板。
3. 发给对方。链接**只显示一次**，hub 不会再次显示。链接丢了就再铸一条。
4. 同一面板上的 **People with access** 列出所有能访问此 chamber 的有效链接。**Remove** 会在确认后吊销：链接立即失效，访客已打开的事件流结束，其下一次请求得到 `401`，并被送回登录页，提示 *"Your session is no longer valid — please sign in again."*（若再次打开那条已吊销的链接，则提示 *"This invite link is no longer valid."*）

分享需要公开模式——在开放的回环 hub 上，面板会直接说明这一点，而不是铸造一条谁也用不上的链接。

同样的 token 也可以从命令行管理：

```bash
cryohub token create --name alice --chambers qec-decoders   # 只打印一次链接片段
cryohub token list
cryohub token revoke alice
```

## 选择 chamber 运行哪个智能体

两个下拉框，都只对 owner 可见：

- **Settings → Default agent** 是主机级默认值，保存在 `cryohub.toml` 的
  `default_agent` 中。它决定**新建** chamber 使用哪个运行器——包括控制台的
  *+ 新建 chamber* 和同一台机器上的普通 `cryo init`。修改它不会重写任何已存在的
  chamber。
- **⋯ Chamber 控制 → Settings → Agent** 是单个 chamber 自己的运行器，保存在该
  chamber 的 `cryo.toml` 的 `agent` 中。

两个列表都提供 `pi`、`opencode`、`claude`、`codex` 和 `kimi`，外加当前已保存的
值——像 `pi --thinking high` 这样手写的命令，或指向自定义运行器的路径，都会保持
可选，而不会被悄悄替换。其他想运行的命令请直接写进 `cryo.toml`。

保存任一下拉框时，hub 都会检查该命令的可执行文件在主机上是否可用。不可用的运行器会被拒绝，原设置保持不变。

守护进程在启动时读取 `cryo.toml`，因此修改**正在运行**的 chamber 的智能体要到下次
重启才生效；遇到这种情况控制台会提示。保存同时会重写 `cryo.toml`，该文件中的注释
不会被保留。

## 编辑 chamber 的计划

**⋯ Chamber 控制 → Plan → Edit plan** 会以 markdown 源码打开 `plan.md` 并写回。
仅 owner 可用。

无需重启任何东西：智能体被要求在每个会话开始时读取 `plan.md`，因此下一次唤醒就会
按新的计划工作。以最后一次写入为准——控制台没有冲突提示，因为 chamber 自己的智能体
被要求把运行状态保存在 `NOTES.md` 中；出于同样的原因，`NOTES.md` 在这里保持只读。

## 安装到手机或桌面

控制台是一个 PWA。在浏览器中打开后：

- **Android / Chrome：** *⋮ → 添加到主屏幕*（或 *安装应用*）。
- **iOS / Safari：** *分享 → 添加到主屏幕*。
- **macOS：** Chrome *安装*，或 Safari *文件 → 添加到程序坞*。

安装的应用绑定到提供它的那个 hub——一次安装对应一个 hub。原生 App 解除了这个限制，可以保存多份访问链接，并按 Owned 与 Joined 分类，参见[安装 App](./install-app.md)。更新随 hub 到来：`cargo install cryochamber` 升级并 `cryohub restart` 之后，打开的应用会显示 *Update available · Reload* 提示条。

**没有推送通知**，这是有意为之：应用只在打开时同步。它是一个你主动查看的控制台，不是寻呼机。

## 公网部署（在外用手机访问）

`cryohub` 始终只绑定回环地址。要在外面用手机访问，需要在前面放一个终止 TLS 的反向代理。公开模式本来就是默认的，无需切换——只要确认你手里有 owner token。

```bash
cryohub start                # 所有 /api 路由启用 bearer 鉴权；首次运行会打印 owner token
cryohub token owner          # 也可以事后再打印一次——这就是你的登录凭据
```

文档采用 Caddy 作为代理。把下面内容复制到 `/etc/caddy/Caddyfile`，替换主机名（在 reload 之前它必须已有指向此主机的 A/AAAA 记录，否则无法签发证书），然后 `systemctl reload caddy`：

```caddyfile
agents.example.com {
	encode zstd gzip
	reverse_proxy 127.0.0.1:8765
}
```

hub 会拒绝 `Host` 头既不是回环地址也不是已配置名称的请求——这正是防御 DNS 重绑定的机制——而 Caddy 默认会转发公网主机名，因此要在 `cryohub.toml` 中放行它：

```toml
public_hosts = ["agents.example.com"]
```

（另一种做法是在 `reverse_proxy` 块里加 `header_up Host 127.0.0.1`。）然后在手机上打开 `https://agents.example.com`，粘贴 owner token 或打开邀请链接，再*添加到主屏幕*。

在 `--public` 下控制台自身的页面仍无需鉴权——它们就是登录页。`/api` 下的一切都在 token 之后。

在 public 模式下，每个凭证（访客与 owner 一视同仁）的发送与上传都受限流：突发额度 5 次，之后每分钟 10 次；超出后 hub 返回 `429`，并带上 `Retry-After` 头。每次被接受的发送都会唤醒一个 agent，所以这个限制正是防止邀请链接把 owner 的账单跑高的东西。

## 从其他位置提供构建产物（`console_dir`）

使用控制台并不需要这一项。它用于开发，或运行一个比二进制内嵌版本更新、不同的控制台构建：

```toml
# ~/.config/cryo/cryohub.toml
console_dir = "/home/alice/src/cryochamber/console/dist"
```

路径必须是**绝对路径**（hub 会基于服务进程的工作目录规范化它，而工作目录由 launchd/systemd 决定）。`make console-build` 生成 `console/dist/`；`cryohub restart` 后生效。`cryohub status` 会打印当前生效的来源——`Console: embedded` 或 `Console: <path> (present|missing)`。

hub 对 `/` 和任何客户端路由返回 `index.html`，从 `/assets/` 提供带不可变缓存的哈希资源，并且绝不允许请求指向控制台目录之外的文件。`/api` 不受影响。

## 设备上存储了什么

访问 token、hub 记录的你的名字、每个 chamber 的已读水位、草稿，以及一小份最近消息缓存——全部存放在该 hub 源（origin）的 `localStorage` 中。退出登录会清除它们。消息正文在客户端渲染，并在进入 DOM 之前经过净化。
