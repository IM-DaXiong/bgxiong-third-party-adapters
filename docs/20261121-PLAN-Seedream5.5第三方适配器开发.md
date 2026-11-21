# Seedream 5.5 独立第三方适配包开发计划

文档 ID `PLAN-SEEDREAM-55-THIRD-PARTY-ADAPTER-20261121`。创建于 2026-11-21 18:18:18（上海），同日完成文件级审核修订。18:25 版已将范围纠正为独立新包，本版取代其执行表。**Seedream 5.5 1.0.0 已进入生产 CDN 序列 9，适配 0.6.1119 在线首装。付费生成由用户验收。** 本文件是本主题唯一执行计划。

## 1. 交付目标与授权

从主仓实际 Git 根向上一级定位同级适配器仓和 SDK 仓。下文无前缀的路径相对适配器仓；H 表示主仓，S 表示 SDK 仓，P 表示 `packages/cloud-adapters/seedream-55-image`。

| 原需求 | 执行约定 |
|---|---|
| R01、R02 保存调用指南与全文 | 本地[脱敏调用示例](./20261121-REF-火山方舟Seedream5.5用户提供调用示例.md)和[用户提供的 API 全文](./20261121-REF-火山方舟图片生成API-用户提供全文.md)只读。示例是字段验收输入，用户提示词只用于获准的对照，不进入默认词或日志。 |
| R03、R05 建独立 5.5 包，安装后走适配器生图 | 首发身份定为 `local.bgx.seedream-55-image@1.0.0`，展示名 Seedream 5.5，模型 `doubao-seedream-5-5-261028`。身份冲突回到 A00；不覆盖同版本字节。现有 5.0 包、其安装及发行条目均只读，不把它列为升级来源。 |
| R04 正规复用已有方舟账号 | 用户选卡或在应用账号中心新建卡，独立绑定本包句柄。5.0 安装与否不影响首装。已知接口支持绑定，本机用户是否有可用卡、真实云端权限仍待验证。 |
| R06 SDK 与适配器隔离 | 厂商字段、请求、错误、默认值归 P；宿主负责发现、账号租约、媒体和历史。仅消费该批核定的公开 SDK。通用合同若缺能力，保留最小复现，独立 SDK 计划完成后再接续；宿主发现问题单独列主仓文件和验收，不能扩大本文件改动面。 |
| R07 覆盖完整示例，先规划 | 文生图为必做。原计划 D02 仍是建议：后续明确批准整份计划（含 D02）才加入 1–10 张参考图操作；只批文生图则跳过全部 E 项。本次要求审核不触发 D02。图层拆分、OCR、组图、流式生成、联网、框选、优化提示词与 safety_identifier 均不随首发实现。 |

首发沿原 T02 同时规划 URL 和 b64_json。默认词为空。真实出图另需付费授权；本地 QA 签名索引、线上签名、外部上传及远程回读分别记账。不得根据一次本地检查声称用户已经可以下载。

## 2. 已核实的计划缺口

审核基线为适配器仓 `ee2badffddaa`（当前分支同时含 WAN 工作）、H `072659a9e410`、S `a47e4f5ead94`。两份 REF 和本计划在审核时均为未跟踪文件。以下是源码、文档与发行工具的静态核对，均非运行测试。

