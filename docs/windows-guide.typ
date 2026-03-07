#set page(paper: "a4", margin: 2cm)
#set text(font: "Arial", size: 11pt)
#set heading(numbering: "1.")

#align(center)[
  #text(size: 20pt, weight: "bold")[
    Cryochamber Windows 用户指南
  ]
  #v(0.5em)
  #text(size: 12pt)[
    Windows平台安装与使用说明
  ]
]

#v(1em)

= 快速开始

== 安装

```bash
cargo install --path .
```

== 初始化项目

```bash
cd your-project
cryo init
```

编辑 `plan.md` 文件，定义你的任务计划。

= 运行模式选择

Cryochamber在Windows上提供两种运行模式：

== 模式1：服务模式（推荐）

*特点：* 完全自动化，daemon持续运行，自动定时唤醒

*要求：* 需要管理员权限

*使用方法：*
```bash
# 以管理员身份运行PowerShell或CMD
cryo start
```

*优点：*
- ✅ 自动定时唤醒
- ✅ 开机自启动
- ✅ 系统服务管理
- ✅ 稳定可靠

*适用场景：*
- 长期运行的自动化任务
- 需要定时执行的工作流
- 生产环境部署

== 模式2：后台模式

*特点：* 无需管理员权限，但需要手动触发唤醒

*使用方法：*
```bash
CRYO_NO_SERVICE=1 cryo start
```

*限制：*
- ❌ 无法自动定时唤醒
- ⚠️ 需要手动运行 `cryo wake`

*适用场景：*
- 测试和开发
- 无管理员权限的环境
- 手动控制执行时机

= 常用命令

== 启动daemon
```bash
# 服务模式（推荐）
cryo start

# 后台模式
CRYO_NO_SERVICE=1 cryo start
```

== 查看状态
```bash
cryo status
```

== 手动唤醒（后台模式需要）
```bash
cryo wake
```

== 查看日志
```bash
cryo watch
# 或
cryo web
```

== 停止daemon
```bash
cryo cancel
```

= 故障排除

== 问题：服务启动失败，提示"拒绝访问"

*原因：* 没有管理员权限

*解决方案：*
1. 右键点击PowerShell/CMD，选择"以管理员身份运行"
2. 或使用后台模式：`CRYO_NO_SERVICE=1 cryo start`

== 问题：后台模式下daemon不自动唤醒

*原因：* Windows后台进程限制

*解决方案：*
- 使用服务模式（需要管理员权限）
- 或设置Windows任务计划程序定期运行 `cryo wake`

== 问题：Agent启动失败

*检查：*
1. 确认agent命令已安装（如 `opencode`）
2. 查看 `cryo-agent.log` 了解详细错误
3. 手动运行agent命令测试

= 推荐配置

== 服务模式（生产环境）

```toml
# cryo.toml
agent = "opencode"
max_retries = 5
max_session_duration = 0
watch_inbox = true
```

以管理员身份运行：
```bash
cryo start
```

== 后台模式 + 任务计划（开发环境）

1. 启动后台模式：
```bash
CRYO_NO_SERVICE=1 cryo start
```

2. 创建Windows任务计划：
   - 打开"任务计划程序"
   - 创建基本任务
   - 触发器：每小时
   - 操作：运行 `cryo wake`

= 更多信息

- 完整文档：https://giggleliu.github.io/cryochamber/
- GitHub：https://github.com/giggleliu/cryochamber
- 问题反馈：GitHub Issues
