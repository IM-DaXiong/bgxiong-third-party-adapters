# 本地 ComfyUI 全景适配包

当前源码为 1.13.1 开发候选：包含单图、多方向工程、Studio 渲染、局部修补返回、三维场景、MoGe 新视角及 LTX 2.3 全景视频。候选不等于公开正式下载版；真实模型、安装版及完整升级验收以产品交接记录为准。

LTX 操作使用包内固定的三阶段图，保留视频音轨，输出解码器的完整 360° ERP，而非示例的透视裁切预览。当前固定输出 2048×1024、24 fps，帧数直接填写 `8n+1`，默认 121；不会先取整为秒，也不会静默裁掉无效帧数。编码 CRF、两处 LoRA 强度及种子由用户声明值映射。通过本地映射/查询测试和实际 SDK 包校验；大模型推理尚未验。帧数依据 [ComfyUI EmptyLTXVLatentVideo](https://github.com/Comfy-Org/ComfyUI/blob/master/comfy_extras/nodes_lt.py) 的时间下采样合同，最终节点/模型版本仍须资源机按冻结清单核对。

## 已验单图基线 1.0.3

1.0.3 已通过应用安装。比格熊经局域网连接 ComfyUI 0.45.0，由 RTX 4090 将一张客厅照片扩展为 2048×1024 ERP，成果按原任务查询并自动回写。

包 ID：`local.thirdparty.comfy-panorama-image`  
模型：`panorama-image-single`  
操作：`image-to-panorama`  
能力：`image.generate` + `requiredFeatures: ["image-projection-v1"]`  
输出：`equirectangular360`（2048×1024 ERP）

## 安装与取景

在比格熊设置的「第三方适配器」中选择含 `adapter-install.json` 的完整目录。默认连接本机 ComfyUI 8188。连接局域网服务时，在安装描述中同时更新端点和网络授权地址。

在生图工作台选择本包模型并提交一张参考图。生成后右键全景原图，选择「新建全景预览」。可在预览节点切换「平铺」和「360°」。拖到需要的朝向，再用「裁切」生成独立的普通图片节点。

## 工程结构

参考 RunningHub 动态包：独立 Cargo workspace、官方 `bgx-adapter-v2 init` 生成的 `vendor-sdk/` + `template-lock.json`（作者 SDK **2.0.0**）、标准 `Build.ps1`、`installation/adapter-install.template.json`。

> Windows 注意：工作流文件是 [workflow/source.json](workflow/source.json)。不要再放一份只差大小写的溯源文件。溯源摘要在包根 [SOURCE.json](SOURCE.json)。

## 模型（不入 git）

见 `MODEL_ASSETS.json`。本机已落到：

- `E:/bgxiong/data/models/comfyui/diffusion_models/flux-2-klein-base-4b.safetensors`
- `E:/bgxiong/data/models/comfyui/text_encoders/qwen_3_4b.safetensors`
- `E:/bgxiong/data/models/comfyui/vae/flux2-vae.safetensors`
- `E:/bgxiong/data/models/comfyui/loras/flux-2-klein-4B-360-erp-outpaint-lora_V1.safetensors`

自定义节点克隆：`E:/bgxiong/data/comfyui_custom_nodes/ComfyUI-Panorama-Stickers`

## 无头贴图合同

`PanoramaStickers.sticker_state` 固定：

```json
{"kind":"pano_sticker_state","version":1,"pose":{"yaw_deg":0,"pitch_deg":0,"roll_deg":0,"hFOV_deg":90}}
```

SaveImage 终端取 **VAEDecode ERP**，不是 Cutout 透视裁切。

## 构建

当前候选固定 SDK **2.4.0**，源码 `ddc91f3c3465863da5b58082292f61583d48b829`。单图 1.0.3 的历史验收使用 SDK 2.1.2/e903a21。

```powershell
# 语法级：原生检查（不启 Comfy）
cargo test --manifest-path Cargo.toml

# 标准包构建（输出目录必须不存在）
.\Build.ps1 -Sdk <SDK发布根> -OutputDirectory D:\00Dev\panorama-image-install-out -Online
```

离线二次构建需已有 `.bgx-sdk` 缓存；缺缓存应显式失败，勿静默换 SDK。

## 已验范围

资源机 API 无头贴图、单图生成与应用自动回写已验证。自由画布与片段画布的平铺、转向与透视裁切已验证。16GB 显存档和正式安装器待验。

## Frame 取景与遮罩（1.13.1 候选）

在 Studio 选择 Frame 摄像机并保存工程，顶栏同一行设置百万像素、尺寸倍数，再点击「取景与遮罩」。服务冻结当前工程、连线父工程与受管素材，沿 ComfyUI 的 PanoramaStudio 输出 first_frame、mask、sticker_state；姿态直接保留上游的关键帧和 fps，不在宿主重算。无需扩散模型权重，但 ComfyUI 须装有固定上游节点及内置 MaskToImage、PreviewAny、SaveImage。

等待渲染时可关闭工作室；重新打开后「收取取景」只查询原任务。收取后 frame、mask、pose 是工程的受管附件。MCP 与 UI 共用 canvas.editor.operation.run/collect，返回 status=pending 或 completed。附件保存失败保留原任务，不重提生成。

当前定向验证包含原生映射及查询、SDK 标准包检查、生产 bundle 的保存复开与同任务收取；真实 ComfyUI 渲染仍交资源机。手工修补回贴与接缝修复还在后续开发，不能把取景服务当作整个修补流程已完成。

1.13.0 首次实际 EXE 收取暴露姿态文件 MIME 声明错误，SDK 对未识别字节只接受 file/application/octet-stream；1.13.1 已按现有合同修正，不改 SDK、不覆盖 1.13.0。旧任务和远端成果保留，不将其固定包身份改写成新版。