| 缺口 | 证据与后果 | 收口位置 |
|---|---|---|
| G01 错用 SDK 基线 | H 的 `resources/adapter-sdk/source-lock.json` 记录发行 2.4.0 和上述 S 提交，Rust SDK crate 为 2.0.0。旧包 template-lock 中的 2.0.0 无法确定本批 CLI、validator、worker。 | A00–A01 |
| G02 预检、回执与控件各有一套解释空间 | S 的 `profiles/operation-input-v1/README.zh-CN.md` 要求冻结选择、输入和环境；`SDK-CONTRACT.zh-CN.md` 限定 appliedParams 只含冻结 input 已有键。原稿允许把省略字段的默认值报作 appliedParams，亦未说明如何录入任意合法像素。 | A02–A05、V1 |
| G03 账号选择与下载权限混用 | H 的 `ui/src/components/settings-tab/AdapterPackageCredentialEditor.tsx` 在无 suffix 时不按账号来源过滤；`src-tauri/src/adapter_host/v2/installation.rs` 的账号源站模式要求认证 origins 为空、endpointBaseUrl 未设置，绑定后写入纯 origin。原稿的固定 API 根加 suffix 无法按此模式安装；照搬旧包 origins 还会给无关域 Bearer 权限。 | A04、A06、V3 |
| G04 厂商像素上限、透明验证和资源边界未闭合 | 本地全文 111–115、193–205、约 740 行分别规定参考图、输出尺寸、实际透明像素。旧包 `src/lib.rs::read` 限单对象 32 MiB，`image_mime` 检查签名；多个参考图转 Base64 后不能据单图检查推断整请求可提交。单看 PNG 文件头或存在 alpha 通道会把全不透明图算成透明成功。 | A03–A05、E1、V2 |
| G05 已计费请求及部分失败缺少可验证的恢复边界 | 旧包 `src/lib.rs::submit` 与 README 已区分原始响应、media_chunk、usage 和终态。原稿没把下载失败后的 POST 次数、事件与历史去重列入验收。 | A05、V2、A08 |
| G06 首装、更新与发行顺序混淆 | `scripts/publish-online-index.py` 校验首装证据精确绑定 ZIP；`scripts/stage-cdn-upload-directory.py` 验签并检查 newInstall。H 的 `real_online_first_install` 末段计数 video_model_ids；把期望视频数设 0 无法验证图片模型发现。原稿让在线阶段先于付费实测，且允许失败即删除候选源码。 | A07–A10 |

## 3. 冻结合同

- 两个操作均用 capabilityKey `image.generate`、transportSlot `image`、inputProfile `operation-input-v1`。文生 localId `image`、operationId `local.bgx.seedream-55-image/generate`、operationVersion 1；E 生效才有 localId `image-edit` 和同包 `/edit` 版本 1。操作身份由选择快照传入，绝不按参考图数量推断。同步提交，厂商 HTTP 固定 `stream:false`；SDK stream-events 专供本地成果/用量事件。无远端 query/cancel、无可恢复任务 ID。
- 输入对象闭合。必填 `prompt` 为非空白字符串，原样提交；`size` 默认 `2K`，接收 `1K|1.5K|2K` 或正十进制整数 `宽x高`（小写 ASCII x）。以防溢出的整数运算验证乘积在 [921600,4624220] 且宽高比在 [1/16,16]，端点含等号。不能把推荐边长当上限。其余字段见下表。显式 null、类型不符、未知键均拒绝，布尔 false 不得回填为 true。
- 默认 generate 无素材槽、无 referenceImages 属性，submit 的媒体集合必须为空。E 的 edit 必填 `referenceImages`，元素仅含非空 `mediaId`，顺序即提交顺序；槽 `role=reference`、`inputPointer=/referenceImages`、`mediaKind=image`、1–10 个。禁止输入本机路径、任意外链、凭证或厂商原始请求。两操作各自产生实际 schema digest，按 SDK 规则归一 LF 后发布，probe 与 submit 钉同一身份。
- 只有宿主注入 Bearer。唯一付费请求为 POST 到 `https://ark.cn-beijing.volces.com/api/v3/images/generations`。模型、stream 均为包内常量，输入里出现同名键即拒绝；示例中的 false 由映射产生。文生省略 image，edit 单参考为原图 data URI 字符串，多参考为有序数组。

| 输入键 | 控件与默认值 | 厂商请求键 |
|---|---|---|
| prompt | multiline，角色 prompt，无预填词 | prompt |
| size | text，角色 size，允许档位和合法像素；默认 2K | size |
| responseFormat | select，url（默认）或 b64_json | response_format |
| outputFormat | select，png（默认）或 jpeg | output_format |
| background | select，auto（默认）、transparent、opaque；transparent 与 jpeg 预检拒绝 | background |
| watermark | toggle，默认 true | watermark |

