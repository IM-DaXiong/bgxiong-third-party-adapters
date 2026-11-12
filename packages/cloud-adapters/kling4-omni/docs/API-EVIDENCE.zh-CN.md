# 官方合同取证（2026-11-12）

状态：公开 Markdown 通过普通 HTTPS 成功读取；以下为自行归纳，不包含官网全文。原文URL和字节摘要便于复核；尚无账号权限或云端调用验证。

- omni: https://klingai.com/document-api/api/video/4-0/video-omni.md; SHA256 `df1ad42167cdf35471794372b869aaeb77517d1bcdbc81d8b6faf8c0c3b9add8`.
- guide: https://klingai.com/document-api/api/usage-guide/kling-4-0.md; SHA256 `c19092e941c2506d383e2132737e358713030cc89ab69bb1596591867961ec6c`.
- auth: https://klingai.com/document-api/api/get-started/authentication.md; SHA256 `f442a831be5732c15ca028926c48c2db12e5e29099a938ce8e36c74252c05974`.
- index: https://klingai.com/document-api/llms.txt; SHA256 `c57e2ac35f1d31f45371ae246ac04075a83aa6527fb25db5578fd74bd25373b8`.
- errors: https://klingai.com/document-api/api/get-started/error-codes.md; SHA256 `751ffe8f6603b7b0069b3dbdd66b03e59a214bdfc47656084d857cd97d6349ef`.
- concurrency: https://klingai.com/document-api/api/get-started/concurrency-rules.md; SHA256 `fa976bf8a9fa7ee0d6aa6a73e9234524ba61075c97bbc38dafd788088cc10d1e`.
- elements: https://klingai.com/document-api/api/video/4-0/elements.md; SHA256 `f4fdb9435e7e64f39f3d92a3c752f0b88c0aa81b0a73250a9cb873d7de2e9cad`.
- voice: https://klingai.com/document-api/api/video/4-0/voice-customization.md; SHA256 `22afbd52ddf1ec04e141c2437ba46b366819742cbb30df21db341a01767597aa`.

## 基础接入已核事实

- 北京 API host 为 api-beijing.klingai.com，新API使用Bearer API Key；不使用旧AK/SK JWT。
- 创建是POST /omni-video/kling-4.0；模型在路径中，不发model_name。contents是数组；文本项使用type=prompt、text。settings承载duration/resolution/aspect_ratio/audio，options承载external_task_id/watermark_info。
- 显式时长为3–30整数；清晰度720p/1080p；画幅21:9、16:9、9:16、1:1、adaptive。未传duration的上游自动推断不能被宿主默认值覆盖。
- 图片可URL/base64；JPEG/PNG、30MB、最小300px、比例2:5–5:2、最多10张。MB的精确字节口径、混合字符分类仍未定义，不能编造。
- 查询GET /tasks使用task_ids或external_task_ids且只能一种；按返回data数组中的id匹配。submitted/processing/succeeded/failed，不沿用旧task_status字段。
- 输出按type=video选择video.url；计费量为十进制字符串。查询/download失败不能重新POST创建；external_task_id没有exactly-once保证。
- 主体和音色管理的旧式任务形状与Omni不同；本包尚不实现云资产创建/列表管理。
- 文档索引未列出可供本地视频自动上传的通用入口；视频URL来源保持BLOCKED_EXTERNAL。此结论不是断言上游绝无上传服务。
- 素材上限、原声/音色互斥和角色要求已读；完整映射和SDK validator fixture尚未实现，不标产品可用。
