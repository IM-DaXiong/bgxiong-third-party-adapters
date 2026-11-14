# 超分工作台看不到图片和视频放大

图片包 1.0.3 已声明 `transportSlot=imageUpscale`，但宿主下拉只读工作流绑定，且登记被生图映射就绪挡住。视频没有对应适配包，也没有 v2 提交分支。

宿主分支按传输槽把已安装操作放进超分下拉，并让直接选择走已安装包。视频包 `local.bgx.seedvr2-video` 1.0.0 已用 `bgx-adapter-v2 init` 后的 `build` 编出，wasm 摘要是 `3730b1b7546e6b46a1a2ad66a5c7602242e41d50898f163cfb23127e6f6b9784`。安装 ZIP 回执在 `release/seedvr2-video-1.0.0.json`。尚未发布 GitHub Release，也没有在本环境跑通真实 ComfyUI 放大。
