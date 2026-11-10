# RunningHub Qwen Image 2.1

比格熊工作站的 SDK V2 图片适配器，一个安装包提供两个模型入口。

- **文生图**调用 RunningHub 工作流 `2119749068753825793`，用 `nodeInfoList` 写入提示词、负向词、画幅、像素量和可选固定种子。
- **多参考图编辑**调用 RunningHub 工作流 `2119750262788284418`，按工作台顺序上传 1～10 张图片，将返回的 ComfyUI 文件名接到对应参考图槽。包内 API 图取自用户提供画布的直接生成分支。按画幅和像素量计算 32 对齐宽高，输出 PNG。云端须返回包内输出节点 9000，才能将结果回写为本次成果。

两种入口的像素量范围为 0.1～16 MP，默认 1 MP。1:1 选择 16 MP 时请求 4096×4096。16:9 选择 8 MP 时请求约 3840×2160，实际请求宽高按工作流对齐。4K 云端出图尺寸待实测。默认画幅为 1:1，种子 -1 由平台随机分配。图像编辑以工作台输入为编辑指令，参考图预处理沿用画布设置。任务受理后保存任务号，用原任务号查询并取回图片。

文生图节点路径依据用户提供的画布及 ComfyUI 子图执行 ID 规则展开。两种模式仍需在目标 RunningHub 账号上验收，分别核对云端参数覆盖和工作台图片回写。当前包为安装候选。参考 [RunningHub 节点参数文档](https://www.runninghub.cn/runninghub-api-doc-cn/doc-8287336) 和 [资源上传文档](https://www.runninghub.cn/runninghub-api-doc-cn/api-425749007)。

## 构建

```powershell
cargo test --locked --offline
./Build.ps1 -Sdk '<公开 SDK V2 作者分发目录>' -OutputDirectory '<尚不存在的输出目录>'
```

## 导入候选包

打开「设置 → 第三方适配器」，导入包含 `adapter-install.json` 的完整目录，再在工作站填写 RunningHub API Key。在生图工作台选用文生图或多参考图编辑入口，填写提示词、画幅及像素量。多参考图编辑按顺序添加 1～10 张图。运行资源托管在 RunningHub，安装包无需本地 companion。
