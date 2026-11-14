# 本地 ComfyUI 全景适配包

当前源码为 1.17.0 开发候选：包含单图、多方向工程、Studio 渲染、局部修补返回、三维场景、MoGe 新视角、LTX/H3 全景视频及图片几何输入。候选不等于公开正式下载版；真实模型、安装版及完整升级验收以产品交接记录为准。

## 1.17.0 图片几何输入候选

全景工作台顶部「几何输入」打开紧凑面板。先读取已配置资源机的模型列表，选 DA3 单目/同场景多视角、MoGe 透视或 MoGe 全景，勾选工程图片后准备。收取只查询原任务，显式「采用几何」后才修改工程；采用支持原生撤销/重做，关闭重开保留来源、参数和受管预览。两张以上来源必须尺寸相同，不会自动缩放。MoGe 全景要求单张 2:1 等距柱状图；DA3 多视角仅适用于上游支持的 Small/Base 模型。

保留上游嵌套多视角文件、depth_grid 及颜色代理、动画 source/clip/box；不长期依赖远端临时目录。未知工程扩展版本只读保全。几何模型权重由资源机安装，选择器读取其真实文件清单，不自动下载模型。27 项原生定向检查、生产 companion 浏览器保存/复开/撤销链及 SDK 四段包检查通过；真实 ComfyUI 推理、视频几何输入和应用 EXE 服务链尚待验。前文单图真实验收不覆盖这些候选功能。

节点声明基线：[ComfyUI eee59185 MoGe](https://github.com/Comfy-Org/ComfyUI/blob/eee59185ff4f43ae2d9ad122b245619d27d9daad/comfy_extras/nodes_moge.py)、[DA3 5ece24e7](https://github.com/Comfy-Org/ComfyUI/blob/5ece24e73c0785e95a45a45a2773ecd1a6f5339b/comfy_extras/nodes_depth_anything_3.py)。Panorama 保持 editor/upstream.lock.json 的固定输入，升级先做差异报告与对应行为检查。

LTX 操作使用包内固定的三阶段图，保留视频音轨，输出解码器的完整 360° ERP，而非示例的透视裁切预览。当前固定输出 2048×1024、24 fps，帧数直接填写 `8n+1`，默认 121；不会先取整为秒，也不会静默裁掉无效帧数。编码 CRF、两处 LoRA 强度及种子由用户声明值映射。通过本地映射/查询测试和实际 SDK 包校验；大模型推理尚未验。帧数依据 [ComfyUI EmptyLTXVLatentVideo](https://github.com/Comfy-Org/ComfyUI/blob/master/comfy_extras/nodes_lt.py) 的时间下采样合同，最终节点/模型版本仍须资源机按冻结清单核对。

Studio 时间线视频新增 `panorama-studio-video`：直接使用 PanoramaStudio 的 VIDEO 输出及核心 SaveVideo，保持已保存的分数帧率和精确帧长，未设置长度时明确拒绝。SDK 模拟提交/原任务收取已核 29.97 fps、73 帧及 360° 投影；宿主视频工程入口、真实 ComfyUI 渲染仍待验。SaveVideo 回执读取 `images`，VHS 视频操作单独读取 `gifs`。1.15.0 的提交回执错误候选保留，不覆盖；1.15.1 仅在 appliedParams 回报输入合同已有字段，工程时间线仍原样传递。

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

参考 RunningHub 动态包：独立 Cargo workspace、官方 `bgx-adapter-v2 init` 生成的 `vendor-sdk/` + `template-lock.json`（本批作者 SDK **2.4.0/ddc91f3**）、标准 `Build.ps1`、`installation/adapter-install.template.json`。

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

等待渲染时可关闭工作室；重新打开后「收取成果」只查询原任务。收取后 frame、mask、pose 是工程的受管附件。MCP 与 UI 共用 canvas.editor.operation.run/collect，返回 status=pending 或 completed。附件保存失败保留原任务，不重提生成。

当前定向验证包含原生映射及查询、SDK 标准包检查、生产 bundle 的保存复开与同任务收取；真实 ComfyUI 渲染仍交资源机。1.14.2 增加修补回贴、接缝准备/还原与显式采用图片成果；真实模型仍未验。

1.13.0 首次实际 EXE 收取暴露姿态文件 MIME 声明错误，SDK 对未识别字节只接受 file/application/octet-stream；1.13.1 已按现有合同修正，不改 SDK、不覆盖 1.13.0。旧任务和远端成果保留，不将其固定包身份改写成新版。

## 修补回贴、接缝与采用（1.14.2 候选）

四种处理方式采用同一行单选，当前方式的参数集中在下一行。取景后将修补图导入工程，在「修补回贴」选择图片及对应取景记录；沿上游 PanoramaStudio 的 sticker/sticker_state 回贴，保留原始 fps、摄像机姿态和关键帧。修补图尺寸必须与取景一致；源工程、父输入或原素材改变时拒绝回贴，不覆盖原成果。

「接缝准备」输出移位图、硬遮罩、羽化遮罩和来源记录；修补后选择原接缝记录执行「接缝还原」。偏移记录的是上游实际夹紧值，逆位移不会再次使用未夹紧的用户输入。修补结果先保留为工程附件，选择图片成果后点击「采用成果」，向当前图片节点追加版本，原版本保留。重复采用同一成果不新增版本、不提交模型任务。对应通用 MCP 为 canvas.editor.asset.publish，UI 通过同一 owner 写入。

模拟链路已覆盖真实 SDK/开发 EXE 的三附件取景、回贴、四附件接缝准备、逐像素逆移位、尺寸错误/来源变化拒绝。真实 ComfyUI 像素、羽化遮罩及大模型效果由资源机验收；模拟结果不代表真实推理已通过。采用入口的开发 EXE 验收见主仓计划检查点。

## 1.16.0 H3工程视频候选

## 1.18.0 视频几何输入候选

“几何输入”可在一行切换图片/视频；视频来源为单选，保存复开保留来源和算法参数。DA3单目序列/同场景多视角、MoGe和MoGe全景沿原生VHS全帧读取；明确使用format=None，不裁切、抽帧、缩放或强制帧率。工程fps/长度须与受管源视频实际timestamps一致，MoGe全景须2:1。深度推理可改变推理分辨率，但不覆盖原视频或音轨；预览须显式采用。

28项适配器检查、生产bundle图片/视频选择及保存复开、标准SDK包检查通过。浏览器使用素材声明夹具，未声称视频解码或真实推理通过。研发机模拟服务启动受阻，实际EXE视频几何联验、真实Comfy/GPU及大素材仍待验；主程序须配套启用冻结编辑工程的视频帧探测。此候选未替换公开完整下载版本。

新增“H3 · 工程新视角视频”，复用固定上游H3/MoGe/CrossView图与已保存Studio工程。参考视频须先导入工程，“参考视频素材”填写工程中的素材名称；工程与实际视频均须24 fps、同帧数且满足17n+5（5至3600帧）。不自动裁切、补帧或改变帧率。参考音轨和生成音轨保留上游连接，输出投影须匹配已保存相机。主程序须启用editor_to_video实际帧时间探测；缺FFmpeg明确报错。

研发记录：23项定向测试、SDK包检查、带音轨73帧小视频的SDK模拟单次提交/原任务查询通过。真实H3/MoGe推理、画质和音频内容待资源机验收。该版本仍为候选，未替换公开下载入口。
