# LTX 2.5 第三方适配器

`pluginId=local.bgx.ltx25`，版本 `0.1.1`。一个包提供三个操作，在视频工作台里选择对应模型即可。

## 三个模式

- **LTX 2.5 文生视频**（`local.bgx.ltx25/t2v`）：按提示词生成视频。本地图会同时生成画面与音轨。
- **LTX 2.5 图生视频**（`local.bgx.ltx25/i2v`）：以一张首帧图和提示词生成视频。本地图同样会生成画面与音轨。
- **LTX 2.5 音频生视频**（`local.bgx.ltx25/a2v`）：输入既有音频，冻结该音频，生成与之对齐的画面，输出附带原音。这不是精确口型或数字人保证；官方专用对口型方案仍是 LTX-2.3 Dub-It，本包未接入。

## 模型文件与目录

权重不随包提供，需放到本机 ComfyUI 的 `models/` 惯例目录。提示词扩写在本包中固定关闭，但增强器权重仍必须放置（校验会覆盖未使用的分支）。

| 用途 | 目录 | T2V / I2V（INT8） | A2V（官方 BF16） |
|---|---|---|---|
| 主模型 | `models/diffusion_models/` | `ltx-2.5-22b-distilled-transformer-comfy-int8-convrot.safetensors` | `ltx-2.5-22b-distilled-transformer-bf16.safetensors` |
| 文本编码 | `models/text_encoders/` | `gemma4-12b-with-proj-ltx-2.5-comfy-int8-convrot.safetensors` | `gemma4-12b-with-proj-ltx-2.5-bf16.safetensors` |
| 增强器 | `models/text_encoders/` | `gemma4_e2b_it_int8_convrot.safetensors` | `gemma4_e2b_it_bf16.safetensors` |
| Video VAE | `models/vae/` | `ltx-2.5-video-vae-bf16.safetensors` | 同名 |
| Audio VAE | `models/vae/` | `ltx-2.5-audio-vae-bf16.safetensors` | 同名 |
| 空间上采样 | `models/latent_upscale_models/` | `ltx-2.5-latent-spatial-upscaler-x2-bf16-1.0.safetensors` | 同名 |

三模式不得互换权重栈。本机若缺少某模式的文件，连通性与提交会按该模式显式报告，不会静默改用另一套权重。

节点：较新的官方 ComfyUI（含 LTX 2.5 原生节点）即可；剪掉 Gemma API 分支后不需要额外自定义节点包。A2V 无首帧时还会用到内置 `EmptyImage`。

## 与官方 A2V 的差异

- 提示词扩写固定关闭（官方默认开启 Enhance positive prompt）。
- 删除 Gemma API 分支，固定走本地文本编码。
- 默认输出尺寸为 1280×704（官方成片默认约 1920×1088，本包不采用）。

## A2V 默认提示词

选择「LTX 2.5 音频生视频」且提示词为空时，工作台会自动填入下面这段英文。用户可以修改；已有内容不会被覆盖。执行器与提交不会再补默认词。

```text
The person shown in the first frame remains the same person and talks while looking toward the camera. Mouth movement, blinking, and small natural shifts in expression and head position follow the timing of the supplied speech. Face, hair, clothing, and background stay consistent with the first frame. This is a single continuous take with a static camera, holding a medium close-up of the head and shoulders, with no pan, zoom, or cut. Lighting stays unchanged from the opening shot.
```

中文说明（不会进入提交字段，也不能代替英文默认词）：

> 首帧里的人保持同一身份，对着镜头说话。口部动作、眨眼和细微的表情、头部变化跟随**已提供语音**的节奏。面孔、发型、衣着和背景与首帧一致。单镜头、固定机位、中近景（头肩），不推拉摇移、不切换。光线与开场相同。

这段模板不是官方逐字原文，也不保证音素级对口型。无首帧时，请自行改写首句。T2V / I2V 没有这段默认词。

## 帧数公式与音频不足

- T2V / I2V 帧数：`时长 × 帧率 + 1`。
- A2V 帧数：`1 + floor(时长 × 帧率 / 8) × 8`。
- 实际成片时长按公式计算，不会暗改裁尾。A2V 还要求 `帧率 × 时长 ≥ 8`。
- 若宿主能提供音频时长，而从「音频起点」算起不足所选时长，提交会被拒绝。若宿主未提供时长（例如缺少探测工具），按当前推荐策略仍会提交；音频不足时成片可能变短。

## 取消

取消在本包中声明为不支持，不会调用 ComfyUI 的 `/interrupt`，因此取消不会中断已经进入 ComfyUI 队列的任务。

## 安装

只从本仓库 `artifacts/current/local.bgx.ltx25/0.1.1/` 选择安装（目录内有 `adapter-install.json`）。安装后把它登记为视频模型，并在软件里配置本机 ComfyUI 服务地址。本包安装信封使用 `comfyui.serviceOnly`，不再写死 `127.0.0.1:8188`。

安装包校验或离线合同检查通过，不代表真机生成已经通过。

## 已知限制

工作台会把 A2V 的模式标签显示为「单图生视频」，因为宿主 `inputMode` 没有独立的音频生视频枚举。这是已知限制，不是安装错误。
