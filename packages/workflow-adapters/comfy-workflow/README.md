# ComfyUI 工作流适配器

此包分析、编译和执行 ComfyUI 工作流。`execute` 可对 LTX 2.5 时序图提交 `timing: { seconds, fps }`。适配器从所选视频输出追溯到图中真实时长及帧率来源，以节点定义校验 `seconds × fps + 1` 及视频、音频和条件支路，再写入两个 Primitive。未传 `timing` 时沿用源图。缺少上述时序结构的图会拒绝 `timing`，仍可按原图执行。

`local.thirdparty.comfy-workflow/reference-slots` 第 1 版接受 `source` 或受管 `sourceRef`、编译 `context` 和一个 `selection.outputKeys` 图片输出。返回原件摘要、编译图摘要、`referenceImage` 的上下限和按 Autogrow `template.names` 声明排列的槽位，已有 `LoadImage` 连线包含实际执行节点和输出索引。传入可选的 `referenceCount` 可校验张数并取得前 N 个槽位。当前操作覆盖具名 IMAGE Autogrow 槽，图中无该声明时返回 `referenceImage: null`。操作只读，保持已存在的图和连线。

## 作者构建与工作流资源

从本目录运行 `tools/build_author.py`。Python 需为 3.11 或以上，并安装同批公开 SDK 指定的 Rust 工具链及 `wasm32-wasip2`。`--sdk-source` 指向带 `templates/` 的同批 SDK 源码目录，`--sdk-runtime` 指向含 CLI 和 `adapter-sdk-v2/` 的运行时目录。该 SDK 须提供 `operation-input-preflight-v2` 和 `websocket-v1`。

```sh
python tools/build_author.py build --sdk-source /path/to/sdk/source --sdk-runtime /path/to/sdk/runtime --workflow examples/scale-image.api.json --out /path/to/new-author-bundle
```

`--workflow` 可重复指定。工具将适配包源码和 `crates/comfy-workflow-core` 的编译源码以原字节暂存为独立作者项目，只重写暂存 `Cargo.toml` 中编译内核的相对路径。SDK 使用锁定依赖构建，并验证标准运行包。首次构建允许 SDK 从 crates.io 取得锁定依赖。如果实建 Wasm 摘要与仓库 `manifest.json` 锁定值不同，工具会停止构建，保留原版本包身份。

- `author/` 是独立作者源码，含该次编译内核和 SDK 镜像，可用同批 SDK 对该目录执行标准 `build`。
- `runtime/` 是通过标准 SDK 检查的运行包。`workflows/` 按原件 SHA-256 命名，根目录 `workflow-templates.json` 提供同一批模板的原件摘要。
- `bundle-index.json` 记录源文件及暂存副本摘要、SDK 工具身份，以及运行包和工作流文件的摘要。`author-bundle.zip` 包含该索引和全部列出的文件。恢复前先核对 `author-bundle.sha256` 再解压。

在已构建或恢复的目录按原件 SHA-256 定位工作流，命令会重新验证文件字节：

```sh
python tools/build_author.py find --bundle /path/to/author-bundle --sha256 <ORIGINAL_SHA256>
```

示例使用 ComfyUI 内置 `EmptyImage`、`ImageScale`、`SaveImage` 和明示的输出节点。可以复制示例并在 `ImageScale.inputs` 按 `/object_info` 声明改 `crop` 或 `upscale_method`，用改后原件重新构建作者资源。作者资源用于验证和恢复，应用的正式安装信封及工作流登记由发行流程管理。

## 时序回归

认证源回归使用主仓 `src-tauri/tests/fixtures/comfyui_ltx2_5`，通过 `BGX_COMFY_TIMING_FIXTURES` 指定该目录后，在本包执行：

```sh
cargo test --locked --lib certified_ltx25_preparation -- --ignored
```
