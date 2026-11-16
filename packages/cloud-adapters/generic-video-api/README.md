# 通用视频 API

本包承接比格熊现有通用视频 API。账号绑定需要填写 `endpointBaseUrl`、SDK 的 `credentialRef` 和服务提供的 `upstreamModelId`。在工作台明确选择「通用视频 API」后再选择生成方式。

- 文生视频提交至 `POST /v1/videos/text2video`。
- 图生视频和运镜提交至 `POST /v1/videos/image2video`，首帧原件由 SDK 租约读取后编码到 `first_frame_b64`。
- 多图生视频提交至 `POST /v1/videos/multi_image2video`，按用户顺序把选中原件编码到 `reference_images_b64`。
- 账号地址已有 `/v1` 路径时保留该路径。接口额外参数可在 `providerParameters` 里填写，模型、提示词、生成方式和媒体由所选操作管理。

成功受理后按原始任务号请求 `GET /v1/videos/{taskId}`。生成完成时保存供应商原始响应和视频原件。SDK 把 Bearer 限定在 API 来源，下载来源按安装绑定授权。已报用量保留供应商的输入、输出和总 token 数。接口未提供取消操作。

本地合同测试使用 `cargo +1.95.0 test --locked --offline --test contract`。构建、打包和验证使用冻结 SDK 的 `bgx-adapter-v2 build`、`pack` 和 `check`。本候选尚未接入主程序默认入口。
