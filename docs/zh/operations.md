# 运行维护与发布

## 备份和恢复

先停止外部聊天桥接和 hub，再在每个 chamber 中运行 `cryo cancel`。
若启用了原生 Zulip 同步，运行 `cryo-zulip unsync`。确认没有相关进程继续写入。
备份整个 chamber，包括配置、计划、TODO、状态、日志、消息、附件和 `.cryo`。
保留 `.cryo/reply-obligation.json`，排除运行时 socket `.cryo/cryo.sock`。
同时备份 host 的 cryo 配置目录和 chamber 注册表。备份含密钥，应限制权限并加密保存。

先恢复到独立目录。原实例保持停止；清除恢复的 `timer.json` 中的 `pid` 和
`instance_id`，删除旧 socket，保留会话状态、TODO 领取状态和回复日志。
检查配置、消息历史和凭据后再启动。确认中断通知和新消息回复正常，再恢复桥接服务。
不要让原实例与恢复实例同时连接同一外部服务。

升级前保存旧版本二进制和一致的数据快照。回滚时一起恢复二进制和匹配的数据。
原生 App 的设备密钥不可迁移，新设备应重新获取 hub token。
完整命令和目录说明见[英文操作指南](../operations.html)。

## 历史和发布验证

控制台首次读取最近 100 条消息，按需加载更早页面。游标使用不可变消息文件名；
归档不改变游标。旧版本手工命名的文件按文件名分页。服务端只读取所选页面的消息正文，
但仍扫描文件名和完整会话日志。20 个 chamber、每个 10,000 条消息是初始测试目标，
尚不是延迟承诺。

版本 tag 必须通过 Rust、控制台、Python、原生 shell 和依赖检查。发布先保持草稿，
原生产物构建通过后才发布 crate，最后公开 GitHub release。发布前还需在真实 macOS
和 Android 设备上完成 `app/README.md` 的安装包测试，记录版本、提交、校验和及结果。

面向用户的 macOS 版本必须使用 Developer ID 签名并完成 notarization。维护者需配置
`APPLE_CERTIFICATE`、`APPLE_CERTIFICATE_PASSWORD`、`APPLE_SIGNING_IDENTITY`、
`APPLE_API_ISSUER`、`APPLE_API_KEY` 和 `APPLE_API_KEY_CONTENT`。缺少凭据时发布失败，
不会降级为 ad-hoc 签名。本地开发构建仍可使用 ad-hoc 签名。

`main` 要求 PR 和必需检查通过，禁止强制推送和删除。依赖检查在 PR、发布和定时任务中运行；
网络失败不能视为通过。原生 Linux 的 GTK3/GLib 上游维护问题需在支持 Linux 安装包前解决。
维护者应在 2026-12-01 前复查，详情见英文操作指南。