## 4. 文件级执行顺序

每项只改所列文件；生成文件由指定工具产生。跨文件合同按表内顺序闭合后才出阶段回执。执行状态见 §7。文件冲突、新字段、新依赖超出给定职责或路径缺失时，先把差异和受影响步骤写回本计划；不自行改宿主、更新 SDK、改旧包或新建平行实现。

| 编号与前置 | 唯一写入位置 | 单项动作与通过条件 |
|---|---|---|
| A00 开工授权 | 本计划的执行记录 | 记录授权范围（T2I 或含 E）、机器角色、三仓实际根/分支/HEAD/脏文件、H 的 SDK 锁和作者 SDK/CLI/worker 摘要。修复或编译前按项目规则同步 H、核最新 SDK，锁定本批已对齐输入。核 P、新 ID、current、线上历史无占用。已有同批半成品先核记录后续做；占用不明、锁漂移或待合并时停在此项。 |
| A01 ← A00 | P/Cargo.toml、P/Cargo.lock、P/template-lock.json、P/vendor-sdk/ | 按 S/AUTHORING.zh-CN.md 用 init 创建尚不存在的 P。crate 名 bgx-seedream-55-image，版本 1.0.0，独立 cdylib；依赖包内封存镜像，禁止跨包 path。Cargo.lock 可由作者工具按实际依赖维护；template-lock 与 vendor-sdk 按生成清单逐文件校验，禁止手改。记录 toolchain、wasm target、构建回执。 |
| A02 ← A01 | P/schemas/input.json、P/schemas/output.json；获准 E 才新增 P/schemas/edit-input.json | 按 §3 声明闭集、默认和必填。输出 schema 仅验证 completed.data，对齐 model、providerRequestId（可空）、imageCount（正整数）、reportedUsageJson（可空字符串）；media 和 usage 按 SDK 外层合同。两输入公共字段逐键等价。 |
| A03 ← A02 | P/src/parameters.rs | 单一解析器返回已验证业务参数，供 probe、submit 共用；检查选择四元组、环境和 §3，错误返回包命名空间 code 与具体 fieldPointer。probe 只用元数据并回传 requestDigest；SDK 封装 preparation，submit 核冻结 input、环境摘要和 material。缺失/失效准备在 POST 前拒绝。appliedParams 只投影冻结 input 中存在且证实已应用的字段，省略值即使进了 HTTP 也不补报。 |
| A04 ← A03 | P/src/lib.rs、P/manifest.json | 在 capabilities 注册 §3 的操作、uiSchema 指针、素材/成果槽、input/config/output schema 及 digest；manifest 声明协议 2、bearer、stream-events-v1、operation-input-preflight-v2；artifactSha256 由 build 更新。按绑定后的规范 origin 组装一次 /api/v3/images/generations，只允许指定北京 HTTPS origin；禁从提示词或 input 覆盖端点。图片输入/HTTP body 与响应经 host-media/host-http 受管句柄，限制单对象、总字节和耗时，超额显式拒绝。记录实现常量及其与实际 WASM/宿主上限的关系。 |
| E1 ← A04，含 D02 才执行 | P/src/parameters.rs、P/src/lib.rs、P/schemas/edit-input.json | 按序将每个 ID 配到唯一 image 租约；缺、重、额外租约或 MIME 与原件不符均拒绝。按全文 111–115 检查单图格式、两边 >14、比例、单图 ≤30 MB 和 ≤36M 像素，预检仅用可信已有元数据，缺少的字节事实在 submit、POST 前查实。逐图预估 Base64 长度和 JSON/总请求预算，越界不得截断或只取首图。README 明列可验证格式和实测资源上限，无法覆盖已批准范围时记缺口，不静默收窄。 |
| A05 ← A04 | P/src/results.rs、P/src/lib.rs | results 负责解析 HTTP/逐项错误、受管原始响应、url/b64 图片、原始 usage 与输出事件；lib 接入唯一 submit。响应原文作为受控 provider-response 文件，可能含签名 URL，不进日志/公开证据。领取 URL 只允许确定的 HTTPS 下载源，GET credential=null、generatedHeaders=[]，遇重定向即报下载失败（S/crates/reference-host/src/http.rs 使用 Policy::none），不向 Location 续发。校验实际格式、可解码性和透明像素，原件不转码；usage 零值保留、缺失不估。先保全响应与已完成原件，逐项错误不吞掉同响应其他成果，最终错误不报 completed。 |
| A06 ← A01，与 A04 联验 | P/installation/adapter-install.template.json、P/README.md | 走宿主已有账号源站模式：authentication.source=userInput，hostConfig.credentials=[]，唯一 bearer 字段 reference=seedream-55-image-user-key、accountOriginHostSuffix=ark.cn-beijing.volces.com、字段 origins=[]；hostConfig.configuration 保留同一 credentialRef，省略 endpointBaseUrl，network.origins 初始只含有证据的下载源，allowPrivate=false，allowEmbeddedAuthorization=false。宿主绑定时加账号 origin 并写入不带 /api/v3 的 endpointBaseUrl；A04 再核完整 origin，防止子域匹配被扩大。bindingId=seedream-55-image-manual，configurationRef=seedream-55-image-v1。README 说明选卡/新建、首发范围、URL 24 小时、资源限制、失败语义。未证实的结果域记“待实测”，不得把示例输入图域当下载授权。 |
| A07 ← A02–A06（E 按范围） | P/verify_http.py、P/src 下上述模块的测试部分 | 落地 §5 夹具。用假 Host 检查生产映射和失败边界，用正式 CLI/worker 对生成 WASM 做 check、caps、probe 和失效准备检查。fixture 与生产信封分开；若离线环境无法对同一包完成 HTTP 通路，记录验证止于哪一层；不能在发版代码增加本地端点开关。 |
| A08 ← A07 通过 | artifacts/current/local.bgx.seedream-55-image/1.0.0/ 与 release/current.json 中该身份一项 | 按 §6 组装安装树、逐文件 SHA256 登记，校验实际 pack 和安装树 package/。在隔离数据根以真实 EXE 完成 §5 V3。若非本批文件发生并行变化，合并其他包条目后重新核本包，不覆盖整份锁。 |
| A09 ← A08，另获付费授权 | 本计划执行记录，脱敏证据路径 | 实机按用户示例提交一次，核 POST 次数、request id、工作台历史图、原件摘要、透明像素和 usage；图生已纳入且获对应调用授权才加一笔单参考。失败先保全原响应，不能为补证重生。新 CDN 源须先核归属、更新信封、复验权限及原成果下载；已封存版本按升版处理。 |
| A10 ← A08，发行前核 A09 状态 | release/online-releases.json、release/evidence/seedream-55-image-1.0.0-first-install.json、artifacts/cdn/；公开回读后才更新 release/cdn-published.json | 按 §6 跑精确 ZIP 首装、签名与回读流程。在线首装与 A09 无先后依赖，缺真机成功记录只报对应已验层。首发 upgradeFrom=[]；不能创建虚假旧版做更新测试。 |

