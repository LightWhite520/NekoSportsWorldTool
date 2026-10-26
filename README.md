<h1 style="font-size: 48px;">别倒卖了😭</h1>

# NekoSportsWorldTool

![Release](https://img.shields.io/github/v/release/YanamiNeko/NekoSportsWorldTool)
![Build](https://github.com/YanamiNeko/NekoSportsWorldTool/actions/workflows/release.yml/badge.svg)
![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-blue)

运动世界校园版全协议自动化工具，纯 Rust 单文件，GUI / CLI 双模式。

> ⚠️ 仅供学习研究，请勿用于违反学校规定的场景，后果自负。

## 功能

- **跑步**：一键全链（策略 → 打卡点 → 轨迹 → 提交 → OBS → 验证），轨迹按打卡点自然生成，开始时间支持随机或指定到前 3 天
- **AI 运动**：任务 / 自由练习双模式，批量补签（前 60 天 × 多项目）
- **数据**：学期完成度、违规自查、排行榜（个人/班级/院系 × 日/月、室内榜、历史榜）、个人主页
- **登录**：支持人机验证自动通过
- **自动更新**：桌面端检查/下载/自替换并重启（GitHub Release 直连，无镜像）；Android 下载 APK 调起系统安装器；启动检查模式可选（静默/询问/关闭）

## 界面

![GUI](docs/screenshot.png)

## 技术特点

- 完整实现 NetSecKit 信封加密与响应解密，每个响应强制 RSA 验签（fail-closed）
- 人机验证全程本地完成，不依赖任何外部服务
- 轨迹生成器：打卡点拟合环 + 自然速度曲线 + 哨兵点 / 断崖 / 点位吸附
- 设备身份持久化（DeviceId / 安装时间 / MAC），稳定不易触发风控
- GUI 与 CLI 共用同一套协议层，数据文件互通

## 使用

双击 exe 进入 GUI；CLI 用法：

```text
NekoSportsWorldTool login --user <手机号> --pass <密码> --remember
NekoSportsWorldTool run                                  # 一键跑步（--altitude 17.2 固定海拔，或 11.6-22.8 映射到区间）
NekoSportsWorldTool ai --sport 2 --score 26000           # AI 运动
NekoSportsWorldTool rank main --type 1 --sort 1          # 排行榜
NekoSportsWorldTool update [--check]           # 自动更新（--check 仅检查不下载）
NekoSportsWorldTool template --file run.gpx   # 本地分析真实记录海拔（不会上传）
NekoSportsWorldTool help                                 # 全部命令
```

`template` 只读取用户手动选择的本地 GPX/JSON 文件，输出采样点、海拔范围、累计上升和累计下降，
不会登录、访问服务器或把模板记录接入跑步上传流程。

手动海拔支持三种状态：留空使用自动海拔；填写单个数字（例如 `17.2`）让所有轨迹点使用固定海拔；填写区间（例如 `11.6-22.8`）将生成的海拔曲线映射到该上下限内，同时保留轨迹的相对起伏。

青龙定时任务：首次手工 `login --remember` 一次，之后定时挂 `run` 即可。

## 常见问题

- **提示未登录 / 401**：先执行 `login --remember`（GUI 勾选记住密码等价），会话失效会自动重登
- **10121 设备风险**：设备身份是持久化的，删除 `identity.json` 等于换了新设备，不要频繁删
- **10603 点位限流**：点位接口 5 分钟限 3 次，程序内置 300s 缓存，正常使用不会触发
- **榜单为空**：当天还没有人产生有效里程，查询会自动回退最近 3 天
- **检查更新失败**：更新仅直连 GitHub，网络不通时到 [Release](https://github.com/YanamiNeko/NekoSportsWorldTool/releases) 手动下载
- **手动更新方式**：Windows 下载新的 `nekosportsworldtool.exe` 后退出软件，用新文件替换旧文件；请保留 `config.json`、`identity.json`、`points_cache.json` 等配置文件，不要删除或修改。
- **人机验证失败**：自动重试 3 轮，仍失败大概率是网络波动，稍后再试
- **杀软误报**：未签名编译产物可能被误报，自行判断后加白

## 构建

```bash
cargo build --release                        # GUI + CLI
cargo build --release --no-default-features  # 仅 CLI
```

推 tag 自动构建 Windows / macOS / Linux 四平台产物并发布 Release（见 `.github/workflows/release.yml`）。

## 许可证

[CC BY-NC 4.0](LICENSE) © YanamiNeko（禁止商用）

### 登录信息加密 ZIP 备份

登录栏的「登录备份（加密 ZIP）」支持跨设备导入/导出。Android 使用系统文件选择器，无需 Root 或存储权限；桌面版使用文件对话框。

- 导出前设置并再次确认独立归档密码（至少 12 个字符，建议使用随机密码）。ZIP 内唯一的 `login.json` 使用 WinZip AES-256 加密和认证，包含版本 2、会话、设备身份、账号及已记住的密码；未记住的密码不导出。文件名本身不加密。密码不保存，遗失后无法恢复，归档不应公开分享。
- 导入需填写归档密码，并确认替换账号和设备身份。先在内存中完整解密、校验，再写入固定的应用文件；不把 ZIP 路径解压到磁盘，也不生成明文中间备份。拒绝旧版明文 ZIP、错误密码、损坏/超限文件、额外文件、未知版本和不匹配的设备身份，限制 4 MiB。
- 跑步及路网设置保留，账号缓存清空，旧会话后台结果失效。导入等待后台任务完成；服务器可能拒绝已过期或撤销的凭证，需重新登录。导入失败会尝试恢复内存快照中的原文件；这不是跨文件事务，断电/进程终止或回滚期间磁盘故障不能保证恢复。
- **存储权限**：导出的归档始终加密；恢复后的 `session.json`、`identity.json`、`config.json` 仍采用应用既有本地明文存储格式，不等于系统密钥链保护。Android 位于应用私有目录；Unix 本次恢复写入设为仅所有者读写（0600）；Windows 继承所在目录 ACL，请把便携版放在自己用户可访问的目录。后续既有保存操作仍遵循原有实现。
- **删除**：在系统文件管理器中删除不再需要的 ZIP；应用不会持有或删除外部归档。退出登录清除当前会话，取消「记住密码」后重新登录会清除本地保存的密码；卸载 Android 应用会移除其私有文件，桌面版清理需退出程序后自行删除上述本地文件。删除文件不保证安全擦除，也不撤销其他副本或已发放的服务端 token。
