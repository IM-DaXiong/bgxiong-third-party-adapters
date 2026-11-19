# 本地与公开仓库目录约定

2026-11-11。公开仓库与本地 checkout 使用同一套源码目录；构建缓存、用户数据和历史现场不上传 Git。

- `packages/`：当前主线适配器源码。`projects/`：独立工程；不复制到第二个源码根。
- `contract/`：受控 SDK 消费资料。`community/`、`docs/`、`scripts/`：社区、文档和工具。
- `release/`：Git 跟踪的当前发行锁、资源恢复回执、逐包安装 ZIP 下载目录与验收记录。
- `artifacts/current/<pluginId>/<version>/`：唯一当前成品目录。发行锁只指向这里。每个 pluginId 只保留当前版本，旧版本成品不保留。安装信封、`package/` 内的 wasm 与清单、LICENSE 和说明使用同一套布局。
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

- `artifacts/cdn/`：只放已核对的 ZIP 与 SHA256SUMS.txt，可整目录上传；历史已发 ZIP 按 `release/cdn-published.json` 固定原字节复用，逐文件仍须匹配 `release/current.json`，禁止覆盖同版本。
- `artifacts/cdn-index/`：独立签名索引，ZIP 上传并回读之后单独上传，不能随 CDN ZIP 目录抢先发布。
- `release/online-*.json`：在线资格与序号回执；资格只登记有实际升级/首装证据的版本。空资格表示尚未取得正式发行资格；本地 S1 不替代开发 EXE/正式验收，也不代表正式索引可发布。

ZImage 1.0.3 与 FLUX2 1.0.4 现补齐 `installation/adapter-install.template.json`，与宿主现行 `comfyui.serviceOnly` 合同对齐：安装时取软件已经保存的 Comfy 连接，不预设端口、地址或账号。运行包 manifest/WASM 未改；此变更只补齐此前缺失的安装信封，真实 EXE 安装验收单独记录。
