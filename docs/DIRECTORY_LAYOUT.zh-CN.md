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

## 在线索引（2026-11-19）

- `artifacts/cdn/`：历史已发 ZIP 按 `release/cdn-published.json` 固定原字节复用，逐文件仍须匹配 `release/current.json`，禁止覆盖同版本。生产签名 `latest.json` 与 ZIP、`SHA256SUMS.txt` 同层，整目录对公网。
- `artifacts/cdn-index/`：签名工作区。签完后把 `latest.json` 写入 `artifacts/cdn/`，不另做上传目录。
- `release/online-*.json`：在线资格与序号回执；资格只登记有实际升级/首装证据的版本。空资格表示尚未取得正式发行资格；本地 S1 不替代开发 EXE/正式验收，也不代表正式索引可发布。

ZImage 1.0.3 与 FLUX2 1.0.4 现补齐 `installation/adapter-install.template.json`，与宿主现行 `comfyui.serviceOnly` 合同对齐：安装时取软件已经保存的 Comfy 连接，不预设端口、地址或账号。运行包 manifest/WASM 未改；此变更只补齐此前缺失的安装信封，真实 EXE 安装验收单独记录。

### 2026-11-19 正式在线资格与owner归档续修

- `release/evidence/` 保存已实测升级对的可提交记录及历史来源锁/公开owner回执；仅元数据，不另建旧版安装树。首批只有LTX0.1.0→0.1.2，`newInstall=false`，host精确限定实测产品版本0.6.1119（来自BGX_APP_VERSION/tauri.conf.json，非Cargo包版本）。
- `scripts/archive-current-resources.py --output <临时owner归档ZIP> --receipt <本地候选回执>` 逐文件验证当前锁全部成品并生成聚合归档，在隔离根调用主仓真实restore owner两次验证；不编译、不签名、不上传、不替换正式current-artifact回执。归档文件名使用Git规范LF锁摘要，不能用Windows CRLF工作树摘要命名。ZIP是上传暂存聚合文件，不是第二套可安装目录。
- `scripts/sign-online-index-interactive.ps1` 一次完成打包、验签，并把生产签名 `latest.json` 写入 `artifacts/cdn/`。已有可复用索引时直接完成，新索引需要在私密终端隐藏输入签名密码。密码仅传给签名子进程的临时环境。使用已配置签名环境的发行任务可直接调用 `publish-online-index.py --prepare-upload`。上传目录就是 `artifacts/cdn/`。

### 在线发布准备

准备程序从 CDN 回读并验签，基于线上序号识别已发布版本，重跑时复用相同候选索引。要纳入特定首装包，使用 `-RequireNewInstall pluginId@version`，程序会核对首装实测记录与 ZIP 摘要。发布后可用 `--commit-receipt` 记录线上回读；下次准备直接核对线上索引。
