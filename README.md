> **BAS-3 修订 / 用户 t372–t374 / 2026-11-23**：唯一交付根 `L:\00Dev\bgxiong-00-release` 下必须直接平级 `dev-项目名`、`release-项目名`、`cdn-项目名`、`deploy-项目名`，不再有编号分类/主机名/client/ai-story导航外壳。进入项目目录就选/拖成品；包内模块/平台结构照旧，CDN实际对象键不变。按本仓 RELEASE_OUTPUTS.md；同项目/用途/类型/平台/渠道只保留有版本/构建证据的最新完整成品，核验后删旧版，不按复制mtime判断；运行安装目录、源码、配置、缓存不迁移。

> 团队构建/发布/开发测试成品归集规范（2026-11-23）：见 [RELEASE_OUTPUTS.md](RELEASE_OUTPUTS.md)。唯一交付根为 `L:/00Dev/bgxiong-00-release`；这是团队开发交付约束，不改变外部用户安装/运行布局。

# bgxiong-third-party-adapters / 比格熊第三方适配器

比格熊官网 [www.bgxiong.com](https://www.bgxiong.com)｜微信公众号：天途影像｜[比格熊开发治理 Skill](https://www.bgxiong.com/goods/9p65SRB2Sl)

## RunningHub 默认名称更新

- **RunningHub-MiniMax-H3 1.0.8**。9 项默认工作流名称。[下载 ZIP](https://github.com/IM-DaXiong/bgxiong-third-party-adapters/releases/download/runninghub-default-names-20261126/local.thirdparty.runninghub-h3-all-1.0.8.zip)
- **RunningHub-千问-Image-2.1 0.1.6**。2 项默认操作名称。[下载 ZIP](https://github.com/IM-DaXiong/bgxiong-third-party-adapters/releases/download/runninghub-default-names-20261126/local.thirdparty.runninghub-qwen-image-21-0.1.6.zip)

两个 ZIP 均通过隔离宿主安装和模型登记，并从公网上重新下载核对摘要。[发行页及 SHA256](https://github.com/IM-DaXiong/bgxiong-third-party-adapters/releases/tag/runninghub-default-names-20261126)

## 下载后安装，无需编译

这里提供已经构建好的比格熊第三方适配器。打开 [发行下载页](https://github.com/IM-DaXiong/bgxiong-third-party-adapters/releases/tag/installable-adapters-20261111-h3-101)，下载对应适配器的 ZIP，解压后在比格熊 **设置 → 第三方适配器 → 安装适配包** 中选择解压目录。含 `adapter-install.json` 的包选完整目录；FLUX.2、Z Image 包按随包说明选择 `package/` 并配置服务。无需下载源码或安装 Rust。

每个 ZIP 都含适配器 WASM、manifest、摘要、许可证和使用说明；需要配套工作流或自定义节点的包同时附上这些文件。**模型权重、ComfyUI 运行环境、云服务账号与 API Key 不包含在这些小包里**，请按随包说明准备。RunningHub 包使用 RunningHub 账号；本地 ComfyUI 包连接你配置的本机或局域网服务。

下载页提供 `SHA256SUMS`，逐包文件清单与摘要见 [发行清单](release/installable-downloads-20261111.json)。GitHub 自动生成的 Source code ZIP 是源码，不能当适配器安装包。

## 可安装 ZIP 目录

以下安装包来自当前发行清单。所有包均已完成包级校验；实际安装、云端生成和 GPU 验证程度不同，详见每项的验收记录字段，不将“已打包”宣称为全部平台实测通过。H3 Image 1.0.1 已完成应用安装、四参考图真实生成；16 张边界已通过离线行为验证，尚未实测 16 张 GPU 生成。

### LTX 2.3 · 0.1.3

本地文生视频、图生视频、图音频生视频和首尾帧生视频。连接：**本地或局域网 ComfyUI**。0.1.3 修正成片出来后查询结果对不上输出合同的问题，四种能力一起生效。已经选过更早版本的节点，安装后请再选一次 LTX 2.3。模型权重不在包内。

[下载页面](https://github.com/IM-DaXiong/bgxiong-third-party-adapters/releases/tag/ltx23-0.1.3) · [直接下载 ZIP](https://github.com/IM-DaXiong/bgxiong-third-party-adapters/releases/download/ltx23-0.1.3/local.bgx.ltx23-0.1.3.zip) · [SHA256](https://github.com/IM-DaXiong/bgxiong-third-party-adapters/releases/download/ltx23-0.1.3/SHA256SUMS.txt)

解压后在比格熊 **设置 → 第三方适配器 → 安装适配包** 中选择含 `adapter-install.json` 的目录。摘要 `ab27e4089e128ab62d872bf9154e15f9224da090652fe0259b56d77e51e3e1d8`。

### 全景图片 · 1.0.3

将单张图片扩展为 ERP 全景图。1.0.3 已实测资源机生成与比格熊自动回写。 连接：**本地或局域网 ComfyUI**。

[下载页面](https://github.com/IM-DaXiong/bgxiong-third-party-adapters/releases/tag/panorama-image-v1.0.3) · [直接下载 ZIP](https://github.com/IM-DaXiong/bgxiong-third-party-adapters/releases/download/panorama-image-v1.0.3/local.thirdparty.comfy-panorama-image-1.0.3.zip)

包内：WASM、manifest 与摘要、许可及说明。完整文件名见发行清单。

### 全景工作室 · 1.24.3 开发候选

[完整候选下载](https://github.com/IM-DaXiong/bgxiong-third-party-adapters/releases/tag/panorama-1.24.3-candidate) · [直接下载 ZIP](https://github.com/IM-DaXiong/bgxiong-third-party-adapters/releases/download/panorama-1.24.3-candidate/comfy-panorama-image-1.24.3.zip) · [文件摘要与验收边界](release/panorama-1.24.3-candidate.json)。包含离线编辑器、上游工作流与配套节点，需本批主程序和 SDK 2.4.0。包级校验、隔离安装和画布工作台读写已通过；真实 Comfy/GPU 与正式安装版未测。当前稳定版仍为 1.0.3。

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

### MiniMax-H3 人脸精修

**1.0.8 开发验收版适用于比格熊 0.6.1130 及以上版本。** 开启「SAM 脸形遮罩」可沿脸部轮廓合成精修区域。工作台从当前配置的 ComfyUI 读取模型列表。若模型已移走，需重新选择。未构建列表时可关闭遮罩继续精修。旧草稿已开启遮罩却没有模型名时，适配器会在提交时选择当前加载器提供的兼容模型。普通选脸和已确认轨迹共用该规则。

启用遮罩需要 ComfyUI-Impact-Pack 的 `SAMLoader`、人脸精修配套节点 `H3FaceMaskSAM` 和可选模型。模型列表为空时，将对应权重放入 ComfyUI 的 `models/sams` 并刷新列表。加载器已提供 ESAM 时也可选用。

**1.0.7 公开下载**

在视频里选择要精修的人脸，预览跟踪效果，再提交精修。连接本地或局域网 ComfyUI。分析目标时若已有渲染占用 GPU，工作台会提示等待队列空闲，之后可再次点击「分析目标」。

从 1.0.1 或 1.0.3 在线升级小脸精修时，请使用比格熊 0.6.1129 或更新版本。0.6.1129 能保留旧精修操作的当前模型选择，并识别 1.0.7 新增的轨迹输入。首次手动安装可用 0.6.1128 或更新版本，解压 ZIP 后在适配器设置选择含 `adapter-install.json` 的目录。包内包含配套 ComfyUI 节点及许可说明。

[下载 1.0.7 安装包](https://github.com/IM-DaXiong/bgxiong-third-party-adapters/releases/download/h3-face-refine-v1.0.7/local.thirdparty.h3-face-refine-1.0.7.zip) · [SHA-256 校验表](https://github.com/IM-DaXiong/bgxiong-third-party-adapters/releases/download/h3-face-refine-v1.0.7/SHA256SUMS-h3-face-refine-1.0.7.txt) · [发行记录](https://github.com/IM-DaXiong/bgxiong-third-party-adapters/releases/tag/h3-face-refine-v1.0.7)

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

### Qwen Image 2.1 · 0.1.2

千问图像生成与参考图编辑。0.1.2 已随 0.6.1116 安装快照打包校验；云端 4K 输出未测。源码候选可能更高。 连接：**RunningHub**。

[下载页面](https://github.com/IM-DaXiong/bgxiong-third-party-adapters/releases/tag/installer-0.6.1116) · [直接下载 ZIP](https://github.com/IM-DaXiong/bgxiong-third-party-adapters/releases/download/installer-0.6.1116/runninghub-qwen-image-21-0.1.2.zip)

包内：WASM、manifest 与摘要、许可及说明。完整文件名见发行清单。

### Wan S2V · 0.1.3

标准语音驱动视频；需要已部署对应模型和工作流。 连接：**本地或局域网 ComfyUI**。

[下载页面](https://github.com/IM-DaXiong/bgxiong-third-party-adapters/releases/tag/installable-adapters-20261111-h3-101) · [直接下载 ZIP](https://github.com/IM-DaXiong/bgxiong-third-party-adapters/releases/download/installable-adapters-20261111-h3-101/local.bgx.wan22-s2v-0.1.3.zip)

包内：WASM、manifest 与摘要、许可及说明。完整文件名见发行清单。

### MiniMax H3 Image · 1.0.1

文生图与 1–16 张有序参考图编辑；动态增减图片槽，支持画幅选择和 2K/4K；附冻结工作流与模型摘要。 连接：**本地或局域网 ComfyUI**。

[下载页面](https://github.com/IM-DaXiong/bgxiong-third-party-adapters/releases/tag/installable-adapters-20261111-h3-101) · [直接下载 ZIP](https://github.com/IM-DaXiong/bgxiong-third-party-adapters/releases/download/installable-adapters-20261111-h3-101/local.bgx.minimax-h3-image-comfy-1.0.1.zip)

包内：WASM、manifest 与摘要、许可及说明；另含 工作流／配置／配套文件。完整文件名见发行清单。

### RunningHub H3 全工作流 · 1.0.3

安装 `runninghub-h3-all` 后，在视频工作台的 **RunningHub** 分组选择多图参考、固定图数参考和首帧工作流，共 9 项。安装时绑定 RunningHub API Key。

[完整安装 ZIP](https://github.com/IM-DaXiong/bgxiong-third-party-adapters/releases/download/runninghub-h3-all-v1.0.3/local.thirdparty.runninghub-h3-all-1.0.3.zip) · [SHA256](https://github.com/IM-DaXiong/bgxiong-third-party-adapters/releases/download/runninghub-h3-all-v1.0.3/SHA256SUMS.txt) · [安装与工作流说明](docs/USAGE.zh-CN.md#runninghub-h3-视频)

1.0.3 使用 SDK 2.4.0，应用安装已验证，本构建未做云端出片。1.0.2 修复三条多参路线的音频 MIME 误拒，与当时的 SDK 2.1.3 联合支持常见音频格式。22 组真实音频已通过平台上传与解码，三条路线均已真实出片、下载并完整解码。含非 UTF-8 旧标签的 APE 仍可能被平台解码器拒绝；具体范围见[1.0.3 记录](release/runninghub-h3-all-1.0.3.json)和[1.0.2 验收](release/runninghub-h3-all-1.0.2.json)。

## 托管语音、作者样例与候选

Breeze TTS 1.0.1 提供[完整托管安装包](https://github.com/IM-DaXiong/bgxiong-third-party-adapters/releases/download/breeze-durable-1.0.1/local.tts.breeze-1.0.1.zip)，包含适配器、companion、资源准备脚本、清单和摘要。plain、voice_design、reference_clone 三种模式保留；新增持久任务与受理结果恢复。需要 SDK 2.2.0 和对应的应用接线，运行时仍为 1.0.2，模型与环境资源按[安装说明](packages/tts-adapters/breeze)另行准备。包级校验与真实模型受理恢复已通过，最终应用安装联验见[独立验收记录](release/breeze-durable-1.0.1.json)。VoxCPM 当前不在可发布安装包清单。

**Breeze 1.0.1 离线安装元数据（新电脑）**：请下载本次的 [安装识别文件](https://github.com/IM-DaXiong/bgxiong-third-party-adapters/releases/download/breeze-durable-1.0.1/breeze-tts-h1-1.0.2.zip.offline-bundle.json)、[原运行时清单](https://github.com/IM-DaXiong/bgxiong-third-party-adapters/releases/download/breeze-durable-1.0.1/breeze-tts-h1-1.0.2.json) 和[独立 SHA256 清单](https://github.com/IM-DaXiong/bgxiong-third-party-adapters/releases/download/breeze-durable-1.0.1/BREEZE-INSTALL-METADATA-SHA256SUMS.txt)。前两个文件与原 [breeze-tts-h1-1.0.2.zip](https://r.bgxiong.com/client/ai-story/runtimes/BreezeTTS/breeze-tts-h1-1.0.2.zip) 放在所选资源目录的 `BreezeTTS/` 内，保留完整文件名；依赖、模型、SoX 归档沿安装包内 `provisioning.json` 的原 URL/文件名放在同目录，Python 归档放在并列 `Python/`。然后在应用的本地离线资源面板选择该资源目录并扫描安装。旧 CDN 识别文件对应旧适配器，不能与 1.0.1 混用；应用不会自动下载或替换这个识别文件。此更新仅补齐元数据，原运行时 1.0.2、完整安装 ZIP 与原 SHA256SUMS 均未覆盖。公开下载字节验证见[元数据回执](release/breeze-durable-1.0.1-metadata.json)。

H3 r2v/r2v-audio/t2v、LTX 2.3 / 2.5 教学样例、Qwen i2i/multi-ref/t2i/two-output 的 `sourceExample` 是 [作者样例](packages/comfy-workflow-adapter/v2)，不列为可直接安装发行包。可安装的 LTX 2.3 生产包是上面的 `local.bgx.ltx23` 0.1.3。源码目录中的候选版本可能高于已冻结版本；用户下载以本页和发行锁为准。

## 开发与跨机器资源恢复

本地与公开仓库遵循同一套[目录结构](docs/DIRECTORY_LAYOUT.zh-CN.md)。安装 ZIP 与本地 current 目录逐文件一致；历史现场归 records，不再混在根目录。

源码在 `packages/` 与 `projects/`；适配器 SDK 独立维护于 [bgxiong-adapter-sdk](https://github.com/IM-DaXiong/bgxiong-adapter-sdk)。协议和 SDK 说明文档采用该仓 MIT 许可，不改变本仓许可。

开发者可按[资源回执](release/current-artifact.json)下载 `third-party-frozen-resources-*.zip`，它是跨机恢复全集，**不是单个可安装适配器**。在主仓运行 `python scripts/restore-third-party-release.py --archive <绝对路径>`，再运行 `python scripts/stage-third-party-release.py --local-current`；恢复须匹配 [资源回执](release/current-artifact.json)。本地安装入口为 `artifacts/current/<pluginId>/<version>/`。

[用户安装指南](docs/USAGE.zh-CN.md) · [开发指南](docs/DEVELOPING.md) · [智能体入口](AGENT.md) · [源码包清单](packages/README.zh-CN.md) · [独立工程](projects/README.md)

## 许可与比格熊

许可见 [LICENSE](LICENSE)（不是 MIT）：允许个人使用和自有项目商用；禁止将本仓内容原样或改造后作为独立商业产品、付费包或付费适配器库出售。

比格熊数字导演工作站是本地 AI 创作工作台。适配器仓不包含主程序。[版本说明](https://www.bgxiong.com/client/version.html)。

比格熊官网 [www.bgxiong.com](https://www.bgxiong.com)｜微信公众号：天途影像｜[比格熊开发治理 Skill](https://www.bgxiong.com/goods/9p65SRB2Sl)
