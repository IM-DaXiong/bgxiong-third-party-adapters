# RunningHub H3 标准动态多参独立适配器

当前权威源码：**1.0.1**；公开 SDK：**V2 2.1.1**（模板合同仍为 2.0.0）。本目录与 `packages/cloud-adapters/` 内其他厂商包同级，具有自己的 Cargo workspace，不加入宿主或 SDK workspace，不参与主程序编译。厂商工作流、HTTP 组装及响应解释只在本目录实现。

本包是官方基础 `MiniMax H3 Reference to Video` 的动态多参包：同一套 `build_graph`（删 LoadImage/LoadAudio、注入 1000+/2000+、接线 136、SaveVideo 92→9000）。底图没有 Turbo LoRA/Sampler，也没有 SpectrumApply。采样链直接接 UNET `127`，BasicScheduler 约 20 步。

| 项 | 本包（标准） | 已有包（冻结，勿改） |
|---|---|---|
| 路径 | `packages/cloud-adapters/runninghub-h3-r2v-dynamic` | turbo `runninghub-h3-dynamic`；spectrum `runninghub-h3-spectrum-dynamic` |
| pluginId | `local.thirdparty.runninghub-h3-r2v-dynamic` | 各自独立 |
| modelId | `runninghub-h3-r2v-dynamic` | `runninghub-h3-r2v-turbo-dynamic` / `runninghub-h3-r2v-spectrum-dynamic` |
| 选择器短名 | `H3-r2v` | `H3-r2v-turbo` / `H3-r2v-spectrum` |
| workflowId | `2119436868289912833`（原版 20 步，2026-11-08 发布） | `2100758868111486978` / `2116576147646533634` |

## 边界

- 图 **1–9**、音 **0–3**；`inputMode` 固定 `multi_image_to_video`；参考音 = 音色条件；成片音轨 = 模型生成（VAEDecodeAudio → CreateVideo）。
- 提交：`POST /openapi/v2/run/workflow/{WORKFLOW}`，body 为整图 JSON 字符串 + 空 `nodeInfoList`。
- 底图来自用户上传的 `video_minimax_h3_r2v.json`：原文旁路存 `workflows/video_minimax_h3_r2v.ui.json`，提交用 API 格式在 `src/workflow-api.json`，两者 20 个功能节点逐一对齐。
- 官方接口说明（提交、查询、上传、结果字段，以及本包为何多带 `workflow` 整图）：[docs/RUNNINGHUB-WORKFLOW-2119436868289912833.md](docs/RUNNINGHUB-WORKFLOW-2119436868289912833.md)。
- 不改 Turbo、Spectrum、经典 `H3-多参` 包。原声成片是独立包 `runninghub-h3-r2v-soundtrack`（工作流 `2119441622462193666`），成片音轨接用户原声，不并进本包。

## 本地验证

```powershell
cargo test --manifest-path packages/cloud-adapters/runninghub-h3-r2v-dynamic/Cargo.toml --locked
```

标准 SDK `build` / `check` / `pack` 已通过。可导入目录：

`L:\00Dev\bgxiong-third-party-adapters\artifacts\runninghub-h3-r2v-dynamic\1.0.1`

导入后它和现有 RunningHub 包一样进「RunningHub」一级，短名是 `H3-r2v`。本轮没有写入正在使用的应用数据库。

## 1.0.1

改绑到 RunningHub 新发布的原版 20 步工作流 `2119436868289912833`（旧号 `2117100865147457537` 不再使用）。output/resume schema 里的 `workflowId` 常量随之变化，因此升版；已装 1.0.0 不会自动变，需另行安装 1.0.1。1.0.0 受理的旧任务仍由 1.0.0 续查。提交协议、素材范围、参数和节点 9000 硬拒不变。本轮未打云端真实生成。

同版升级到 SDK 2.1.1：`vendor-sdk/` 与 `template-lock.json` 由 2.1.1 `init` 生成后替换（新增 `ResolutionEvidenceV1Outcome::completed`，SDK 自身 serde_json 去掉 `arbitrary_precision`），包代码无需改动；用主仓 `installer-user-examples/vendor-adapter-v2` 打包。
