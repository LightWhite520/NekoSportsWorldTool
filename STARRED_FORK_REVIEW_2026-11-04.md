# 有星标 fork 调研汇总

调研日期：2026-11-04。用途：汇总可复核的实现、重复功能和待验证协议线索；不是已合并清单。

## 范围与证据边界

- GitHub 上游 `YanamiNeko/NekoSportsWorldTool` 当前仍可访问。仓库元数据的 forks_count 为 99；分页 forks API 本次返回 98 项，第二页为空，不能把元数据计数当成已检查数量。
- 返回列表中 9 个仓库有星标，包括本仓库。对有下级 fork 的仓库继续枚举，返回的下级仓库均为 0 星；按用户要求不审阅其源码。
- 检查有星仓库的默认分支文件树、近期提交、分支列表和相关代码；进一步检查检查点修复分支。不是每个历史提交或每个实验分支的完整审计。
- 额外纳入用户已指定、15 星的 `DemaciaForge/DemaciaSportsWorldTool`：它当前是独立仓库（fork=false），不冒充上游 fork 列表中的一项。
- 上游 main 与多个 fork 的 compare API 返回 No common ancestor，因此未使用 ahead/behind 数推断改动量；改看各仓库文件及自身分支差异。
- 未登录业务平台、请求检查点、提交记录或运行外部二进制。未读取 session.json、config_backup.json。所有协议实测说法均明确区分作者声称与本次验证。

## 仓库清单

星数仅为本次查询快照，不代表正确性。

