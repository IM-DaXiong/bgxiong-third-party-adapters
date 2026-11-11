# 本地与公开仓库目录约定

2026-11-11。公开仓库与本地 checkout 使用同一套源码目录；构建缓存、用户数据和历史现场不上传 Git。

- `packages/`：当前主线适配器源码。`projects/`：独立工程；不复制到第二个源码根。
- `contract/`：受控 SDK 消费资料。`community/`、`docs/`、`scripts/`：社区、文档和工具。
- `release/`：Git 跟踪的当前发行锁、资源恢复回执、逐包安装 ZIP 下载目录与验收记录。
- `artifacts/current/<pluginId>/<version>/`：本地唯一当前安装入口，与对应下载 ZIP 解压后的文件树和摘要完全相同。
- `artifacts/releases/`：发行锁指向的不可变完整文件；从公开发行附件恢复。二进制通过 GitHub Releases 分发，不在 Git 源码中重复堆放。
- `artifacts/downloads/<release-tag>/`：与 GitHub Releases 附件同名、同字节的 ZIP 与 SHA256SUMS。
- `artifacts/candidates/`：尚未冻结的候选。`artifacts/cache/`：临时构建缓存。
- `records/`：仅本地的历史现场、旧工作树和迁移记录；不作为活动源码或用户安装入口。
- `logs/`、`build/` 和各源码目录 `target/`：本地验证或构建输出，不上传。

新机器 clone 后从 `release/current-artifact.json` 下载完整资源归档，按主仓恢复脚本还原 `artifacts/releases/`，再刷新 `artifacts/current/`。用户直接下载单个适配器 ZIP 即可，无需恢复开发全集。托管语音与作者样例不混入通用安装 ZIP。

## 本次目录整理

原子步骤：先核当前发行锁及所有资源摘要、公开下载和远程恢复，再处理以下目录；只移动保全，不删除未确认内容。检查所有目标在本仓内，禁止覆盖已有目标；移动清单保存在本地归档。

1. 根目录迁移脚本、扫描清单与日志移入 `records/20261111-layout-cleanup/root/`。
2. 未被现行锁引用的驱动器名产物目录、旧独立产物和 `current.previous-*` 移入同批 `records/.../artifacts/`。
3. 构建暂存 `_sdk-stage`、`_opensource-sync` 统一到 `artifacts/cache/`。
4. 退役 `sources/` 保全到同批 `records/.../sources/`，其所有内容保留；当前唯一源码仍为 `packages/` 和 `projects/`。
5. Git 中旧的发行二进制副本取消跟踪，现行字节已通过公开完整归档恢复验证；本地冻结文件保留。旧版本仍可通过 Git 历史追溯。
6. 修正独立工程 AGENTS 中过时的 `sources/main/packages` 路径，统一引用当前源码与安装入口。

不移动注册中的工作树，不修改用户安装数据，不清除任务回执或媒体。历史版本和候选不改称当前发行版。
