> **BAS-3 / 2026-11-23 生效覆盖**：最终交付及CDN待上传根统一为 `L:/00Dev/bgxiong-00-release`，详见本仓 `RELEASE_OUTPUTS.md` / 主仓统一交付根GUIDE。下文仓内build/dist/artifacts/cdn若仍作为命令输入，只指工具组装/缓存/运行阶段，不再是最终交付位置；artifacts/current本机安装归档与最终用户EXE旁布局不迁移。

# 构建产物与公开下载

源码仓只跟踪这份目录说明。完整安装 ZIP 与资源归档在 [GitHub Releases](https://github.com/IM-DaXiong/bgxiong-third-party-adapters/releases/tag/installable-adapters-20261111-h3-101)，逐包摘要见 [发行下载目录](../release/installable-downloads-20261111.json)。

本地 `current/<pluginId>/<version>/` 与单包 ZIP 解压结构一致；`releases/` 保存冻结资源，`downloads/<release-tag>/` 保存同名公开附件，`candidates/` 保存候选，`cache/` 保存临时缓存。完整规则见 [目录约定](../docs/DIRECTORY_LAYOUT.zh-CN.md)。
