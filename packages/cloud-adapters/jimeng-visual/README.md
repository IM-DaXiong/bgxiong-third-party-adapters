# 火山智能视觉适配器

即梦视觉使用火山引擎 IAM 的 Access Key ID 与 Secret Access Key。安装时由宿主管理账号和源站授权，默认 API 源站为 `https://visual.volcengineapi.com`。适配器只接收受管凭据引用，具体签名由 SDK 按 `derived-hmac-credential-v1` 完成。

操作参数先经 `operation-input-v1` 冻结。异步提交只发出一次，续办请求使用受理回执的原任务号和原请求键。结果按下载字节提交给 SDK，下载源站需纳入宿主网络授权，签名头只发往签名源站。

视频 3.0 720P 提供文生、首帧、首尾帧与运镜。1080P 提供文生、首帧与首尾帧。Pro 提供文生与首帧。SeedEdit 3.0 和人像写真各收一张 JPEG 或 PNG 底图。SeedEdit 在包内生成受限尺寸和体积的 JPEG 请求素材。人像照片保留应用原有 `CVSubmitTask` / `CVGetResult` 和底图 Base64 合同。

本机作者构建使用同批冻结 SDK 的 CLI 和 Worker，详见独立 SDK 作者文档。使用合成素材和回环 HTTP 可以验证签名、提交、原任务查询与文件摘要。

平台合同见[火山引擎鉴权签名](https://docs.volcengine.com/docs/machine-translation/authentication-signature?lang=zh)、[即梦视频 3.0 Pro](https://docs.volcengine.com/docs/jimeng-ai/jimeng-ai-video-generation-30-pro-interface-documentation?lang=zh)、[即梦视频 3.0 720P](https://docs.volcengine.com/docs/jimeng-ai/jimeng-ai-video-generation-30720-p-interface-documentation?lang=zh)、[即梦视频 3.0 1080P](https://docs.volcengine.com/docs/jimeng-ai/jimeng-ai-video-generation-301080-p-interface-documentation?lang=zh)和[SeedEdit 3.0](https://docs.volcengine.com/docs/large-model-of-image-generation/graphics30-instruction-editing-seededit30?lang=zh)。
