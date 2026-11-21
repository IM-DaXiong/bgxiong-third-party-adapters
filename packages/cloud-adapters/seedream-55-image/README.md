# Seedream 5.5 第三方适配器

包 ID 为 `local.bgx.seedream-55-image`，模型为 `doubao-seedream-5-5-261028`。在比格熊的第三方适配器设置中选择含 `adapter-install.json` 的版本目录。点账号配置，选择已有北京方舟 API Key 卡，或填写 `https://ark.cn-beijing.volces.com/api/v3` 与 API Key，由账号中心建卡或匹配已有卡。

安装并绑定后，图片工作台可选择 Seedream 5.5 文生图或参考图生图。参考图生图按素材栏顺序发送 1–10 张原图。单图最多 30,000,000 字节、两边各至少 15 像素、总像素最多 3600 万、宽高比在 1:16 至 16:1 内。接受宿主可测量的方舟参考图格式。当前宿主的 HEIC/HEIF 原图解码尚未通过集成验证。

默认 2K、PNG、自动背景、开启水印、URL 响应。尺寸可填 1K、1.5K、2K 或宽x高。透明背景须选 PNG。URL 模式在任务中立即下载原件，允许火山官方列出的 Seedream 下载源。b64_json 模式直接保存返回字节。输出图片预算为 32 MiB，响应 JSON 预算为 48 MiB。

收到原始响应后先保全响应和厂商用量，再验证并保存图像。若方舟受理状态未知或下载失败，先核对原回执与已存成果。URL 有效期为 24 小时。

厂商合同参见 [图片生成 API](https://docs.volcengine.com/docs/ark/image-generation-api?lang=zh)。结果源参见[火山引擎生成内容下载说明](https://docs.volcengine.com/docs/station-wide-acceleration/accelerate-large-model-generated-content-download?lang=en)。