**按文件交接的检查点。** 同一行列出的文件服务于同一个可验证行为。交接时按以下顺序打勾并保留对应回执，不能只勾“整包完成”。

- A01.1 先 init 并核封存文件 → A01.2 只改 Cargo.toml 的作者元数据及所用依赖 → A01.3 工具生成匹配的 Cargo.lock → A01.4 校验 vendor-sdk 各文件与 template-lock 的来源/目标摘要。Rust 1.95.0、wasm32-wasip2 以本批 S/rust-toolchain.toml 核实；镜像漂移停在 init。
- A02.1 写 input.json → A02.2 仅 E 生效时写 edit-input.json → A02.3 写 output.json → A03.1 在 parameters.rs 为有效值、显式 false、非法组合和选择失效分别断言。不在 schema 上设截断行为。
- A04.1 manifest.json 声明最小所需权限，生产拒绝私网及内嵌认证 → A04.2 lib.rs 注册 caps 和配置 schema（未绑时允许缺 endpointBaseUrl，已绑提交须等于北京 origin）→ A04.3 同文件组装 host-http POST → A05.1 results.rs 生成 media 与 usage → A05.2 lib.rs 发单一终态。成果槽为 artifact/image（1 至多份，用于保全实收项）、provider-response/file（恰一份），与 completed.data 分开校验。
- A06.1 先生成无凭证的模板 → A06.2 对照 H 的账号编辑器核已有卡按 ID 绑定、新卡 resolve 及失败时 rollbackCreate；新建卡 baseUrl 填完整方舟 API 根，账号去重还比较鉴权方式、规范化服务地址、密钥及相关 extra，不能保证同一 key 一定合卡 → A06.3 README 写清用户实际安装与选卡步骤。
- A07.1 在参数和结果所属 Rust 模块用内存 Host 驱动实际 adapter 调用，检查付费 POST 计数、目标、Body、无凭证 GET、原件和错误；运行包目录的 `cargo test --locked --lib`。A07.2 verify_http.py 使用下节五个绝对路径参数、专用全新证据目录和非秘密测试值，执行 CLI check/caps/probe，保存退出码与实际回执。A07.3 核闭集 schema 并在 submit 前更改绑定/输入，验证准备失效。A07.4 分开登记 Rust 业务夹具、真实 worker 运行、宿主工作台和真实厂商各层结果。没有付费授权时不调用厂商。
- A08.1 在唯一 current 树核运行包 → A08.2 按当前字节生成安装信封/附属文件 → A08.3 将七个文件及其摘要登记到 release/current.json 中唯一新增条目 → A08.4 从该目录安装实测，逐项填 V3。A10.1 完整 ZIP → A10.2 隔离签名 QA 首装与图片登记 → A10.3 写证据 JSON → A10.4 新增 release/online-releases.json 对应条目 → A10.5 验生产签名索引及远程字节 → A10.6 公开成功后按发布脚本回写收据。每次封包变化，旧 ZIP 的首装资格立即失效。

