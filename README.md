# bgxiong-third-party-adapters / 比格熊第三方适配器

比格熊官网 [www.bgxiong.com](https://www.bgxiong.com)｜微信公众号：天途影像｜[比格熊开发治理 Skill](https://www.bgxiong.com/goods/9p65SRB2Sl)

## 下载后安装，无需编译

这里提供已经构建好的比格熊第三方适配器。打开 [发行下载页](https://github.com/IM-DaXiong/bgxiong-third-party-adapters/releases/tag/installable-adapters-20261111-h3-101)，下载对应适配器的 ZIP，解压后在比格熊 **设置 → 第三方适配器 → 安装适配包** 中选择解压目录。含 `adapter-install.json` 的包选完整目录；FLUX.2、Z Image 包按随包说明选择 `package/` 并配置服务。无需下载源码或安装 Rust。

每个 ZIP 都含适配器 WASM、manifest、摘要、许可证和使用说明；需要配套工作流或自定义节点的包同时附上这些文件。**模型权重、ComfyUI 运行环境、云服务账号与 API Key 不包含在这些小包里**，请按随包说明准备。RunningHub 包使用 RunningHub 账号；本地 ComfyUI 包连接你配置的本机或局域网服务。

下载页提供 `SHA256SUMS`，逐包文件清单与摘要见 [发行清单](release/installable-downloads-20261111.json)。GitHub 自动生成的 Source code ZIP 是源码，不能当适配器安装包。

## 可安装 ZIP 目录

以下安装包来自当前发行清单。所有包均已完成包级校验；实际安装、云端生成和 GPU 验证程度不同，详见每项的验收记录字段，不将“已打包”宣称为全部平台实测通过。H3 Image 1.0.1 已完成应用安装、四参考图真实生成；16 张边界已通过离线行为验证，尚未实测 16 张 GPU 生成。

### 全景图片 · 1.0.3

将单张图片扩展为 ERP 全景图。1.0.3 已实测资源机生成与比格熊自动回写。 连接：**本地或局域网 ComfyUI**。

[下载页面](https://github.com/IM-DaXiong/bgxiong-third-party-adapters/releases/tag/panorama-image-v1.0.3) · [直接下载 ZIP](https://github.com/IM-DaXiong/bgxiong-third-party-adapters/releases/download/panorama-image-v1.0.3/local.thirdparty.comfy-panorama-image-1.0.3.zip)

包内：WASM、manifest 与摘要、许可及说明。完整文件名见发行清单。

### SeedVR2 图片放大 · 1.0.3

单张图片高清修复与放大；包含工作流模板声明。 连接：**本地或局域网 ComfyUI**。

[下载页面](https://github.com/IM-DaXiong/bgxiong-third-party-adapters/releases/tag/installable-adapters-20261111-h3-101) · [直接下载 ZIP](https://github.com/IM-DaXiong/bgxiong-third-party-adapters/releases/download/installable-adapters-20261111-h3-101/local.bgx.seedvr2-image-1.0.3.zip)

包内：WASM、manifest 与摘要、许可及说明；另含 工作流／配置／配套文件。完整文件名见发行清单。

### Wan Animate 2 · 0.1.2

角色动画工作流接入；包含工作流模板声明。 连接：**本地或局域网 ComfyUI**。

[下载页面](https://github.com/IM-DaXiong/bgxiong-third-party-adapters/releases/tag/installable-adapters-20261111-h3-101) · [直接下载 ZIP](https://github.com/IM-DaXiong/bgxiong-third-party-adapters/releases/download/installable-adapters-20261111-h3-101/local.bgx.wan-animate2-0.1.2.zip)

包内：WASM、manifest 与摘要、许可及说明；另含 工作流／配置／配套文件。完整文件名见发行清单。

### Wan 2.2 Animate · 0.2.13

视频人物替换与动画；附 BGXAnimateTracking 节点及资源安装说明。 连接：**本地或局域网 ComfyUI**。

[下载页面](https://github.com/IM-DaXiong/bgxiong-third-party-adapters/releases/tag/installable-adapters-20261111-h3-101) · [直接下载 ZIP](https://github.com/IM-DaXiong/bgxiong-third-party-adapters/releases/download/installable-adapters-20261111-h3-101/local.bgx.wan22-animate-0.2.13.zip)

包内：WASM、manifest 与摘要、许可及说明；另含 自定义 ComfyUI 节点及配套文件。完整文件名见发行清单。

### DeepSeek 文本 · 1.0.1

文本生成与对话，需要自己的平台 API Key。 连接：**DeepSeek 云端 API**。

[下载页面](https://github.com/IM-DaXiong/bgxiong-third-party-adapters/releases/tag/installable-adapters-20261111-h3-101) · [直接下载 ZIP](https://github.com/IM-DaXiong/bgxiong-third-party-adapters/releases/download/installable-adapters-20261111-h3-101/local.bgx.deepseek-text-1.0.1.zip)

包内：WASM、manifest 与摘要、许可及说明。完整文件名见发行清单。

### MiniMax M3 文本 · 1.0.3

M3 文本生成与对话，需要自己的平台 API Key。 连接：**MiniMax 云端 API**。

[下载页面](https://github.com/IM-DaXiong/bgxiong-third-party-adapters/releases/tag/installable-adapters-20261111-h3-101) · [直接下载 ZIP](https://github.com/IM-DaXiong/bgxiong-third-party-adapters/releases/download/installable-adapters-20261111-h3-101/local.bgx.minimax-m3-1.0.3.zip)

包内：WASM、manifest 与摘要、许可及说明。完整文件名见发行清单。

### Seedream 5.0 Pro · 1.0.6

图片生成与编辑接入；需配置服务账号和 API 凭证。 连接：**Seedream 云端图像服务**。

[下载页面](https://github.com/IM-DaXiong/bgxiong-third-party-adapters/releases/tag/installable-adapters-20261111-h3-101) · [直接下载 ZIP](https://github.com/IM-DaXiong/bgxiong-third-party-adapters/releases/download/installable-adapters-20261111-h3-101/local.bgx.seedream-image-1.0.6.zip)

包内：WASM、manifest 与摘要、许可及说明。完整文件名见发行清单。

### H3 人脸精修 · 1.0.0-a31

视频人脸精修；附 H3-FaceRefine、BGXFaceRefineContract 节点及许可说明。 连接：**本地或局域网 ComfyUI**。

[下载页面](https://github.com/IM-DaXiong/bgxiong-third-party-adapters/releases/tag/installable-adapters-20261111-h3-101) · [直接下载 ZIP](https://github.com/IM-DaXiong/bgxiong-third-party-adapters/releases/download/installable-adapters-20261111-h3-101/local.thirdparty.h3-face-refine-1.0.0-a31.zip)

包内：WASM、manifest 与摘要、许可及说明；另含 自定义 ComfyUI 节点及配套文件。完整文件名见发行清单。

### FLUX.2 有序参考图 · 1.0.2

按顺序使用多张参考图生成图片；附配置样例和工作流模板。 连接：**本地或局域网 ComfyUI**。

[下载页面](https://github.com/IM-DaXiong/bgxiong-third-party-adapters/releases/tag/installable-adapters-20261111-h3-101) · [直接下载 ZIP](https://github.com/IM-DaXiong/bgxiong-third-party-adapters/releases/download/installable-adapters-20261111-h3-101/local.independent.flux2-ordered-reference-1.0.2.zip)

包内：WASM、manifest 与摘要、许可及说明；另含 工作流／配置／配套文件。完整文件名见发行清单。

### H3 Continuum · 1.0.3

独立驱动音频的视频工作流；附原始工作流、节点定义和请求样例。 连接：**本地或局域网 ComfyUI**。

[下载页面](https://github.com/IM-DaXiong/bgxiong-third-party-adapters/releases/tag/installable-adapters-20261111-h3-101) · [直接下载 ZIP](https://github.com/IM-DaXiong/bgxiong-third-party-adapters/releases/download/installable-adapters-20261111-h3-101/local.independent.comfy-h3-continuum-1.0.3.zip)

包内：WASM、manifest 与摘要、许可及说明；另含 工作流／配置／配套文件。完整文件名见发行清单。

### Z Image Turbo · 1.0.3

快速文生图工作流接入；附工作流模板声明。 连接：**本地或局域网 ComfyUI**。

[下载页面](https://github.com/IM-DaXiong/bgxiong-third-party-adapters/releases/tag/installable-adapters-20261111-h3-101) · [直接下载 ZIP](https://github.com/IM-DaXiong/bgxiong-third-party-adapters/releases/download/installable-adapters-20261111-h3-101/local.zeroauthor.comfy-zimage-turbo-1.0.3.zip)

包内：WASM、manifest 与摘要、许可及说明；另含 工作流／配置／配套文件。完整文件名见发行清单。

### Qwen Image 2.1 · 0.1.1

千问图像生成与参考图编辑；下载版本以冻结清单为准，源码候选另计。 连接：**RunningHub**。

[下载页面](https://github.com/IM-DaXiong/bgxiong-third-party-adapters/releases/tag/installable-adapters-20261111-h3-101) · [直接下载 ZIP](https://github.com/IM-DaXiong/bgxiong-third-party-adapters/releases/download/installable-adapters-20261111-h3-101/local.thirdparty.runninghub-qwen-image-21-0.1.1.zip)

包内：WASM、manifest 与摘要、许可及说明。完整文件名见发行清单。

### Wan S2V · 0.1.3

标准语音驱动视频；需要已部署对应模型和工作流。 连接：**本地或局域网 ComfyUI**。

[下载页面](https://github.com/IM-DaXiong/bgxiong-third-party-adapters/releases/tag/installable-adapters-20261111-h3-101) · [直接下载 ZIP](https://github.com/IM-DaXiong/bgxiong-third-party-adapters/releases/download/installable-adapters-20261111-h3-101/local.bgx.wan22-s2v-0.1.3.zip)

包内：WASM、manifest 与摘要、许可及说明。完整文件名见发行清单。

### MiniMax H3 Image · 1.0.1

文生图与 1–16 张有序参考图编辑；动态增减图片槽，支持画幅选择和 2K/4K；附冻结工作流与模型摘要。 连接：**本地或局域网 ComfyUI**。

[下载页面](https://github.com/IM-DaXiong/bgxiong-third-party-adapters/releases/tag/installable-adapters-20261111-h3-101) · [直接下载 ZIP](https://github.com/IM-DaXiong/bgxiong-third-party-adapters/releases/download/installable-adapters-20261111-h3-101/local.bgx.minimax-h3-image-comfy-1.0.1.zip)

包内：WASM、manifest 与摘要、许可及说明；另含 工作流／配置／配套文件。完整文件名见发行清单。

### RunningHub H3 全工作流 · 1.0.2

安装 `runninghub-h3-all` 后，在视频工作台的 **RunningHub** 分组选择多图参考、固定图数参考和首帧工作流，共 9 项。安装时绑定 RunningHub API Key。

[完整安装 ZIP](https://github.com/IM-DaXiong/bgxiong-third-party-adapters/releases/download/runninghub-h3-all-v1.0.2/local.thirdparty.runninghub-h3-all-1.0.2.zip) · [SHA256](https://github.com/IM-DaXiong/bgxiong-third-party-adapters/releases/download/runninghub-h3-all-v1.0.2/SHA256SUMS.txt) · [安装与工作流说明](docs/USAGE.zh-CN.md#runninghub-h3-视频)

1.0.2 修复三条多参路线的音频 MIME 误拒，与 SDK 2.1.3 联合支持常见音频格式。22 组真实音频已通过平台上传与解码，三条路线均已真实出片、下载并完整解码。含非 UTF-8 旧标签的 APE 仍可能被平台解码器拒绝；具体范围见[验收记录](release/runninghub-h3-all-1.0.2.json)。

## 托管语音、作者样例与候选

Breeze TTS 1.0.1 提供[完整托管安装包](https://github.com/IM-DaXiong/bgxiong-third-party-adapters/releases/download/breeze-durable-1.0.1/local.tts.breeze-1.0.1.zip)，包含适配器、companion、资源准备脚本、清单和摘要。plain、voice_design、reference_clone 三种模式保留；新增持久任务与受理结果恢复。需要 SDK 2.2.0 和对应的应用接线，运行时仍为 1.0.2，模型与环境资源按[安装说明](packages/tts-adapters/breeze)另行准备。包级校验与真实模型受理恢复已通过，最终应用安装联验见[独立验收记录](release/breeze-durable-1.0.1.json)。VoxCPM 当前不在可发布安装包清单。

**Breeze 1.0.1 离线安装元数据（新电脑）**：请下载本次的 [安装识别文件](https://github.com/IM-DaXiong/bgxiong-third-party-adapters/releases/download/breeze-durable-1.0.1/breeze-tts-h1-1.0.2.zip.offline-bundle.json)、[原运行时清单](https://github.com/IM-DaXiong/bgxiong-third-party-adapters/releases/download/breeze-durable-1.0.1/breeze-tts-h1-1.0.2.json) 和[独立 SHA256 清单](https://github.com/IM-DaXiong/bgxiong-third-party-adapters/releases/download/breeze-durable-1.0.1/BREEZE-INSTALL-METADATA-SHA256SUMS.txt)。前两个文件与原 [breeze-tts-h1-1.0.2.zip](https://r.bgxiong.com/client/ai-story/runtimes/BreezeTTS/breeze-tts-h1-1.0.2.zip) 放在所选资源目录的 `BreezeTTS/` 内，保留完整文件名；依赖、模型、SoX 归档沿安装包内 `provisioning.json` 的原 URL/文件名放在同目录，Python 归档放在并列 `Python/`。然后在应用的本地离线资源面板选择该资源目录并扫描安装。旧 CDN 识别文件对应旧适配器，不能与 1.0.1 混用；应用不会自动下载或替换这个识别文件。此更新仅补齐元数据，原运行时 1.0.2、完整安装 ZIP 与原 SHA256SUMS 均未覆盖。公开下载字节验证见[元数据回执](release/breeze-durable-1.0.1-metadata.json)。

H3 r2v/r2v-audio/t2v、LTX2.3、LTX2.5、Qwen i2i/multi-ref/t2i/two-output 的 `sourceExample` 是 [作者样例](packages/comfy-workflow-adapter/v2)，不列为可直接安装发行包。源码目录中的候选版本可能高于上面的已冻结版本；用户下载以本页和发行锁为准。

## 开发与跨机器资源恢复

本地与公开仓库遵循同一套[目录结构](docs/DIRECTORY_LAYOUT.zh-CN.md)。安装 ZIP 与本地 current 目录逐文件一致；历史现场归 records，不再混在根目录。

源码在 `packages/` 与 `projects/`；适配器 SDK 独立维护于 [bgxiong-adapter-sdk](https://github.com/IM-DaXiong/bgxiong-adapter-sdk)。协议和 SDK 说明文档采用该仓 MIT 许可，不改变本仓许可。

开发者可按[资源回执](release/current-artifact.json)下载 `third-party-frozen-resources-*.zip`，它是跨机恢复全集，**不是单个可安装适配器**。在主仓运行 `python scripts/restore-third-party-release.py --archive <绝对路径>`，再运行 `python scripts/stage-third-party-release.py --local-current`；恢复须匹配 [资源回执](release/current-artifact.json)。本地安装入口为 `artifacts/current/<pluginId>/<version>/`。

[用户安装指南](docs/USAGE.zh-CN.md) · [开发指南](docs/DEVELOPING.md) · [智能体入口](AGENT.md) · [源码包清单](packages/README.zh-CN.md) · [独立工程](projects/README.md)

## 许可与比格熊

许可见 [LICENSE](LICENSE)（不是 MIT）：允许个人使用和自有项目商用；禁止将本仓内容原样或改造后作为独立商业产品、付费包或付费适配器库出售。

比格熊数字导演工作站是本地 AI 创作工作台。适配器仓不包含主程序。[版本说明](https://www.bgxiong.com/client/version.html)。

比格熊官网 [www.bgxiong.com](https://www.bgxiong.com)｜微信公众号：天途影像｜[比格熊开发治理 Skill](https://www.bgxiong.com/goods/9p65SRB2Sl)
