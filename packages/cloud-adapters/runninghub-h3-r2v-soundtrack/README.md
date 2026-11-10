# RunningHub H3 原声动态多参独立适配器

当前权威源码：**1.0.1**；公开 SDK：**V2 2.1.1**（模板合同仍为 2.0.0）。本目录与 `packages/cloud-adapters/` 内其他厂商包同级，具有自己的 Cargo workspace，不加入宿主或 SDK workspace，不参与主程序编译。厂商工作流、HTTP 组装及响应解释只在本目录实现。

本包是官方基础 `MiniMax H3 Reference to Video` 的原声成片包。采样链与 `runninghub-h3-r2v-dynamic` 相同（无 Turbo、无 Spectrum，BasicScheduler 20 步，UNET `127`）。差别只在成片音轨：上传的那一段音频同时接到节点 136 的 `ref_audio_0` 和 `CreateVideo` 130，不再使用 `VAEDecodeAudio` 121 的解码音。

| 项 | 本包（原声） | 生成音轨包（冻结，勿改） |
|---|---|---|
| 路径 | `packages/cloud-adapters/runninghub-h3-r2v-soundtrack` | `runninghub-h3-r2v-dynamic` |
| pluginId | `local.thirdparty.runninghub-h3-r2v-soundtrack` | `local.thirdparty.runninghub-h3-r2v-dynamic` |
| modelId | `runninghub-h3-r2v-soundtrack` | `runninghub-h3-r2v-dynamic` |
| 选择器短名 | `H3-r2v-原声` | `H3-r2v` |
| workflowId | `2119441622462193666` | `2119436868289912833` |
| 参考音 | 恰好 1 段，`soundtrack_copy` | 0～3 段，`voice_timbre` |
| 成片音轨 | 用户原声（LoadAudio → CreateVideo） | 模型解码音（VAEDecodeAudio → CreateVideo） |

## 边界

- 图 **1–9**、原声 **恰好 1**；`inputMode` 固定 `multi_image_to_video`。
- 提交：`POST /openapi/v2/run/workflow/{WORKFLOW}`，body 为整图 JSON 字符串 + 空 `nodeInfoList`。
- 底图来自用户上传的 `video_minimax_h3_r2v_soundtrack.json`：原文旁路存 `workflows/video_minimax_h3_r2v_soundtrack.ui.json`。提交用 API 格式在 `src/workflow-api.json`。相对生成音轨图，只多节点 141，并把 130 与 136 的参考音都接到 141。
- 官方接口说明：[docs/RUNNINGHUB-WORKFLOW-2119441622462193666.md](docs/RUNNINGHUB-WORKFLOW-2119441622462193666.md)。
- 不改 `H3-r2v`、Turbo、Spectrum、经典 `H3-多参`。

## 本地验证

```powershell
cargo test --manifest-path packages/cloud-adapters/runninghub-h3-r2v-soundtrack/Cargo.toml --locked
```

可导入目录：

`L:\00Dev\bgxiong-third-party-adapters\artifacts\runninghub-h3-r2v-soundtrack\1.0.1`

导入后进「RunningHub」一级，短名是 `H3-r2v-原声`。本轮没有写入正在使用的应用数据库，也没有云端真实生成。
