# 可灵 4.0 Omni

支持显式的文生视频和 1 至 10 张有序参考图。视频时长 3–30 秒，清晰度 720p 或 1080p。创建请求使用北京 API，素材按 SDK 受管句柄读写。

支持经典 `kling-v3` 和 Omni `kling-image-o1`、`kling-v3-omni` 图像操作。经典文生图与单图图生图走 `/v1/images/generations`。Omni 文生图与 1–9 张参考图生图走 `/v1/images/omni-image`。参考图通过 SDK 媒体租约按用户顺序编码。任务沿原任务号查询并收取原图，保留原始回执及可灵上报的积分。

安装声明使用 `credentialMode: "jwt-hs256"` 支持 AK/SK 账号。宿主协商 `jwt-host-time-v1` 并为 `credentialRef` 提供仅限北京 API 的 JWT 凭据租约，以 AK 作为公开标识，以 SK 作为签名密钥。提交和查询时由 SDK 使用宿主 UTC 签发，媒体源下载走独立的无凭据请求。原有缺省模式的配置继续接受 SDK 管理的已签发 Bearer。

- 安装候选目录含 package、adapter-install.json 和 LICENSE。通过「设置 → 第三方适配器 → 安装适配包」选择目录，并选用已有可灵 AK/SK 账号或录入新的密钥对。
- 适配器保存原任务号供查询和成果回收。云端未确认受理时请核对原任务。
- 视频参考、编辑、首尾帧及音色接线仍在后续阶段。正式发行以真实账户和成果回收验收为准。

开发复验使用本批 SDK 的 CLI/worker 执行 Build.ps1。包内原生测试运行 cargo test --locked --offline --lib。测试使用合成请求和媒体。
