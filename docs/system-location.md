# 系统定位

GUI 设备页使用地图分支的原生定位实现；以下平台依赖表主要说明合入的桌面 CLI 定位入口。
结果只填入编辑区；核对实际城市及经纬度，点击「保存」后才生效。
拒绝权限、系统定位关闭或超时不会覆盖现有配置，也不会回退到默认坐标或 IP 定位。
系统返回 WGS84 坐标，保存设备锚点前转换为 BD-09，与地图选点保持一致。精度由系统报告，不能保证 GPS 精度。
本功能不会主动把位置发送给第三方地理编码接口；系统位置提供者可能自行使用网络。
CLI 保存要求显式指定城市；GUI 自动识别城市后仍需核对。

桌面 CLI（无需登录运动账号）：

```sh
nekosportsworldtool location
nekosportsworldtool location --save --city 北京市
```

第一条只显示当前位置；第二条更新持久化定位锚点与城市，保留设备 UUID、安装时间和其它身份字段。
不会在启动或定时跑步时自动重新定位。

| 平台 | 系统提供者 | 要求 |
| --- | --- | --- |
| Windows | Windows.Devices.Geolocation / WinRT | Windows PowerShell 5.1；设置 → 隐私和安全性 → 位置，打开位置服务和桌面应用访问 |
| macOS 11+ | CoreLocation | 系统设置 → 隐私与安全性 → 定位服务；允许 NekoSportsWorldTool Location helper |
| Linux | GeoClue2 system D-Bus | `/usr/bin/python3`、python3-gi、geoclue-2.0；桌面定位授权代理或管理员配置的应用授权 |
| Android | LocationManager | 前台粗略或精确位置权限；网络/GPS 提供者至少一个启用；进入后台取消监听 |

Linux 安装 `assets/org.nekosportsworld.tool.desktop` 至
`~/.local/share/applications/`，保证 `nekosportsworldtool` 在 PATH 中。
GeoClue 的 DesktopId 为 `org.nekosportsworld.tool`；无桌面授权代理的服务器可能被拒绝，
须由管理员按发行版的 GeoClue 策略配置授权，不会通过 IP 查询绕过权限。
Linux 无桌面会话的容器/青龙环境通常没有系统位置服务，可继续手动配置。

macOS helper 在构建时通过 Xcode Command Line Tools 编译，随 Rust 二进制嵌入，
运行时释放到临时目录并在结束后移除，不需要用户安装编译器。
其可执行文件内嵌定位用途声明，单独进程在主线程运行 CoreLocation/RunLoop。
macOS 系统授权行为和首次提示需在真实 macOS 上验证；分发时建议给 helper 和主程序签名。

目前应用构建目标是 Windows、macOS、Linux、Android；设备页 iOS/Android 选项是协议身份，
不是当前操作系统，选择它不会改变系统定位提供者。本仓库没有 iOS 应用目标。
