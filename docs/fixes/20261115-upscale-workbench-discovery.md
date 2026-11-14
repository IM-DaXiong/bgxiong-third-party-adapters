# 超分工作台看不到图片和视频放大

图片包 1.0.3 已声明 `transportSlot=imageUpscale`，但宿主下拉只读工作流绑定，且登记被生图映射就绪挡住。视频没有对应适配包，也没有 v2 提交分支。

宿主分支按传输槽把已安装操作放进超分下拉，并让直接选择走已安装包。视频包 `local.bgx.seedvr2-video` 1.0.0 的 wasm 摘要是 `db8f73c7736a5d6ebae09d6b5cc18b54bcf6d988c09ecd15cea7d1b9d47f8dfe`。安装 ZIP 的回执在 `release/seedvr2-video-1.0.0.json`。尚未发布 GitHub Release，也没有在本环境跑通真实 ComfyUI 放大。
