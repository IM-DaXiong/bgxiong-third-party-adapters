# OpenAI 兼容图像网关

本包承接已明确配置为 `image_openai_compat` 的图像请求，保留原即梦 OpenAI 兼容通道的网关协议。

- `dall-e-3-text` 对应现有 DALL·E 3 文生图登记。
- `gateway-text` 和 `gateway-reference` 使用适配器账号显式填写的 `gatewayModelId`。它可以是厂商模型 ID 或推理接入点 ID。

账号需要配置 `endpointBaseUrl` 和 `credentialRef`，由 SDK 将 Bearer API Key 限定到已授权网关来源。网关路径规范为 `/v1/images/generations`，包括已填写完整图片生成路径或从同一路径的 `/chat/completions` 切换。

提交支持 `url` 和 `b64_json`，默认 `url`。参考操作支持 1–9 张图片，按工作台顺序从 SDK 租约读取，单张传字符串，多张传数组。结果保留原始图片字节、原响应和厂商上报的用量。成果下载使用单独的来源授权，网关凭据只用于生成请求。连接在提交后断开或无法确认受理时报告受理未知。

`cargo +1.95.0 test --locked --offline` 验证准确的参考图报文、凭据隔离、两类成果和受理未知。用同批公开 SDK 作者 CLI 执行构建、打包及离线检查。
