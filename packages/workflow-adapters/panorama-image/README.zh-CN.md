# 本地 ComfyUI 全景适配包

当前源码为 1.12.0 开发候选：包含单图、多方向工程、Studio 渲染、局部修补返回、三维场景、MoGe 新视角及 LTX 2.3 全景视频。候选不等于公开正式下载版；真实模型、安装版及完整升级验收以产品交接记录为准。

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

SDK 发行基线为 **2.1.2**，本批验证 SDK 源码 `e903a21`。

```powershell
# 语法级：原生检查（不启 Comfy）
cargo test --manifest-path Cargo.toml

# 标准包构建（输出目录必须不存在）
.\Build.ps1 -Sdk <SDK发布根> -OutputDirectory D:\00Dev\panorama-image-install-out -Online
```

离线二次构建需已有 `.bgx-sdk` 缓存；缺缓存应显式失败，勿静默换 SDK。

## 已验范围

资源机 API 无头贴图、单图生成与应用自动回写已验证。自由画布与片段画布的平铺、转向与透视裁切已验证。16GB 显存档和正式安装器待验。