## 5. 可判定的验收

**V1 合同与请求。** 对实际 WASM 枚举 caps，核对包 ID/version、模型、操作版本、默认、操作数、所有 pointer 和 LF digest。空白 prompt、显式 null、错误类型、透明 JPEG、未知 model/stream 键和旧包选择均应 POST=0。接受 1280x720、2880x1440 和 3840x240（比例 16），拒绝 512x512、4096x4096、4000x240 及溢出输入。用录制 Host 请求逐键比对脱敏 cURL 的八个 body 字段；再测默认、watermark=false、jpeg+opaque、b64_json 与省略键的 appliedParams。分别更换 input、素材事实、账号环境或包身份，旧 preparation 不能发 HTTP。E 关闭时无 edit、无参考槽；E 开启时测 0/1/2/10/11、顺序、重复、MIME 不符及超总预算，所有拒绝案例零 POST。

**V2 结果与失败。** 夹具含有效 URL PNG、b64 PNG、JPEG、截断图片、伪 MIME、坏 Base64、全不透明 RGBA 和真正含 alpha<255 的 PNG。透明示例须解码落地原件，至少一个像素 alpha<255。校验保存字节摘要相同；下载拒绝/断开、空 data、逐项 error、401、429、内容拒绝、POST 发出后断连均不得报成功或自动再次 POST。提交受理不确定返回 ACCEPTANCE_UNKNOWN；已经拿到结果而下载失败保留 request id、原响应与已有成果，仍在有效期内可对原件重新取回；无法取回就记录失败。无远端取消/续查能力，本地中断不声称云端已取消。按 media.id 合并事件和终态，usage 快照不累加。私密 URL 查询串、Bearer、原始提示词、Base64、用户媒体不得出现在公开记录。

