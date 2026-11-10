# 适配器开发指南

从当前独立 SDK 发布目录的 `README.md` 和 `AUTHORING.zh-CN.md` 开始。通过 `bgx-adapter-v2 init` 建作者目录，按其中的 WIT、协议和 schema 实现能力。

- 厂商报文、工作流节点和状态映射归各适配器。
- 用同一 SDK 的 `build`、`check` 和 `pack` 构建。检查通过后按[安装说明](USAGE.zh-CN.md)导入完整包。
- ComfyUI 作者参考见 [v2 工作流示例](../packages/comfy-workflow-adapter/v2/)。
- 需要密钥的包通过账号绑定取得密钥。最终验收分别记录构建、登记、真实生成和回写。
