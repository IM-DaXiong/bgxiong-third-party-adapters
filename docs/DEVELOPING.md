# 适配器开发指南

从当前独立 SDK 发布目录的 `README.md` 和 `AUTHORING.zh-CN.md` 开始。通过 `bgx-adapter-v2 init` 建作者目录，按其中的 WIT、协议和 schema 实现能力。

- 厂商报文、工作流节点和状态映射归各适配器。RunningHub H3 的能力声明、参数和构建入口见[源码说明](../packages/cloud-adapters/runninghub-h3-all/README.md#源码入口)。
- 用同一 SDK 的 `build`、`check` 和 `pack` 构建。检查通过后按[安装说明](USAGE.zh-CN.md)导入完整包。
- ComfyUI 作者参考见 [v2 工作流示例](../packages/comfy-workflow-adapter/v2/)。
- 需要密钥的包通过账号绑定取得密钥。最终验收分别记录构建、登记、真实生成和回写。

## 用户可见显示名称

用户安装后看到的包标题、默认账号名、别名和操作展示 label，按本规范取名。适用于以后所有新适配包、示例和模板。

- 只用模型名，或厂商 + 模型 / 工作流，例如 `RunningHub-千问-Image-2.1`、`MiniMax-H3-Continuum`、`Seedream 5.5`。
- 品牌大小写按官方写法：MiniMax、RunningHub、ComfyUI、DeepSeek、FLUX.2、Seedream、Seedance、SeedVR2、Breeze、VoxCPM、Qwen / 千问、LTX。
- 以 H3 开头的名称冠以 MiniMax-H3。RunningHub 上的 H3 包显示为 `RunningHub-MiniMax-H3`，不要附加「经典七工作流」。
- 不得出现内部身份前缀：`local.bgx`、`local.thirdparty`、`local.independent`、`local.example`、`local.zeroauthor`、`local.tts` 等。
- 不得出现 `.sdkv2`、`SDK v2` 或其他内部技术尾标；也不要把 Zero Author、independent、author package、author example 等内部措辞写进显示名。
- 内部 `pluginId`、`operationId`、API `modelId` 和账号 / 路由绑定不随显示名改动。
- 默认账号名和别名应使用上述显示名（通常即 manifest `displayName`）。不要把内部 ID 当作标题。若宿主仍回退到 pluginId，属主程序问题，不在本仓改宿主。
