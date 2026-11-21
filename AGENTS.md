# 比格熊第三方适配器统一规范

- **源码与成品各只留一处（用户2026-11-19）**：当前适配器源码只放 `packages/<分组>/<包名>`，一个包一个目录，不在 `projects/`、`artifacts/` 或发行哈希目录再放一份源码。可安装成品只放 `artifacts/current/<pluginId>/<version>/`，文件树与 `release/current.json` 指向的字节一致，布局统一为安装信封、`package/manifest.json`、`package/manifest.sha256`、`package/adapter.wasm`，以及该包实际需要的 LICENSE、说明和伴随文件。每个 pluginId 只保留发行锁里的当前版本。旧版本成品、候选目录和已经搬进 `artifacts/current` 的散列发行副本直接删除，不另留平行成品目录。本地测试安装和卸载只使用 `artifacts/current/<pluginId>/<version>/`，该目录里有 `adapter-install.json`。不要再从 `artifacts/cache`、`artifacts/downloads`、`build/` 或 `projects/install-*` 拿安装包。

- **本地与公开目录一致（用户2026-11-11）**：遵循 `docs/DIRECTORY_LAYOUT.zh-CN.md`。源码在 packages/projects，发行锁与下载目录在 release；完整二进制通过 Releases 分发并可远程恢复，artifacts/current 与安装 ZIP 解压树一致。旧根迁移材料、退役 sources 及重复产物入口已保全到 records/20261111-layout-cleanup，禁止重新创建活动 sources/main。不得把 sourceExample 或托管运行时说明文件冒充通用可安装 ZIP。

- **四仓平级与 SDK（用户2026-11-10）**：先从同级主仓 `../bgxiong-ai-story` 定位同一开发根。四个 checkout 固定为 bgxiong-ai-story、bgxiong-third-party-adapters、bgxiong-adapter-sdk、bgxiong-content-skills。SDK 远程是 `https://github.com/IM-DaXiong/bgxiong-adapter-sdk.git`，2026-11-11 起该仓公开，说明文档采用其根目录 LICENSE 的 MIT 许可。本仓许可仍见仓库 LICENSE，不是 MIT。主程序需求驱动更新，消费和导出使用该批最新已核定的 SDK 回执。缺仓从登记远程恢复，禁止创建替代真源。创作 Skill（含 H3）仍只在内容仓维护。见[四仓规范](../bgxiong-ai-story/docs/dev/guides/20261107-GUIDE-三仓平级与创作Skill唯一治理目录.md)。

用户2026-11-05明确：唯一工程名、开发目录、Git仓名及用户可见分发总目录名均为bgxiong-third-party-adapters；中文名比格熊第三方适配器。

- 先发现主仓H，使用同级H/../bgxiong-third-party-adapters。本机L:/00Dev/bgxiong-third-party-adapters。公开远程https://github.com/IM-DaXiong/bgxiong-third-party-adapters，禁止再建平行开发真源。
- 本根已承接原bgxiong-vendor-adapters的Git历史、合同、源码和未提交修改，并合入原sources/main/packages独有包。packages是合并后主线源码；projects是独立项目。旧sources/main不是活动owner，不得再次在其中开发。
- artifacts存标准安装包，records存历史保全；禁止按最大版本覆盖源码，禁止同版本覆盖不同包内容。每包唯一owner，分支独有修改先保全合并。
- **本机安装测试固定入口（用户2026-11-09，2026-11-19收口）**。以后开发和测试产出的可安装包只放到 `artifacts/current/<pluginId>/<version>/`，目录内有 `adapter-install.json`。软件安装、卸载都选这个目录。每包只留 `release/current.json` 里的当前版本。禁止再把测试安装包放到 `artifacts/releases`、`artifacts/cache`、`artifacts/downloads`、`build/` 或 `projects/install-*`。发行上传目录是 `artifacts/cdn/`：每个当前包一个 `<pluginId>-<version>.zip`，平铺、只留最新版本。上传 CDN 或网盘时拖走整个目录。主仓打发行包时自动生成，打发行包的智能体必须留下整理好的目录，禁止让用户自己运行或整理。
- 发行逐包核源码版本、已验证产物、安装信封、companion、SDK、license和SHA256，并解包实际EXE核对。WIP/教学材料不冒充已发行可用包；缺项明确列阻断，不静默打旧包。
- 用户安装不依赖我方开发路径。当前引用、历史任务、冻结合同及卸载由统一安装/恢复owner管理，禁止直接删运行包或UPDATE用户库绕过校验。
- 厂商逻辑归独立包，SDK/宿主通用规则归公共owner。公开同步不带密钥、模型、用户媒体、运行库和本机记录；合并目录不构成把未审查项目全量公开的理由。
- 禁强推、reset --hard、覆盖未提交修改及改写已推送历史；本地提交后按用户/主仓授权同步并核对远程。
- 全部智能体开工先读本文件。目录迁移的本机保全和主仓计划不在本公开仓库，本次未能读取产品仓原文；不要把它们写成这里的文件。公开文档入口见仓库 README 的「文档入口」。

- **用户2026-11-06，更新必须同步公开下载**：适配器源码或包更新后正常提交推送本公开仓并核对远程包含提交。通过实际验收的新版本同时发布可下载的完整安装信封、包、companion、LICENSE及SHA256，核下载字节并更新发行锁/下载入口。源码已推送、候选/WIP和草稿Release分别记实，不得称用户已可下载正式最新版。主仓同步记录安装版本和真实验收结果，禁止同版本覆盖。

- **生图画幅、清晰度与结果下载（2026-11-21）**。对每个生图操作核对官方支持的档位、画幅、像素范围及结果来源。支持比例的包通过共享 aspectRatio 角色声明宽:高，清晰度提供可选档位，自定义宽高保留示例。平台请求映射归包；需要宽高的包可用 scripts/image-size-presets.py 按档位像素预算、对齐及比例生成尺寸表，优先使用平台公布的尺寸。验收须从工作台选项核到最终请求与结果下载，文生图和参考图生图分别记录。主仓配套规范见 docs/dev/guides/20261102-GUIDE-第三方适配器唯一开发与产物目录.md 的「生图适配器的画幅与清晰度验收」。
