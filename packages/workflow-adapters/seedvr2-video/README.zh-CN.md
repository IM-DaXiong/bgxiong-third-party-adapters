# SeedVR2 视频放大适配包

一条原视频到一条放大视频。能力键是 `local.bgx.seedvr2-video/upscale`，传输槽是 SDK 已有的 `videoUpscale`。宿主按传输槽发现，不按包名写死下拉。

`workflow/source.json` 是视频高清放大原件。`src/workflow-api.json` 是它的 API 投影：LoadVideo、GetVideoComponents、SeedVR2、CreateVideo、SaveVideo。绕过的 alpha 节点不进入投影。`writerCrf` 不在本图的保存节点上，传入会被拒绝。取消声明为 `unsupported`，不调用 ComfyUI 全局 interrupt。

1.0.1 的操作版本 2 声明 `operation-input-v1`，预检冻结源视频身份、目标短边、最长边上限和其它放大参数。SeedVR2 的每批帧数遵循 `4n+1`，批次重叠须小于每批帧数。编码或解码的分块重叠错误定位到对应字段。[上游节点合同](https://github.com/numz/ComfyUI-SeedVR2_VideoUpscaler/blob/main/README.md)

提交上传同一条源视频，帧率和音轨由工作流从源视频读取。受理回执保存源视频的实测宽高和毫秒时长。原任务查询下载视频原件，再通过 SDK 管理的 FFprobe 取得成果的实测宽高和毫秒时长。配置使用安装声明的受管媒体探测，并按部署端点授权 ComfyUI。

`Build.ps1` 输出 SDK 严格运行包 `runtime/`，以及记录清单摘要的安装目录 `install/`。安装目录携带匹配的工作流原件。1.0.0 的构建记录见 `release/seedvr2-video-1.0.0.json`。

1.0.1 已通过四项 Rust 定向测试、SDK 打包和离线包检查。回环 HTTP 验证了预检、原件上传、单次原任务提交和续查，以及读回视频分辨率和时长。主程序工作台、真实 GPU 推理和公开发行仍待验收。
