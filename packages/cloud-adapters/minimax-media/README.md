# MiniMax 图像适配包

`image-text` 提供文生图，`image-character` 接收一张 JPEG 或 PNG 人物参考图。两项操作按包声明提供 `image-01` 和 `image-01-live`。提示词、画幅、种子和结果格式归适配包，主程序只传入选定操作与冻结素材。

绑定 MiniMax 图片账号后，SDK 把凭据限定在所配置的 API 来源。Base64 是默认结果格式。选择 URL 时，下载来源也须在安装时授权。生成后保留图片原件和 MiniMax 响应。

[MiniMax 文生图](https://platform.minimaxi.com/docs/api-reference/image-generation-t2i.md) · [MiniMax 图生图](https://platform.minimaxi.com/docs/api-reference/image-generation-i2i.md)
