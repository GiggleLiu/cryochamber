# 安装 App

**Cryochamber App** 是把 hub 在浏览器里提供的那个 [Agent
Console](./agent-console.md) 装进原生窗口的版本。它存在的理由只有一个——浏览器做不到的事：
它能**同时持有多个 hub**，合并成一份 chamber 列表，并且每个 hub 的请求都走操作系统的网络栈，
而不是页面自身的 origin。

hub 这边什么都不变。hub 提供的网页控制台还在原处——用浏览器打开
`http://127.0.0.1:8765`，或者把它*添加到主屏幕*当作 PWA 使用，得到的是绑定到那**一个** hub
的同一套界面。当你的 hub 数量不再是「一个」时，App 才是那个选项。

构建产物附在每次 [GitHub
release](https://github.com/GiggleLiu/cryochamber/releases) 上：

| 文件 | 适用于 |
|---|---|
| `cryochamber-vX.Y.Z-android-arm64.apk` | Android 手机与平板，64 位 ARM |
| `cryochamber-vX.Y.Z-macos-arm64.dmg` | Apple Silicon 上的 macOS |
| `cryochamber-vX.Y.Z-macos-arm64.app.zip` | 同一个 App 的 zip 包，如果你不想挂载磁盘映像 |

没有 Play Store 上架，没有 App Store 上架，没有 Windows 构建，也没有 Intel Mac 构建。
上面列出的就是全部。

## Android

1. 在手机浏览器中打开 release 页面，下载
   `cryochamber-vX.Y.Z-android-arm64.apk`。
2. 点击下载好的文件。Android 第一次会拒绝，并给出一个设置页面——针对你下载所用浏览器的
   *「安装未知应用」*。允许它，返回，再点一次。
3. 打开 App，它会停在 **Add a hub**（添加 hub）界面。
4. 填入 hub 地址和访问 token；或者把邀请链接粘贴到 *Invite link* 中——链接会替你填好两个字段
   并随即清空自己，于是 token 只会停留在那个被遮蔽显示的字段里。

该构建仅支持 arm64。32 位 ARM 或 x86 的 Android 设备会拒绝安装。

## macOS

1. 下载 `cryochamber-vX.Y.Z-macos-arm64.dmg`，打开它，把 **Cryochamber**
   拖到 *应用程序*。
2. **首次启动需要右键。** 该构建使用 ad-hoc 签名，且**未做公证（notarization）**，
   所以双击只会得到*「无法打开，因为无法验证开发者」*。在 *应用程序* 里右键（或
   Control-点击）该 App → **打开** → **打开**。macOS 会记住这个决定，之后就能正常启动了。
3. 添加 hub 的方式与 Android 相同。

仅支持 Apple Silicon。公证尚未完成，在完成之前本页会一直这样写着。

## 信任：App 在发送 token 之前会问什么

访问 token 就是密码。在把它发往某个地址之前，App 会**显式地**判定这个地址可以被信任到什么程度。
一共三种情况。

**系统已信任其证书的 HTTPS。** 什么都不问。这就是挂在反向代理后、持有真实证书的 hub
（[控制台指南里的 Caddy 配置](./agent-console.md)），也是应当追求的情况。

**明文 `http://`。** 只要你输入的是明文地址，地址下方立刻出现警告，并且 **Add hub**
按钮保持禁用，直到你勾选 *「I understand traffic to this hub is unencrypted」*
（我明白发往此 hub 的流量未加密）。字面含义是：token 和你发送的每一条消息，在设备与 hub
之间的任何一环上都是可读的。在自己机器上（`http://127.0.0.1:8765`）这个「任何一环」是空的，
勾选只是走个形式；在咖啡馆或会场的网络里，它是同一网络上的所有人。修改地址会取消勾选——
这份确认针对的是某一个主机，而不是这张表单。

**系统不信任其证书的 HTTPS**——自签名证书，或私有 CA。App 既不会默默接受，也不会默默拒绝。
它会探测该主机，然后弹出 **Untrusted certificate**（不受信任的证书）面板，
以冒号分组的大写十六进制显示证书的 SHA-256 指纹。请与 hub 运维者念给你听的值比对：

```bash
# 在 hub 主机上，针对代理所提供的证书
openssl x509 -fingerprint -sha256 -noout -in /path/to/cert.pem
# sha256 Fingerprint=88:44:DD:65:…

# 或者从任意位置，直接取自实时握手
openssl s_client -connect hub.example:8443 </dev/null 2>/dev/null \
  | openssl x509 -fingerprint -sha256 -noout
```

面板正是采用这种分组方式，好让两者可以一组一组地对读，而不是把 64 个字符当成一整串去瞪。
如果一致，**Add hub anyway** 会从此把*那一张*证书钉死（pin）在*那一个* hub 上。
如果不一致，那么是别人在冒充这个 hub——**Cancel** 不会存下任何东西。

被钉死的 hub 之后若换了另一张证书，连接会直接失败，而不是悄悄信任新证书。当运维者正常更换证书时，
在 *Settings → Hubs* 中移除该 hub 再重新添加，即可重新钉定。

## 同时使用多个 hub

*Settings → Hubs* 列出 App 记住的每一个 hub——标签、地址、当前 token 在该 hub
上是 Owner 还是 Guest、以及该 hub 的 `cryohub` 版本——底部是 **Add hub**，每一行上是 **Remove**。

当配置了不止一个 hub 时，chamber 列表会变成跨所有 hub 合并的一份列表，每一行都会多出一个
**hub 标签片（chip）**，标明它来自哪个 hub。（只有一个 hub 时标签片会被隐藏；每行重复同一个词
只是噪音。）未读计数、草稿和已读水位都按 hub 分开保存，彼此不会串。

各个 hub 独立失败。某个 hub 不再应答时，它那些行的标签片会显示 **· unreachable**
并变灰——该行仍然展示这个 hub 最后说过的话，而不是消失——与此同时其他每个 hub
照常推流、发送、更新。它恢复之后，那些行会自行复原；不需要重启，也不需要重新输入 token。

Hub 账号保存在 App 自己的私有数据目录里，而不是浏览器存储中，所以清空浏览器、
或 WebView 数据被回收，都不会让你丢掉 token。在 macOS 上该文件是
`~/Library/Application Support/com.cryochamber.console/hubs.json`。
它以明文保存 bearer token，仅靠该目录的文件权限保护——请像对待
`~/.config/cryo/cryohub-tokens.json` 那样对待它。

## 更新

没有应用内更新器，也没有更新通道。下载下一个 release 的构建，覆盖安装即可。

- **Android：** 直接安装更新的 APK。你的 hub 和 token 会保留，因为各次 release
  使用同一个密钥签名，Android 会将其视为同一 App 的升级。（来自*不同*签名者的 APK
  不算升级——Android 会要求你先卸载，而卸载会丢弃 hub 存储。）
- **macOS：** 替换 *应用程序* 里的 *Cryochamber*。hub 存储位于 App 包之外，不受影响。
  新下载的这一份可能需要再做一次右键打开的动作。

hub 自身是单独升级的（`cargo install cryochamber`，然后 `cryohub restart`）。
如果某个 hub 比 App 旧，*Settings → Hubs* 会在该 hub 那一行注明——
*「hub is older — some features may be missing」*。