**V3 实际宿主。** 从 A08 唯一安装树进入隔离应用，记录 EXE 摘要、数据根、package/WASM 摘要和 caps；在图片工作台选到文生操作，检查输出参数可编辑、自定义 size 可提交，切换模型恢复对应记忆，已有节点配置优先；E 获准再查编辑操作及有序素材槽。已有方舟测试卡选 credentialId；新卡通过 resolve 建立或命中，绑定失败回滚新建卡，不删已存在卡。验证错误域、撤销后提交受阻、用户更新同卡密钥按现行绑定生效，下载不携带 Bearer。隔离卸载/重装，5.0 条目和共享卡仍在；有当前引用/未完任务时尊重宿主卸载检查。未授权云端时只能记录绑定、发现与本地预检。任务完成后再打开画布须读到本地原件和历史。

## 6. 构建、发行与停工

1. A01/A07 复用 S/AUTHORING.zh-CN.md §2 的实际 CLI。下列变量均为本批绝对路径：Cli 为作者/运行工具，Sdk 为匹配命令所用公开 SDK，Worker 为匹配的 worker，P 为作者目录，Package 为此前不存在的 pack 输出。每条检查退出码并记录实际命令；首次显式取依赖，再离线复验。pack 中间目录不是安装入口。

   ```powershell
   & $Cli init --sdk $Sdk --dir $P
   # 完成 A02–A06 后继续
   & $Cli build --sdk $Sdk --project $P
   & $Cli build --sdk $Sdk --project $P --offline
   & $Cli check --sdk $Sdk --worker $Worker --project $P
   & $Cli pack --sdk $Sdk --project $P --out $Package
   & $Cli check --sdk $Sdk --worker $Worker --project $Package
   python "$P/verify_http.py" --cli $Cli --sdk $Sdk --worker $Worker --package $Package --output $FreshEvidence
   ```

2. A08 安装树精确清单为 `adapter-install.json`、`package/manifest.json`、`package/manifest.sha256`、`package/adapter.wasm`、适配器仓 `LICENSE`、`README.txt`、`安装说明.txt`（后两项由本包说明生成）。信封 schemaVersion=1、packageDirectory=package；manifestSha256 为 package/manifest.json 文件原始字节的 SHA-256 小写十六进制（H 的 installation.rs::resolve_with），与 package/manifest.sha256 清单文件自身的摘要分别核验；不得从 5.0 复制散列。release/current.json 的 sourceManifest、kind=installable、文件 path/source/sha256 与实物一一对应。唯一测试安装入口在 artifacts/current。失败保留作者源码、最后已验制品和证据；只撤销本批未生效登记，不清理其他包。
3. A10 在发行分工范围内运行 `python scripts/pack-cdn-directory.py --check` 后运行同脚本无参数生成完整平铺目录与 SHA256SUMS.txt。比对 ZIP 解压文件集合、每项摘要、字节数与锁。脚本会重建整个 CDN 目录，已有 latest.json 可能只是沿用旧索引；其他包资源不足时停止发行阶段，不删除其锁项。
4. 在隔离宿主用测试签名索引和该 ZIP 完成首装，核出现 `Install` 候选、登记的是新包的 `image.generate`、目标 modelId 和操作 ID，再次检查不再提供同一候选。H 的 `src-tauri/src/comfyui/workflow/adapter_online_update/apply.rs::real_online_first_install` 可验证安装链，须另从已装目录/真实图片工作台读回模型；“0 个视频模型”不能证明图片可用。回执记 newInstallQualified、exitCode、pluginId、version、targetZipSha256、targetZipBytes 与实际测试宿主。缺证时停止首发资格登记。
5. 登记实测 hostVersionRange、newInstall=true、upgradeFrom=[] 和上述 evidence 路径。获签名授权后按 `scripts/publish-online-index.py --prepare-upload --require-new-install local.bgx.seedream-55-image@1.0.0`，用 `scripts/stage-cdn-upload-directory.py --check --require-new-install local.bgx.seedream-55-image@1.0.0` 复核。发布人员先上传包再发布签名索引，实际远程下载核 SHA256、大小、完整解包及候选后才记已公开。按项目规则核适配器源码远程可得、发布完整 ZIP 与摘要，主仓同步本批安装和验收事实。撤回走更高 sequence 的授权索引；已发布字节、历史任务、旧包和账号均保全。
6. 预算为建议的停止界限：每阶段合同与离线验证、本地安装、首装发布验证分别以 2 小时、1 小时、1 小时为一次排查窗口，每项正式失败最多一次有根因的定向修正；真机以批准调用次数为硬限。达到边界在本表登记最小复现、最后通过项、阻断 owner 和恢复条件。SDK/宿主缺口、真实回归、同版本不同字节立即停受影响步骤。非整机发行不跑全仓门禁。

