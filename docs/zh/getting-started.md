# 快速上手

这一页带着一台机器从零走到一个运行中的 hub：建好第一个 chamber，在手机上就能读，
还能分享给朋友。一共五步：启动 hub、装好 Pi agent、连接、创建第一个 chamber、分享。

> **平台：** hub 运行在 macOS 和 Linux 上。原生 App 支持 Apple Silicon 的
> macOS；在手机上，浏览器控制台可以作为 PWA 安装使用。

## 1. 启动 hub

本页介绍当前 main 的功能，控制台和认证功能晚于 v0.2.8。下次发布前，
请安装 Rust 和 Node.js 22，并在仓库源码目录运行：

```bash
make console-build
cargo install --path . --locked
cryohub start
```

`cryohub start` 会把 hub 安装为用户级服务（重启后依然存活），绑定
`http://127.0.0.1:8765`，并在第一次运行时打印 **owner token**：

```
Owner token (save it — or reprint later with `cryohub token owner`):
3f9c…
```

这个 token 就是你的登录凭证——没有账号、密码或邮箱。任何时候都可以用
`cryohub token owner` 重新打印。

## 2. 装好 Pi agent

chamber 需要 hub 主机上装有一个 agent 运行器。**Pi** 是内置默认：

```bash
npm install -g @mariozechner/pi-coding-agent
which pi        # hub 在启动 chamber 前会验证这个可执行文件存在
```

Pi 从环境变量读取模型提供商的 API key，而你不需要在全局导出任何东西：下一步的
**+ New chamber** 表单里有一个折叠的 *API key* 区域，会把 key 写进该 chamber 自己的
`cryo.toml` 的 `[provider] env`，daemon 在每个会话启动时注入。

想用别的运行器？`claude`、`opencode`、`codex`、`kimi` 都在 *Settings → Default
agent* 下拉框里——见[选择 chamber
运行哪个智能体](./agent-console.md#选择-chamber-运行哪个智能体)。

## 3. 连接

**在 hub 所在机器上：** 用浏览器打开 `http://127.0.0.1:8765`，粘贴 owner token。

**从手机或另一台机器：** hub 始终只绑定 loopback，所以要在前面架一个终结 TLS 的反向
代理——[公网部署一节](./agent-console.md#公网部署在外用手机访问)有完整的 Caddy
配方。然后二选一：

- 在手机浏览器打开 `https://agents.example.com`，*添加到主屏幕*；或者
- 在 Mac 上安装[原生 App](./install-app.md)，添加 hub——地址加 owner token。App
  可以**同时持有多份访问链接**，并按 Owned 与 Joined 分类；浏览器安装的 PWA 只绑定提供它的那一个 hub。

## 4. 第一个 chamber：host manager

一个很好的引导型 chamber，就是照看 hub 主机自身的那个。在控制台里点
**+ New chamber**，取名 `host-manager`，把 API key 粘贴进折叠的 *API key* 区域，然后
创建——表单会在一次操作里搭好 chamber 并启动它。接着打开 **⋯ Chamber controls →
Plan → Edit plan**，给它一份这样的任务简报：

```markdown
# Host manager

你负责照看这台机器。每天一次：

- 检查磁盘剩余空间（`df -h /`）；使用率超过 85% 时提醒我。
- 检查 `cryohub status` 和其他 chamber 的 `cryo status` 是否健康。
- 用几行字汇报；只把需要人处理的事提出来。

每天 09:00 左右醒来一次。如果我给你发消息，处理并回复。
```

编辑计划后不需要重启任何东西：agent 在每个会话开始时都会读 `plan.md`。从这里开始，
它自己安排唤醒时间，把每个会话的结果汇报到 chamber 的对话里，并回复你发去的任何消息。

## 5. 把 chamber 分享给朋友

打开这个 chamber，点标题栏里的 **Invite**，可选地填上朋友的名字，然后
**Copy invite link**。把链接发给对方：

- 在浏览器里打开，这个链接*就是*登录——对方直接落在这一个 chamber 里；
- 粘贴进 App 的 *Add a chamber → Admin or invite link* 字段，它会替对方填好地址和 token。

链接只限定在这一个 chamber：访客可以在里面阅读和发送，但永远看不到你的其他
chamber 和任何控制项。同一张表单上的 **People with access** 列出每条有效链接；
**Remove** 立即吊销一条。分享的前提是朋友能访问到这个 hub——也就是第 3 步的反向代理。
