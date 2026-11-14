# SeedVR2 视频放大适配包

一条原视频到一条放大视频。能力键是 `local.bgx.seedvr2-video/upscale`，传输槽是 SDK 已有的 `videoUpscale`。宿主按传输槽发现，不按包名写死下拉。

`workflow/source.json` 是视频高清放大原件。`src/workflow-api.json` 是它的 API 投影：LoadVideo、GetVideoComponents、SeedVR2、CreateVideo、SaveVideo。绕过的 alpha 节点不进入投影。`writerCrf` 不在本图的保存节点上，传入会被拒绝。取消声明为 `unsupported`，不调用 ComfyUI 全局 interrupt。

源码清单里的 `artifactSha256` 是 `wasm32-wasip2` 正式构建产物的真实摘要。安装树和 ZIP 不在本目录；发行回执是 `release/seedvr2-video-1.0.0.json`。本目录没有 GitHub Release 下载地址。