## 7. 实施与验收记录 · 2026-11-21

用户已批准完整实施（含 E1），付费出图由用户执行。开发机角色为 RELEASE-HEAVY-01，机器 DESKTOP-4OVG8RB。开工时 H/适配器/S 分别为 `072659a9e410`、`ee2badffddaa`、`a47e4f5ead94`，三仓均已抓取并核最新远程。H 的同期业务变更随后正常合入 `d8f57f9e8e`，本包无宿主代码变更。独立包源码提交 `36b39158d72950f130babebd6c39eacbf158afb7` 已推送 `codex/seedream-55-image`，远程同提交。发行锁和 QA 资格已随生产回执同步，原始 REF 留存本机。

- **A00–A01 作者环境。** 实际 SDK 2.4.0、Rust 1.95.0、wasm32-wasip2。SDK 根及同提交的 Debug 导出首试 init 报 `INVALID_TEMPLATE`，`sync_templates.py --check` 确认上游模板锁滞后。固定同一个 SDK 提交，在隔离工作树用 `tools/sync_templates.py --write` 生成模板后 init 成功。SDK 真源与宿主消费锁未修改。CLI SHA-256 `e6e3a90c28d573073a7f1bd1ddae648df6a0e4d9c6c237ae8c5d184d1e752ec2`，worker SHA-256 `7db395fe0c60c2d76340cee36843e593fe70cf4efe2b7d3a9170b98ac606f95e`。作者命令使用隔离 SDK 源，check/caps/probe 使用主仓 `build/dev/adapter-sdk-v2` 作为运行 SDK 根。工具拒绝位于 SDK 根之外的 worker，按合同改用运行 SDK 内的同批 worker 后校验通过。
- **A02–A06 与 E1 已实现。** 两项操作均采用 operation-input-v1。文生无素材槽，图生按序接收 1–10 个独立原图租约。请求最多 401,100,000 字节，单张参考图最多 30,000,000 字节；结果图片最多 32 MiB、原始响应最多 48 MiB。首次 Wasm 编译发现标准库解码器与 SDK guest panic handler 冲突，改用 no_std PNG/JPEG 解码和宿主已解码测量的参考图尺寸。允许无凭证访问的唯一下载域由[火山官方的 Seedream 下载源说明](https://docs.volcengine.com/docs/station-wide-acceleration/accelerate-large-model-generated-content-download?lang=en)证实。真实 5.5 结果 URL 路径及账号云端模型权限待用户验证。
- **参考图格式边界。** 适配器接受方舟所列的原图类型及宿主租约事实。当前图片工作台 `H/src-tauri/src/adapter_host/v2/installed_image.rs::prepare` 经 `image_format_sniff::sniff_image_ext` 只允许 PNG、JPEG、GIF、WebP。BMP、TIFF、HEIC、HEIF 会被现有通用入口拒绝。此限制发生在新包执行前。通用媒体入口扩展涉及主程序和解码范围，应单独立项。
- **A07 离线验收通过。** `cargo +1.95.0 test --locked --lib` 8 组通过，覆盖完整请求、水印 false、透明像素、JPEG/base64、1/2/10 张顺序、坏租约、越界参数、下载授权、部分成果和单次 POST。SDK 作者 build、--offline、check、pack 均成功。`verify_http.py` 对实际 pack 的 Wasm 跑 8 项能力及预检检查，通过，并另断言换提示词的提交返回 `PREPARATION_STALE`。SDK 验证配置无网络授权，厂商 POST 为 0。请求/结果夹具通过 Rust SDK Host 接口注入，完整线上 HTTPS 请求及应用历史由用户付费验收。
- **A08 成品和现有宿主。** 唯一安装目录为 `artifacts/current/local.bgx.seedream-55-image/1.0.0/`，信封原始字节与包清单逐项校验通过。WASM SHA-256 `80f6c880c431365debd9999c1b78907e7afac710854e480777066b0d5d6881c9`。0.6.1119 宿主测试二进制运行 17 项定向用例通过，覆盖图片注册、素材顺序、成果历史去重、账户复用和凭证域；`npx --no-install vitest run src/components/image-workbench-shell/ImageAdapterControls.test.tsx` 18 项通过。桌面现有实例由用户使用，真实 GUI 选卡、撤销与新包卸载操作尚未实测。
- **A09 付费验收由用户执行。** 本机未配置真实凭据、未发服务商请求。验收时选文生图，背景透明、PNG、水印开启、URL 响应；再选参考图生图。成功回执需要工作台历史中的落地原件和真实用量。
- **A10 QA 首装通过，生产索引待发布。** `pack-cdn-directory.py --check` 校验 18 个可安装包，随后生成完整本地 CDN ZIP 目录与摘要。5.5 ZIP SHA-256 `afc4bf9288fb023efe0c433da0f5f7946b9cccbafb26b50cbd0119b8643cf7a4`，321721 字节。H `scripts/test-adapter-online-first-install.py --video-models 0 --test-executable <本机宿主测试二进制>` 使用 loopback 签名索引通过；在隔离 install.db 中解码实际登记的模型选择，核实 image 与 imageToImage 分别写入两项操作及各自 schema digest。证据见 [本地首装回执](../release/evidence/seedream-55-image-1.0.0-first-install.json)。已登记本地首装资格，`publish-online-index.py --check` 验 4 个在线条目通过。`stage-cdn-upload-directory.py --check --require-new-install local.bgx.seedream-55-image@1.0.0` 退出 2，生产索引序列 8 未包含本包。本地旧回执为序列 7。生产签名、上传和公网字节回读待发布阶段。

主仓交接清单已更新并推送，远程 main 核对到提交 6fa6496ac035fc3d2e8fd128ba0223b1ff829596。

## 8. 生产 CDN 发布回执 · 2026-11-21

用户在私密终端完成正式索引签名，并自行上传 CDN。正式索引序列 9，Seedream 5.5 1.0.0 的 `newInstall=true`、`hostVersionRange==0.6.1119`。现有其他 3 个在线条目一并保留。上传目录 `artifacts/cdn/` 含 18 个 ZIP、SHA256SUMS.txt 和生产签名 latest.json。

回读公网索引与本地生产索引字节一致。公网 ZIP 321721 字节，SHA-256 `afc4bf9288fb023efe0c433da0f5f7946b9cccbafb26b50cbd0119b8643cf7a4`，安装清单全部 7 个文件在真实下载 ZIP 中逐项与发行锁及本地成品摘要一致。阶段校验显示本地签名、回执及公网序列均为 9。 [公网索引](https://r.bgxiong.com/client/ai-story/bgxiong-third-party-adapters/latest.json) · [公网 ZIP](https://r.bgxiong.com/client/ai-story/bgxiong-third-party-adapters/local.bgx.seedream-55-image-1.0.0.zip) · [首装与回读回执](../release/evidence/seedream-55-image-1.0.0-first-install.json)

适配器仓发布回执提交 `2de2e465226aa05a5e0801046a207a03cd00b929`，已核对远程 `codex/seedream-55-image`。用户付费出图保持待验。