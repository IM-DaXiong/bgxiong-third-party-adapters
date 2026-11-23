# 本地与公开仓库目录约定

2026-11-11。公开仓库与本地 checkout 使用同一套源码目录；构建缓存、用户数据和历史现场不上传 Git。

- `packages/`：当前主线适配器源码。`projects/`：独立工程；不复制到第二个源码根。
- `contract/`：受控 SDK 消费资料。`community/`、`docs/`、`scripts/`：社区、文档和工具。
- `release/`：Git 跟踪的当前发行锁、资源恢复回执、逐包安装 ZIP 下载目录与验收记录。
- `artifacts/current/<pluginId>/<version>/`：唯一当前成品目录，也是以后开发和测试安装、卸载适配包的唯一目录。发行锁只指向这里。每个 pluginId 只保留当前版本。该目录必须包含 `adapter-install.json`。禁止把测试安装包放到缓存、下载、构建目录或 `projects/install-*`。
- `artifacts/cdn/`：与公网 `/client/ai-story/bgxiong-third-party-adapters/` 对齐的唯一上传目录。每个当前可安装包一个 `<pluginId>-<version>.zip`，同层还有 `SHA256SUMS.txt` 和生产签名 `latest.json`。整目录上传即可。
- `artifacts/cdn-index/`：签名工作区（`payload.json`、签名和 `latest.json` 副本）。不是上传目录。
- `artifacts/cache/`：临时构建缓存，不是发行成品，可以清空。
- 不再使用 `artifacts/releases/`、`artifacts/candidates/` 存放当前或历史成品。公开下载 ZIP 仍由 GitHub Releases 分发；本机不再为同一版本另存一套散列目录。
- `records/`：仅本地的历史现场、旧工作树和迁移记录；不作为活动源码或用户安装入口。
- `logs/`、`build/` 和各源码目录 `target/`：本地验证或构建输出，不上传。

新机器需要本地成品时，按 `release/current.json` 把文件放到 `artifacts/current/<pluginId>/<version>/`。用户直接下载单个适配器 ZIP 即可，无需恢复开发全集。托管语音与作者样例不混入通用安装 ZIP。

## 本次目录整理

原子步骤：先核当前发行锁及所有资源摘要、公开下载和远程恢复，再处理以下目录；只移动保全，不删除未确认内容。检查所有目标在本仓内，禁止覆盖已有目标；移动清单保存在本地归档。

1. 根目录迁移脚本、扫描清单与日志移入 `records/20261111-layout-cleanup/root/`。
2. 未被现行锁引用的驱动器名产物目录、旧独立产物和 `current.previous-*` 移入同批 `records/.../artifacts/`。
3. 构建暂存 `_sdk-stage`、`_opensource-sync` 统一到 `artifacts/cache/`。
4. 退役 `sources/` 保全到同批 `records/.../sources/`，其所有内容保留；当前唯一源码仍为 `packages/` 和 `projects/`。
5. Git 中旧的发行二进制副本取消跟踪，现行字节已通过公开完整归档恢复验证；本地冻结文件保留。旧版本仍可通过 Git 历史追溯。
6. 修正独立工程 AGENTS 中过时的 `sources/main/packages` 路径，统一引用当前源码与安装入口。

不移动注册中的工作树，不修改用户安装数据，不清除任务回执或媒体。历史版本和候选不改称当前发行版。

## 发行上传根与服务器逐层对齐（2026-11-21）

第三方适配器的唯一上传交付目录是 `artifacts/cdn/`，测试和生产批次均交到这里。以服务器目标为基准校验相对文件路径、层级和命名。上传到公网生产路径 `/client/ai-story/bgxiong-third-party-adapters/` 的目录同层必须具备发行锁中全部当前 ZIP、`SHA256SUMS.txt` 和生产签名 `latest.json`，包括用于公网人工验收的批次。签名工作区供工具使用，发布流程负责将索引写入上传根。

发布过程中先组装完整候选，再核全量 ZIP 的相对路径及字节、SHA-256 校验表和 `latest.json` 的签名。索引的每一条文件名、版本、大小和摘要必须与候选内的 ZIP 一致，本批获准上线的更新版本必须出现在索引里。同版本已发布的 ZIP 复用原字节。缺件、索引陈旧、待签名或校验未通过时保留上一批完整上传根，当前批标记待交付。完整候选验收后一次切换，禁止只替换 ZIP、拆走正式 JSON 或让用户自行补传其他路径的文件。

