# bgxiong-third-party-adapters / 比格熊第三方适配器

2026-11-11 当前 SDK 重建的 12 个新版本包与跨机冻结资源：[完整 ZIP、配套文件和 SHA256](https://github.com/IM-DaXiong/bgxiong-third-party-adapters/releases/tag/sdk-completion-20261111)。78 项原生测试和 SDK 构建/检查/封包通过；逐包实际安装、云端生成与安装器状态见 [本批记录](release/sdk-completion-20261111.json)，未验项不计为通过。

开发机同步主仓与本仓后，下载 `third-party-frozen-resources-20261111.zip`，在主仓显式执行 `python scripts/restore-third-party-release.py --archive <下载ZIP的绝对路径>`，再执行 `python scripts/stage-third-party-release.py --local-current`。恢复只接受 [当前资源回执](release/current-artifact.json) 对应的锁和摘要，不覆盖不同字节。SDK 独立源码与运行工具从同级 SDK 仓及其发布页获取；Breeze 托管运行时仍使用原来的独立资源 owner。

本仓库是比格熊第三方适配器的独立开源真源，与主程序仓 `bgxiong-ai-story` 分开维护。适配器的源码与分发以本仓库为准，协议和 SDK 由独立 bgxiong-adapter-sdk 维护；主程序只按契约导入并运行插件。不要把适配器开发写回主程序仓库。

现名 `bgxiong-third-party-adapters`；公开地址 https://github.com/IM-DaXiong/bgxiong-third-party-adapters 。旧名 `bgxiong-vendor-adapters` 已更名并指向本仓库，不要再当成另一份产品。

许可见 [LICENSE](./LICENSE)（不是 MIT）：个人使用和在自有项目/产品中商用都可以；禁止将本仓库内容原样或改造后作为独立商业产品、付费包或付费适配器库出售。

2026-11-05统一：本目录为开发和Git唯一根，公开仓库 https://github.com/IM-DaXiong/bgxiong-third-party-adapters 。

- packages/：已合并主线包源码；Wan22合并时以0.2.12及原未提交修改为基线，当前版本见下表及release/current.json，其他包从原sources/main/packages合入。
- projects/：独立工程及其唯一 owner（清单见 `projects/README.md`）。
- artifacts/：发行产物；records/：保全与核查证据；这两类本机资料不自动发布。

六个当前视频包的完整安装信封、WASM、配套文件和 SHA256：[2026-11-11 下载与实际验收](https://github.com/IM-DaXiong/bgxiong-third-party-adapters/releases/tag/sdk-video-explicit-modes-20261111)。版本与摘要见 [下载清单](release/video-explicit-modes-20261111.json)。

## 本机安装测试

打开适配器仓的 `artifacts/current/`，按 `<pluginId>/<version>/` 找当前可导入的包。软件中进入 **设置 → 第三方适配器 → 安装适配包**。有 `adapter-install.json` 时选择版本目录，否则按包说明选其中的 `package/`。

发行准备会依据 `release/current.json` 自动刷新此目录。已有发行文件时，也可在同级主仓运行 `python scripts/stage-third-party-release.py --local-current`。`index.json` 列出每个包的当前版本及验证状态。新机器需要先取得锁定的发行文件。
- contract/v2/：现役包的受控 SDK 消费资料。
- 用户分发目录统一名称bgxiong-third-party-adapters；实际新发行落盘和安装验收另记，目录合并不等于全包构建通过。

## 公开同步台账（2026-11-06）

- 开源远程：`https://github.com/IM-DaXiong/bgxiong-third-party-adapters`（原 `bgxiong-vendor-adapters` 已更名指向同一仓库）。
- 主线包 SDK 依赖改为本仓 `contract/v2`，去掉对本机 `bgxiong-ai-story` / `L:/00Dev/...` 的硬编码路径。
- 纳入 RunningHub H3 作者材料、seed 接线冒烟工程，以及 AutoDL 作者占位规范；本机安装验收目录与 AutoDL 参考 API 副本仍不入库。
- 发行产物与核查证据仍在本机 `artifacts/`、`records/`，不随公开同步。

## 开发与使用

- 使用者按[安装说明](docs/USAGE.zh-CN.md)安装完整的构建产物。
- 作者从随软件提供的 `user-examples/vendor-adapter-v2/` 取得 SDK，按[开发指南](docs/DEVELOPING.md)构建、校验和打包。
- 把目标厂商的官方 API 文档和所需能力交给编程智能体，并让它先读 [AGENT.md](AGENT.md)。
- 现役包清单见 [packages/README.zh-CN.md](packages/README.zh-CN.md)，独立工程见 [projects/README.md](projects/README.md)。

## 案例与适配器目录

- [ComfyUI 工作流作者包](packages/comfy-workflow-adapter/v2/)。
- [产品级适配器](packages/)。
- [独立工程](projects/)。

版本以各包当前 `manifest.json` 为准。

### 独立工程：H3 人脸精修

| 路径 | 说明 | pluginId | 当前版本 |
|------|------|----------|----------|
| [bgxiong-h3-face-refine-adapter](./projects/bgxiong-h3-face-refine-adapter) | H3 人脸精修适配器（Rust 源码、配套节点与工作流都在这里）。[h3-face-refine-adapter](./packages/third-party/h3-face-refine-adapter) 只是指向本工程的指针，不是另一份源码。 | `local.thirdparty.h3-face-refine` | `1.0.0-a30` |

`projects/` 里还有 RunningHub / AutoDL 作者材料与接线冒烟工程，见 [README.md](./projects/README.md)，它们不是独立发行的适配器包。

### 产品级适配器（`packages/`）

#### 云端

| 路径 | 说明 | pluginId | 当前版本 |
|------|------|----------|----------|
| [deepseek-text](./packages/cloud-adapters/deepseek-text) | DeepSeek 云端文本 | `local.bgx.deepseek-text` | `1.0.0` |
| [minimax-m3](./packages/cloud-adapters/minimax-m3) | MiniMax M3 云端文本 | `local.bgx.minimax-m3` | `1.0.2` |
| [seedream-image](./packages/cloud-adapters/seedream-image) | Seedream 5.0 Pro 独立图片预览 | `local.bgx.seedream-image` | `1.0.5` |
| [runninghub-h3-dynamic](./packages/cloud-adapters/runninghub-h3-dynamic) | RunningHub H3-r2v-turbo | `local.thirdparty.runninghub-h3-dynamic` | `1.2.6` |
| [runninghub-h3-r2v-dynamic](./packages/cloud-adapters/runninghub-h3-r2v-dynamic) | RunningHub H3-r2v（官方20步） | `local.thirdparty.runninghub-h3-r2v-dynamic` | `1.0.1` |
| [runninghub-h3-r2v-soundtrack](./packages/cloud-adapters/runninghub-h3-r2v-soundtrack) | RunningHub H3-r2v-原声（官方20步） | `local.thirdparty.runninghub-h3-r2v-soundtrack` | `1.0.0` |
| [runninghub-h3-spectrum-dynamic](./packages/cloud-adapters/runninghub-h3-spectrum-dynamic) | RunningHub H3-r2v-spectrum | `local.thirdparty.runninghub-h3-spectrum-dynamic` | `1.0.1` |
| [runninghub-minimax-h3](./packages/cloud-adapters/runninghub-minimax-h3) | RunningHub H3-多参 | `local.example.runninghub-minimax-h3` | `1.0.4` |
| [runninghub-qwen-image-21](./packages/cloud-adapters/runninghub-qwen-image-21) | RunningHub Qwen Image 2.1 文生图 / 1～10 张参考图编辑 | `local.thirdparty.runninghub-qwen-image-21` | `0.1.2` 候选 |

#### 本地工作流

| 路径 | 说明 | pluginId | 当前版本 |
|------|------|----------|----------|
| [wan22-animate](./packages/workflow-adapters/wan22-animate) | Wan 2.2 Animate 人物替换（ComfyUI） | `local.bgx.wan22-animate` | `0.2.13` |
| [wan-animate2](./packages/workflow-adapters/wan-animate2) | Wan Animate 2 角色动画（ComfyUI） | `local.bgx.wan-animate2` | `0.1.1` |
| [seedvr2-image](./packages/workflow-adapters/seedvr2-image) | SeedVR2 单张图片高清放大 | `local.bgx.seedvr2-image` | `1.0.2` |
| [panorama-image](./packages/workflow-adapters/panorama-image) | 本地 ComfyUI 单图扩展全景（ERP） | `local.thirdparty.comfy-panorama-image` | `1.0.2` |

#### 语音

| 路径 | 说明 | pluginId | 当前版本 |
|------|------|----------|----------|
| [breeze](./packages/tts-adapters/breeze) | Breeze TTS（plain / voice_design / reference_clone） | `local.tts.breeze` | `1.0.0-d` |
| [voxcpm](./packages/tts-adapters/voxcpm) | VoxCPM 材料包（独立 SDK 分发策略见包内说明） | `local.tts.voxcpm` | `1.0.0-a` |

#### Comfy 工作流作者包（`packages/comfy-workflow-adapter/v2/`）

| 路径 | 说明 | pluginId | 当前版本 |
|------|------|----------|----------|
| [`h3-r2v`](./packages/comfy-workflow-adapter/v2/h3-r2v) | ComfyUI H3 双参考视频 | `local.example.comfy-h3-r2v` | `1.0.1` |
| [`h3-r2v-audio`](./packages/comfy-workflow-adapter/v2/h3-r2v-audio) | ComfyUI H3 双参考 + 音频 | `local.example.comfy-h3-r2v-audio` | `1.0.1` |
| [`h3-t2v`](./packages/comfy-workflow-adapter/v2/h3-t2v) | ComfyUI H3 文生视频 | `local.example.comfy-h3-t2v` | `1.0.1` |
| [`ltx23-ia2v`](./packages/comfy-workflow-adapter/v2/ltx23-ia2v) | ComfyUI LTX2.3 图 + 参考音频 | `local.example.comfy-ltx23-ia2v` | `1.0.1` |
| [`ltx25-i2v`](./packages/comfy-workflow-adapter/v2/ltx25-i2v) | ComfyUI LTX2.5 图生视频 | `local.example.comfy-ltx25-i2v` | `1.0.1` |
| [`qwen-t2i`](./packages/comfy-workflow-adapter/v2/qwen-t2i) | ComfyUI Qwen Image 2.1 文生图 | `local.example.comfy-qwen-t2i` | `1.0.0` |
| [`qwen-i2i`](./packages/comfy-workflow-adapter/v2/qwen-i2i) | ComfyUI Qwen Image 2.1 图生图 | `local.example.comfy-qwen-i2i` | `1.0.0` |
| [`qwen-multi-ref`](./packages/comfy-workflow-adapter/v2/qwen-multi-ref) | ComfyUI Qwen Image 2.1 多参考 | `local.example.comfy-qwen-multi-ref` | `1.0.0` |
| [`qwen-two-output`](./packages/comfy-workflow-adapter/v2/qwen-two-output) | ComfyUI Qwen Image 2.1 双输出 | `local.example.comfy-qwen-two-output` | `1.0.0` |
| [`h3-continuum-independent`](./packages/comfy-workflow-adapter/v2/h3-continuum-independent) | H3 Continuum 独立驱动音频 | `local.independent.comfy-h3-continuum` | `1.0.3` |
| [`flux2-multi-ref-independent`](./packages/comfy-workflow-adapter/v2/flux2-multi-ref-independent) | FLUX.2 有序参考 | `local.independent.flux2-ordered-reference` | `1.0.1` |
| [`zimage-zero-independent`](./packages/comfy-workflow-adapter/v2/zimage-zero-independent) | Z Image Turbo | `local.zeroauthor.comfy-zimage-turbo` | `1.0.2` |

---

## License

**自定义许可**（不是 MIT）：个人与商用均可使用；**禁止**将本仓库内容原样或改造后作为独立商品出售。全文见 [LICENSE](./LICENSE)。

---

## About BGXiong / 关于比格熊

**比格熊数字导演工作站（BGXiong）**是装在你电脑上的 AI 导演工具：从故事到分镜、生图/生视频与导出，尽量在同一桌面客户端完成——数据在本地，模型与算力通道由你选择。

- 官网 / 下载：[https://www.bgxiong.com](https://www.bgxiong.com)
- 版本说明：[https://www.bgxiong.com/client/version.html](https://www.bgxiong.com/client/version.html)

---

## 联系我们 / Contact

公众号搜索 **天途影像**，可在后台私信联系。

WeChat Official Account: search **天途影像** and message us in the backend.

## 与主仓同级克隆（跨机器）

主程序仓为 `bgxiong-ai-story`。验收/开发适配器时，默认把本开源仓克隆到它的同级目录，目录名是 bgxiong-third-party-adapters（不要写死盘符）。

```powershell
git clone https://github.com/IM-DaXiong/bgxiong-third-party-adapters.git <实际开发根>/bgxiong-third-party-adapters
```

本仓库为第三方适配器 **Git 真源（公开）**。本机若另有产物/缓存根目录名，不得替代本仓库作为 git 远程。


## Wan 2.2 S2V 0.1.3 下载

[完整安装包与 SHA256](https://github.com/IM-DaXiong/bgxiong-third-party-adapters/releases/tag/wan22-s2v-v0.1.3)。解压后在比格熊「设置 → 第三方适配器」导入含 `adapter-install.json` 的目录。需要可连接的 ComfyUI 和 Wan 2.2 S2V 模型。实测自由画布和片段画布的音频选段、提交、出片与版本写回。

## Wan 2.2 Animate 0.2.13 候选下载

[完整安装包与校验信息](https://github.com/IM-DaXiong/bgxiong-third-party-adapters/releases/tag/wan22-animate-v0.2.13)。修复参数拒绝凭证缺少能力声明，明确窗口/重叠帧数字段错误。37项包测试、标准包校验及真实CLI拒绝回放通过，完整应用GUI/GPU成片验收待完成，按prerelease候选提供。解压后在应用“设置→第三方适配器”导入包含adapter-install.json的目录。

RunningHub H3 当前 SDK 完整包：[1.0.4 下载与 SHA256](https://github.com/IM-DaXiong/bgxiong-third-party-adapters/releases/tag/runninghub-h3-v1.0.4)。实际验证范围见 [分项记录](release/runninghub-h3-current-sdk-20261111.json)。

## MiniMax H3-Image 1.0.0-dev.3 实测预发布版

[完整包与 SHA256](https://github.com/IM-DaXiong/bgxiong-third-party-adapters/releases/tag/minimax-h3-image-v1.0.0-dev.3) · [验收记录](release/minimax-h3-image-20261111.json)。已实测应用安装、文生图、1–2 张参考编辑、自由画布工作台、片段画布、MCP 及同任务重启回收。仅 2048×2048；需要现有 ComfyUI 0.45.0+ 与模型权重，包内 companion 为依赖清单与冻结工作流，不包含推理环境或权重。导入包含 adapter-install.json 的目录；LAN 使用前按 README 同时填写 endpoint 和允许的 origin。

本机固定安装入口 `artifacts/current/local.bgx.minimax-h3-image-comfy/1.0.0-dev.3/`；远程机器从上述 ZIP 恢复该目录。正常应用发行安装器尚未验证。本版本不与 RunningHub H3 视频包混用。
