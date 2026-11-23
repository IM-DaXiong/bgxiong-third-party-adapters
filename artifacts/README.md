> **BAS-3 修订 / 用户 t372–t374 / 2026-11-23**：唯一交付根 `L:\00Dev\bgxiong-00-release` 下必须直接平级 `dev-项目名`、`release-项目名`、`cdn-项目名`、`deploy-项目名`，不再有编号分类/主机名/client/ai-story导航外壳。进入项目目录就选/拖成品；包内模块/平台结构照旧，CDN实际对象键不变。按本仓 RELEASE_OUTPUTS.md；同项目/用途/类型/平台/渠道只保留有版本/构建证据的最新完整成品，核验后删旧版，不按复制mtime判断；运行安装目录、源码、配置、缓存不迁移。

> **BAS-3 / 2026-11-23 生效覆盖**：最终交付及CDN待上传根统一为 `L:/00Dev/bgxiong-00-release`，详见本仓 `RELEASE_OUTPUTS.md` / 主仓统一交付根GUIDE。下文仓内build/dist/artifacts/cdn若仍作为命令输入，只指工具组装/缓存/运行阶段，不再是最终交付位置；artifacts/current本机安装归档与最终用户EXE旁布局不迁移。

# 构建产物与公开下载

源码仓只跟踪这份目录说明。完整安装 ZIP 与资源归档在 [GitHub Releases](https://github.com/IM-DaXiong/bgxiong-third-party-adapters/releases/tag/installable-adapters-20261111-h3-101)，逐包摘要见 [发行下载目录](../release/installable-downloads-20261111.json)。

本地 `current/<pluginId>/<version>/` 与单包 ZIP 解压结构一致；`releases/` 保存冻结资源，`downloads/<release-tag>/` 保存同名公开附件，`candidates/` 保存候选，`cache/` 保存临时缓存。完整规则见 [目录约定](../docs/DIRECTORY_LAYOUT.zh-CN.md)。

自动CDN成品输出/校验已改到 `L:/00Dev/bgxiong-00-release/cdn-bgxiong-third-party-adapters`（pack-cdn-directory.py / stage-cdn-upload-directory.py / publish-online-index.py）；`artifacts/cdn-index` 只保留签名/资格工作区，`artifacts/current` 是仍在使用的安装档案，均不是待上传导航壳。
