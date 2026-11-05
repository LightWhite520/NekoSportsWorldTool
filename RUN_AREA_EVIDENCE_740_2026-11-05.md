# runAreaModels：7.4.0 静态证据核验

核验日期：2026-11-05。初始取证仅搜索公开资料、下载公开 dump、离线解析与定向反编译；同日后续据此修正区域 ID 类型与请求距离，并运行本地测试。未登录或向真实业务服务器发起请求、提交记录。

## 结论

已找到 Android 7.4.0 的公开 DEX dump，并在其中确认：策略响应含 `runAreaModels`；所选区域的 `id` 用作请求 `runAreaId`；`fenceIds` 关联围栏；`valid=false` 阻止开始。现有跑区映射改动不再只有 7.2.70 的依据。

这支持字段模型与客户端关联逻辑，不证明某所学校实际下发这些字段、不证明其跑道策略的服务端触发条件，也不代表当前工具完整兼容 7.4.0。区域 ID 类型与请求距离已按下列证据修正；坐标传输约定仍待独立核实。

## 主证据：Inverx 公开的 7.4.0 dump

- 仓库快照：`Inverx/com.zjwh.android_wh_physicalfitness@412a2e28bed209201057bf8b943fcd0a0e2211c1`。
- 文件：[原始脱壳资料/运动世界7.4.0-FridaDump.zip](https://github.com/Inverx/com.zjwh.android_wh_physicalfitness/blob/412a2e28bed209201057bf8b943fcd0a0e2211c1/%E5%8E%9F%E5%A7%8B%E8%84%B1%E5%A3%B3%E8%B5%84%E6%96%99/%E8%BF%90%E5%8A%A8%E4%B8%96%E7%95%8C7.4.0-FridaDump.zip)。
- 首次添加的[提交 60466f4](https://github.com/Inverx/com.zjwh.android_wh_physicalfitness/commit/60466f41667db83093bf7ed558ce16239d49f640)，消息明确为“添加运动世界 7.3.92 和 7.4.0 原始脱壳资料”。
- 作者在 [issue #19 的回复](https://github.com/Inverx/com.zjwh.android_wh_physicalfitness/issues/19#issuecomment-6399817002)确认补充这两个版本的资料。

下载中断后续传成功，最终文件长度 51,164,059 字节，按 Git blob 算法计算的 SHA-1 与 GitHub tree 中 `e91be63ee6f3e02e540b0325ee457f27070fdf4c` 相同。

ZIP SHA-256：`f236f5bcfc5648d017b874c9074a33a75217b1908a1f2603129902112fca484e`。

9 个 DEX 均非全零，实际大小与 DEX header 声明相同，Adler-32 校验均通过；类表中存在本次核验的业务类。这些检查证明文件可解析且下载内容与公开 blob 一致，不证明所有方法均可完整反编译。

### 版本交叉核对

本机此前留存 APK 的二进制 AndroidManifest.xml 解码结果：

- package：`com.zjwh.android_wh_physicalfitness`
- versionName：`7.4.0`
- versionCode：`408`
- APK SHA-256：`ff9e36d970df261f1e8b49785c8ea5e65d84b334312c01b1aee168ef1d15d3e3`

该 APK 的 `classes.dex` 与公开 dump 的 `dump_exact_0_71634936.dex` 字节摘要相同：`8a232106b6ba8701d47833359de38685247f19d69326bf9a5f8b8cae2495b0a9`。这提供了与本地 7.4.0 安装包的关联，强于仅依据 ZIP 文件名判断版本。其余业务 DEX 来自发布者的运行时 dump，本轮没有自行运行 App 重新采集。

## 定向反编译结果

使用本机 JADX 1.5.6，读取 dump 中的第 6、7 号业务 DEX；不执行 APK。输出位于 `target/run-area-evidence-20261105/java740/`，行号对应本轮保留的文件。

| 核验项 | 7.4.0 观察 | 本地定位 |
|---|---|---|
| 字段类型 | `id: int`、`valid: boolean`、`name: String`、`fenceIds: List<Integer>`、`select: boolean` | `RunAreaModels.java:14` |
| 策略响应 | `RandomConfigBean` 保存 `runAreaModels` 与 `geoFence` | `RandomConfigBean.java:18` |
| 策略成功分支 | 读取 `getRunAreaModels()`，保存到 `CacheDataUtils` 并传给视图 | `RunPolicyViewModel.java:86` |
| 用户选区 | `setRunAreaIdInternal(data.getId())` 后重新请求点位 | `RunCreditFragment.java:1296` |
| 围栏关联 | 以围栏 ID 建表，遍历区域的 `getFenceIds()` 取对应围栏 | `RunStartViewModel.java:172` |
| 有效性 | 所选区域 `getValid()` 为 false 时提示“跑区有效时间已更新，请重新选择”并返回 | `RunCreditFragment.java:2397` |
| 距离初始化与单位 | `RunRuleModel.OooO0o()` 返回 `minDistance`，初始化 `mDistance`；公里显示时除以 1000 | `RunRuleModel.java:56`、`RunStartViewModel.java:168`、`:422` |
| 请求路径 | `"/api/v560/get/1/distance/" + getMDistance()` | `RunRandomStartViewModel.java:44` |
| 实际请求体 | `JSONObject.put("runAreaId", this.$runAreaId)`，捕获字段为 `int` | `CommonApiManager$requestFivePoint$1.java:26`、`:81` |
| 旧请求响应隔离 | 用递增请求序号检查点位响应有效性，过期响应直接丢弃 | `RunRandomStartViewModel.java:62`、`RunStartViewModel.java:470` |

主要业务 DEX 摘要：

- `dump_exact_6_10774560.dex`：`e5114390538e967ab40d3767319e87e197755bf1332655ca796eaf68f4482612`
- `dump_exact_7_9888816.dex`：`6af1c7325f4f671cb0c40f999047bcae3f75b5df07a0ff7d89591567b4bbb9a0`

`RunCreditFragment` 普通模式输出可读代码，但 JADX 返回 1 个错误，不能声称全类无误。另以 fallback 模式重导出，返回成功；其中 `RunCreditFragment.fallback.java:2155` 起可核对无效区域返回分支，`:2265` 起可核对所选区域 getter 的整数结果传入 `setRunAreaIdInternal`。其他本次定向导出的 7 个类命令均成功。JADX 可根据 getter 恢复可读名称；fallback 中对应 `RunAreaModels.OooO0O0()` 为 `id`，`OooO0Oo()` 为 `valid`。

## 其他 GitHub 证据的权重

| 来源 | 查到的内容 | 能证明什么 |
|---|---|---|
| [Inverx 7.3.92 policy/client.py](https://github.com/Inverx/com.zjwh.android_wh_physicalfitness/blob/412a2e28bed209201057bf8b943fcd0a0e2211c1/%E5%8D%8F%E8%AE%AE/7.3.92/policy/client.py) | 按 `runAreaModels`、`valid`、`fenceIds` 选择 ID | 第三方实现相符；不是官方 7.4.0 实测 |
| [no-study110 policy/client.py](https://github.com/no-study110/shijie-yundong-xiaoyuan/blob/0984ef3ed3507845e1451ef6e95a2506381f67a1/policy/client.py) | 相同选择逻辑 | `get_checkpoints_for_run` 的 AST 与 Inverx 完全相同，不能算第二份独立佐证 |
| [FunSportWorld policy.py](https://github.com/Funrestan/FunSportWorld/blob/41359e518763e1cb5e5731249323d44378f8a124/funsport/api/policy.py) | 保存 `run_area_models` 和 `geo_fence`，相关文档提及 Android 7.3.70 静态分析 | 支持字段沿用；没有提供本次所需的 7.4.0 非空真实响应 |
| [Demacia policy.rs](https://github.com/DemaciaForge/DemaciaSportsWorldTool/blob/b9916c059a3a4874aab562167e1504fc624ebc5d/src/api/policy.rs) | 测试样本中含 `runAreaModels: null` | 不能证明非空区域的结构与映射；7.4.0 抓包说明不等于此字段已独立实测 |
| Coaixy 7.2.70 反编译 | 先前已核对的字段与选区链路 | 较旧版本的旁证，本次已有新版 dump 可对照 |

代码搜索 `runAreaModels` 返回 22 个命中；字段与 `7.4.0` 的同文件组合搜索、JSON/Smali 限定搜索及 issue 字段搜索未找到额外直接样本。搜索索引不覆盖所有分支、二进制压缩包或私有资料，不能把未命中理解为不存在。最终 7.4.0 证据来自仓库文件树中的 ZIP，不是搜索引擎直接检索到的 Java。

## 对当前实现和推送说明的影响

可以说明：**已依据 7.4.0 公开 DEX 静态核对策略跑区字段、ID 与围栏映射、无效区域拦截，并完成本地回归测试。**

不能说明：已在该学校验证、保证获取到跑道检查点、完整适配 Android 7.4.0，或保证真实记录通过。

## 同日请求契约修正

在进一步导出并核对 7.4.0 的 `RunRuleModel` 后确认：`OooO0o()` 直接返回整数 `minDistance`；`RunStartViewModel` 用它初始化 `mDistance`，展示公里数时除以 1000；`RunRandomStartViewModel` 将未换算的米数直接拼入点位 URL。

已据此修正：

1. 区域 ID 在请求链中使用整数，并以 JSON 整数发送；缺少区域时继续省略字段，非法 ID 在请求前报错。
2. 点位 URL 使用本次学校策略的 `runRuleModel.minDistance`，签名基于该完整 URL 计算。预览与正式准备使用相同来源，CLI 的点位样本命令也先读取学校规则距离。
3. 学校距离须明确提供有效的正整数米数，不再以 1000 或 1 猜测。缓存键增加距离，缺少距离的旧缓存失效。

本地回环 HTTP 回归确认路径、整数区域字段、URL 签名固定向量、跨距离缓存隔离及预览采用学校规则距离。完整工作区测试通过 293 项，6 项跳过。测试使用合成学校数据，不证明具体学校的服务端规则。

尚存限制：Android dump 中点位请求取 `getgLng()/getgLat()`，当前工具仍采用其既有 iOS 请求链与 BD 锚点。Android 证据不能单独证明 iOS 坐标契约，因此未据此全局改坐标。跑区实际配置、学校的跑道检查点生成条件和服务端响应范围仍需真实学校数据验证。这些改动也不涉及 GeeGuard 设备验证问题。