**每次发布必须随 ZIP 主动更新 `latest.json`。** 这是[仓库最高级发行规范](../AGENTS.md#最高级发行规范--所有智能体必读用户2026-11-22)。更新受阻时立即报告具体原因和当前交付状态。`python scripts/pack-cdn-directory.py` 只有在本批索引通过验签，且与在线资格及 ZIP 完全一致时，才会生成上传目录。需要先做 ZIP 字节的发行资格验收时，运行 `python scripts/pack-cdn-directory.py --qualification`。验收用 ZIP 和校验表在 `artifacts/cdn-index/qualification/`。取得本批回执后，运行 `scripts/sign-online-index-interactive.ps1` 一次性校验资格、准备签名并生成完整上传根。

## 在线索引（2026-11-19）

- `artifacts/cdn/`：历史已发 ZIP 按 `release/cdn-published.json` 固定原字节复用，逐文件仍须匹配 `release/current.json`，禁止覆盖同版本。生产签名 `latest.json` 与 ZIP、`SHA256SUMS.txt` 同层，整目录对公网。
- `artifacts/cdn-index/`：签名工作区。签完后把 `latest.json` 写入 `artifacts/cdn/`，不另做上传目录。
- `release/online-*.json`：在线资格与序号回执。常规资格登记实际升级或首装证据。用户可明确授权本批先上线后人工验收，授权记录须锁定 ZIP 大小、摘要、包身份和宿主范围。只按此授权开放首装，托管升级资格仍需实测证据。

托管语音资格另登记 `resourceVersion`，发行程序核原始冻结 ZIP、随包准备器、运行时版本、已验宿主版本和升级来源。签名索引在 `dependencies` 中声明 `sdkSpeech` 资源版本。具备相应随包资源的宿主展示安装或更新操作，用户确认后复用已有运行资源或从正式资源 CDN 下载。

ZImage 1.0.3 与 FLUX2 1.0.4 现补齐 `installation/adapter-install.template.json`，与宿主现行 `comfyui.serviceOnly` 合同对齐：安装时取软件已经保存的 Comfy 连接，不预设端口、地址或账号。运行包 manifest/WASM 未改；此变更只补齐此前缺失的安装信封，真实 EXE 安装验收单独记录。

### 2026-11-19 正式在线资格与owner归档续修

- `release/evidence/` 保存已实测升级对的可提交记录及历史来源锁/公开owner回执；仅元数据，不另建旧版安装树。首批只有LTX0.1.0→0.1.2，`newInstall=false`，host精确限定实测产品版本0.6.1119（来自BGX_APP_VERSION/tauri.conf.json，非Cargo包版本）。
- `scripts/archive-current-resources.py --output <临时owner归档ZIP> --receipt <本地候选回执>` 逐文件验证当前锁全部成品并生成聚合归档，在隔离根调用主仓真实restore owner两次验证；不编译、不签名、不上传、不替换正式current-artifact回执。归档文件名使用Git规范LF锁摘要，不能用Windows CRLF工作树摘要命名。ZIP是上传暂存聚合文件，不是第二套可安装目录。
- `scripts/sign-online-index-interactive.ps1` 统一组装 ZIP、签名及验签，并将包含 `latest.json` 的完整上传树切换到 `artifacts/cdn/`。首次用 `-RememberPassword` 在私密终端授权，签名验证通过后由 Windows DPAPI 将密码加密保存在发行账号的用户配置中。以后同一台机器、同一 Windows 账号可直接自动签名。密码仅临时传给签名子进程。密钥或发行账号变更时重新授权。

### 在线发布准备

默认发布准备从 CDN 回读并验签，基于线上序号识别已发布版本，重跑时复用相同候选索引。本地准备使用 `-LocalOnly`，序号以本地签名索引和最近发布回执为锚。要纳入特定首装包，使用 `-RequireNewInstall pluginId@version`，程序会核对首装实测记录与 ZIP 摘要。发布后可用 `--commit-receipt` 记录线上回读。
