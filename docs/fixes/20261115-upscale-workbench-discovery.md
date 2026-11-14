# 超分工作台看不到图片和视频放大

图片包 1.0.3 已声明 `transportSlot=imageUpscale`，但宿主下拉只读工作流绑定，且登记被生图映射就绪挡住。视频没有对应适配包，也没有 v2 提交分支。

宿主分支按传输槽把已安装操作放进超分下拉，并让直接选择走已安装包。视频源码在 `packages/workflow-adapters/seedvr2-video`，尚未编出 wasm，不能当安装包。