| 仓库 | 星数 | 默认分支 HEAD（短 SHA） | 本次筛查结论 |
|---|---:|---|---|
| [GAN1112233131/NekoSportsWorldTool](https://github.com/GAN1112233131/NekoSportsWorldTool) | 9 | 555c059e | 响应分类、地图下载备用源、详情诊断；部分基础修复本仓库已有 |
| [DemaciaForge/NekoSportsWorldTool](https://github.com/DemaciaForge/NekoSportsWorldTool) | 9 | 9ac21e4c | 区域元数据与嵌套解析；后续工作见独立仓库 |
| [Yunlk/Neko-SportsWorld-Tool](https://github.com/Yunlk/Neko-SportsWorld-Tool) | 6 | 6b677e1b | 默认分支与 qiuxiuya 完全相同；不是两份独立证据 |
| [LightWhite520/NekoSportsWorldTool](https://github.com/LightWhite520/NekoSportsWorldTool) | 5 | 本地基线 fc452f90 | 对照基线，不重复引入自己的功能 |
| [Esvion-008/-NekoSportsWorldTool-](https://github.com/Esvion-008/-NekoSportsWorldTool-) | 5 | f353023d | 新鲜点位获取、GeoJSON 校园导入；有不宜照搬的推断和重试策略 |
| [hb-degithub/NekoSportsWorldTool](https://github.com/hb-degithub/NekoSportsWorldTool) | 3 | 0d24ad26 | 较早版本；城市输入等本仓库已有，点位缓存隔离更弱 |
| [Miempire/NekoSportsWorldTool](https://github.com/Miempire/NekoSportsWorldTool) | 3 | 6227977d | 跑道环识别性能修复值得单独评估；未见新检查点接口 |
| [ConCosew/NekoSportsWorldTool](https://github.com/ConCosew/NekoSportsWorldTool) | 1 | e229abdb | main 含 GAN 的早期修复；额外检查点分支有新的距离参数线索 |
| [qiuxiuya/NekoSportsWorldTool](https://github.com/qiuxiuya/NekoSportsWorldTool) | 1 | 6b677e1b | 较早道路路由、Android 依赖隔离，本仓库已有相应功能 |
| [DemaciaForge/DemaciaSportsWorldTool](https://github.com/DemaciaForge/DemaciaSportsWorldTool)（独立续作） | 15 | b9916c05 | 抓包契约核验工具、区域元数据、业务结果判定 |

## 与当前检查点问题最相关的内容

### 1. ConCosew：接口路径中的 distance 值

来源：[pr/checkpoints-fix，提交 1d8d392](https://github.com/ConCosew/NekoSportsWorldTool/commit/1d8d392131138a51e30659cecd8535a8c59400fb)。此分支相对 main 多 1 个提交，不在该仓库 main 内。

代码把固定 `/api/v560/get/1/distance/1` 改为按目标米数拼路径，预览与运行传入距离，缓存作用域增加 distance_m。注释声称真机抓包见过 `/distance/2000`，并将固定/不全点位归因于写死 1。

证据等级：修改本身已确认；路径尾参数单位、服务端语义及作者的因果解释未独立验证。补丁未附原始抓包，仓库 issues API 本次为空。这是优先核验线索，不能直接宣称它能解决跑道附近触发机制。

若后续通过正常官方 App 请求证实，应同步考虑路径、签名所用 URL、预览与运行一致性、缓存键以及输入校验，不能只替换一个常量。

同一提交还改了 position、窗口 id/queueNum 和末窗步数。这些是不同议题，不宜随路径修复捆绑合并。尤其用时间取模构造 id 不能证明注释所称的跨跑步单调递增；其窗口编号解释与 Demacia 的解释存在差异。

### 2. Demacia：保留点位响应的区域上下文

来源：[points.rs](https://github.com/DemaciaForge/DemaciaSportsWorldTool/blob/b9916c059a3a4874aab562167e1504fc624ebc5d/src/api/points.rs)。

- `PointsContext` 同时保留 points 和 area；从点位响应读取区域和围栏。
- 兼容嵌套 data/result、JSON 字符串，以及明确的点位字段别名。
- 提取点位时排除围栏 points，避免将边界顶点误当成检查点。

建议：借鉴结构化保存原始响应来源、区域字段与解析结果的做法，便于检查旧道路策略和新跑道策略的区别；只接受有明确来源的字段。

不要照搬：把任意 fence.id、areaId、runId 当作 runAreaId。当前本仓库明确拒绝根据围栏 id 推断跑步区域 id；没有该学校响应证据时应保持这一边界。递归搜第一个匹配字段也可能混淆多个区域。

该独立仓库主流程调用 fetch_points_context，内部 run_area_id 为 None，不能当成已经实现跑道区域切换的范例。

### 3. Esvion：新请求不复用旧点位

来源：[points.rs](https://github.com/Esvion-008/-NekoSportsWorldTool-/blob/f353023db169c164230a4e517d1e4f8709085f86/src/api/points.rs)。

- 正常获取不读取磁盘点位缓存，失败不拿旧点位替代。
- 虽留有 SharedPoints 结构和旧注释，实际 fetch_points_shared 已直接调用独立获取，并不共享其他账号的结果。
- 记录点位数量和分类字段，坐标允许数值字符串。

可借鉴：明确区分历史预览缓存和本次新获取结果，显示获取时间、区域与来源。当前本仓库已按账号/学校/区域/位置隔离缓存，但仍可命中 300 秒缓存。

不要照搬：12 次重试、固定长等待、原始点位全文日志，以及把空点位推断成“学期已完成或未到开放时间”。空结果本身不足以证明具体原因。作者注释中关于账号派发和限流范围的说法也不是本次实测结论。

### 4. GAN：区分未配置、空列表、错误和损坏响应

来源：[points.rs](https://github.com/GAN1112233131/NekoSportsWorldTool/blob/555c059efd49389df35d96fca6e8efb9db3f1b77/src/api/points.rs)。

使用 PointResponse::{Points, NotConfigured, Empty}；未配置分支同时匹配错误码及特定提示，列表 JSON 字符串有大小限制，格式损坏和认证错误保持失败。

可借鉴：用于诊断与明确错误提示，避免把接口失败包装成“学校没有点位”。不能据此允许忽略官方要求的检查点。当前本仓库对空列表统一报错，尚无这一显式状态区分。

## 其他值得保留的候选

| 内容 | 来源 | 建议与边界 |
|---|---|---|
| Overpass 失败后切换官方 OSM 小范围下载 | [GAN school_roads.rs](https://github.com/GAN1112233131/NekoSportsWorldTool/blob/555c059efd49389df35d96fca6e8efb9db3f1b77/src/api/school_roads.rs) | 有超时、大小限制和可重试错误分类。可改善可用性，不会凭空补齐 OSM 缺失道路；当前本地该文件未见相同备用实现 |
| 点位与围栏之外的校园手工数据 | [Esvion campus/import.rs](https://github.com/Esvion-008/-NekoSportsWorldTool-/blob/f353023db169c164230a4e517d1e4f8709085f86/src/campus/import.rs) | GeoJSON 导入校园、禁行区、通道、操场等；源码有 WGS84 输入约定及几何验证。属于独立功能，不是自动获取学校检查点的方案 |
| 跑道识别避免无界搜索 | [Miempire ae4de5c](https://github.com/Miempire/NekoSportsWorldTool/commit/ae4de5c3f9d19f2b43c688cd678841d798685f4f) | 平面面遍历配合 DFS 搜索预算，针对复杂图上的长时间搜索。本仓库模块结构不同，不能直接套补丁；先评估现有算法是否有同类问题 |
| 离线 HAR/OBS 一致性核验 | [Demacia 抓包核验文档](https://github.com/DemaciaForge/DemaciaSportsWorldTool/blob/b9916c059a3a4874aab562167e1504fc624ebc5d/docs/CAPTURE_ALIGNMENT_740_zh.md) | 工具与证据分级方法有价值；原始抓包未公开。离线一致性不代表官方接受，更不证明跑道触发策略 |
| 原始详情保留、未知状态、重复提交保护 | [GAN 修复记录](https://github.com/GAN1112233131/NekoSportsWorldTool/blob/555c059efd49389df35d96fca6e8efb9db3f1b77/docs/record-bugfix-release-2026-10-24.md) | 适合逐字段复核差异；当前已有 submission_guard 等基础设施，不应整套重复移植 |

## 不应重复引入或直接采用

- 当前已支持坐标数值字符串、GCJ 字段回退、isFixed 原样保留、缓存作用域隔离、Android rfd 隔离；这些 fork 中出现同类实现不等于本仓库缺功能。
- Yunlk 与 qiuxiuya 默认分支同一 SHA；ConCosew main 含 GAN 早期提交。多个仓库重复相同代码不能作为多份独立实测证据。
- 部分旧 fork 允许接口失败后复用历史缓存，甚至缓存不区分账号/学校。不能倒退本仓库现有隔离与失败处理。
- 不直接采用字段强改、任意点位分类重写、自动跨障碍连边或缺点位时继续运行等行为。
- 文档中“服务端必然如何判定”“完全真实”等描述未经服务端证据支持，不纳入事实结论；测试计数是作者报告，本次未重跑。
- 不恢复刚回滚的多坐标系功能。

## 当前结论与后续核验顺序

尚未找到公开证据完整说明“学校按靠近跑道切换新旧点位策略”。当前最有价值的新线索是 ConCosew 的 distance 路径参数，其次是响应中的区域上下文和点位缓存新鲜度。

建议依次核对：官方 App 的实际检查点 URL 与尾参数；实际定位及 runAreaId；响应区域和点位字段；是否存在额外模式请求；旧缓存是否掩盖新结果。用正常获取流程的脱敏样本验证，避免拿源码注释替代接口证据。

上述为首次调研结论。用户随后授权整合，落实内容见下节；没有推送或发布。

## 补查：非 fork 的有星同名仓库

GitHub 仓库搜索 `NekoSportsWorldTool in:name stars:>0 fork:false` 返回上游及以下 5 个独立衍生仓库。已检查默认分支文件树、近期提交和点位代码，并对相关改动阅读源码或 README；搜索结果不等于所有任意改名仓库的完整清单。

| 仓库 | 星数 | HEAD | 处理 |
|---|---:|---|---|
| [huang3337/NekoSportsWorldTool-Plus](https://github.com/huang3337/NekoSportsWorldTool-Plus) | 2 | 2957c28b | isFixed 透传本仓库已有；不照搬空经过列表回退全量上报。移除 native-certs 会改变系统证书信任，当前没有复现其 TLS 故障，不全局替换 |
| [czcdr/NekoSportsWorldTool-antiban](https://github.com/czcdr/NekoSportsWorldTool-antiban) | 5 | 0c2120ac | 采用 complete 缺失保留未知的处理；distance 路径与 ConCosew 注释相同，不能视为独立抓包证据。不引入基于文本关键词的达标猜测 |
| [ooo12061018/NekoSportsWorldTool](https://github.com/ooo12061018/NekoSportsWorldTool) | 1 | 4f00834c | 新增自由跑模式，但不是学校新检查点策略的证据；不引入跳过点位的运行模式 |
| [fhvbd/NekoSportsWorldTool-SimLocation](https://github.com/fhvbd/NekoSportsWorldTool-SimLocation) | 1 | 093456bd | 地图定位与当前功能重叠；不恢复用户刚回滚的多坐标系选项 |
| [Alan-Tesla/NekoSportsWorldTool-macOS](https://github.com/Alan-Tesla/NekoSportsWorldTool-macOS) | 1 | bf6d6189 | macOS 包装和路线变体为另一条功能线；当前已有统计重建及路线预览，不整套替换运动模型。未在 Mac 上复验 |

## 已授权整合的实现

本次采用有针对性的源码适配，不做整个 fork 的 Git 合并，避免带回旧功能、冲突协议假设或其他运行模式。保留各来源的 CC BY-NC 4.0 署名与许可约定；来源链接见上文。

1. **检查点解析与响应上下文**（参考 GAN / Demacia）
   - 新模块 `src/api/points/response.rs` 支持明确的三种列表字段、data/result 包装、JSON 字符串，限定 8 层及编码文本累计 8 MiB。
   - 保留原始点位；区分未配置、成功空列表、错误和格式损坏。空结果仍中止，不推测学期或开放时间。
   - 多个不同列表/区域冲突时报错；不会递归把围栏顶点当成点位，也不根据通用 id 推断区域。
   - 本地 points_cache 新增可选 response_context，保存列表路径、明确 runAreaId、geoFences 和 freedomShowFence。只用于本地诊断，不改变请求区域或坐标转换。旧缓存兼容。
2. **点位新鲜度**（参考 Esvion，保留本仓库确认流程）
   - 主动强制刷新、未绑定跑道确认的新一次生成重新请求；普通预览仍可使用 300 秒内的作用域缓存。
   - 已确认跑道依赖预览点位快照，沿用有效缓存；过期后若实际点位变化，原有 resolve 校验要求重新确认。避免每次随机分配令确认永远无法使用。
   - 网络失败不回退旧点位，不添加自动批量重试；日志区分缓存与本次获取。
3. **公共地图备用源**（参考 GAN）
   - 学校路网及地图选点下载复用 `src/api/osm_download.rs`。
   - 连接类错误、408/500/502/503/504 后最多切换一次官方 OSM map API；429、认证错误、大小超限不自动重试。
   - 原有解析和缓存验证仍执行；有连接/总超时、响应体大小、备用范围限制，学校任务切换备用前检查取消。
   - 选点备用范围为覆盖原圆形下载范围的矩形，不把下载框当成学校围栏。不会补出 OSM 未收录道路。
   - 公共请求不带业务凭据；错误不回显服务端正文或坐标 URL。
4. **记录“未知”状态**（参考 czcdr）
   - RecordRow、RunDetail、ReasonItem 的 complete 保留 Option<bool>。
   - 手机/桌面列表、详情判定项及 CLI 缺字段时显示未知，不默认未达标，不根据原因文本猜测通过。

### 未移植项

- 未确认的 distance 路径含义、窗口 id/queueNum、区域 id 推断等协议改动。
- Miempire 的最长环搜索修复：本仓库使用唯一端点拼接且设节点上限，没有该版本的无界 DFS；不替换现有严格跑道识别。
- GeoJSON 校园编辑是新增独立子系统，现有 OSM 导入/跑道编辑可用，本次不扩大为整套编辑器迁移。
- 模拟模型、批量账号、自由跑绕过点位、证书信任全局调整。

### 验证

- `cargo test --workspace --offline -- --test-threads=4`：主库 249 passed / 6 ignored；route-planner 单元 12 passed；集成测试 9 + 7 + 3 passed。合计 **280 passed / 0 failed / 6 ignored**。
- `cargo test --no-default-features --lib --offline -- --test-threads=4`：**154 passed / 0 failed**。
- 新测试包括回环 HTTP 强制刷新、原始点位/响应上下文缓存、请求失败不回退、真实 POST→备用 GET、备用坐标顺序、认证头隔离、429 不切换、取消及大小限制，以及手机/桌面 egui 帧内未知状态文本。
- 首次构建遇到共享 target 内另一 checkout 的旧 route-planner 产物；确认当前源码导出存在后，仅清理该包并重建，以上结果来自重建后的当前源码。
- 本次未请求真实学校接口，未登录、提交、抓取用户运行时账号文件；未在 Android/macOS 上构建或启动，也未验证学校两套策略切换。
