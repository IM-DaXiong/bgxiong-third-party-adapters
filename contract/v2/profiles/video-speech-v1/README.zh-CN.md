# video-speech-v1（隔离源码草案，未注册）

本目录属于 T02 隔离源码实施：已写入 control probe 分支、原生 validator、CLI/reference-host 路由和 runtime 端口限制，已生成Rust/TS类型并同步模板。validator定向行为、工具/runtime编译及既有六操作回归已通过；新预检worker验收见计划最新记录。模型/GUI验收NOT_RUN；不得将本目录作为可安装包或已发布 SDK。0.6.1111 当前打包不包含这些文件。

capabilityKey 为 `video.speech`，transportSlot 为 `video`，与 TTS `speech-v1` 分离。operation 身份与配置留在既有 V2 envelope，不复制进 input。`portrait` 和 `drivingAudio` 是各一个受管 mediaId 的数组；mediaSlots 的 role 分别为 `portrait`、`driving_audio`，inputPointer 分别为 `/portrait`、`/drivingAudio`。其他参考素材必须由操作 schema 和 mediaSlots 一起显式声明，不能经 parameters 偷带本机路径。

应用层持有原音频身份与 startMs/endMs，在事务外准备实际选段。包只接收选段的真实时长、采样率、声道和 SHA256，不接收 projectId、目标节点或数据库路径。instruction 保留用户完整原文，允许空字符串。基线 parameters 是闭集空对象；包按支持面声明属性、默认值与范围。普通参考音、音色参考和 soundtrack_copy 不构成口型驱动能力。

包完成结果只有一个受管视频引用和冻结输入来源回执，不把预计帧数/音轨时长冒充实测。应用finish owner必须在下载后独立ffprobe/解码，核实音轨、尺寸、fps、帧数、音/视频各自时长并完整试听，才能形成应用完整结果；缺音轨不得报成功，来源回执不是验收证明。结果固定新增视频版本，不覆盖源图、原音或旧视频。原声重编码允许，静默裁音、变速、补静音不允许。

预检合同见 `preflight.schema.json` 引用的公共 `schemas/operation-preflight.schema.json`。请求使用 modelId/capabilityKey/operationId/operationVersion/inputSchemaDigest、包操作 input、冻结 configuration、requestDigest 和 mediaMetadata；不重复发明选择身份。原生 owner 以 CapabilityRef 和 Binding 核包版本、配置 revision/digest，再用包 schema 校验 input。metadata 中 durationMs 是实际准备后的音频时长；sourceInterval 和 sourceDurationMs 是原音频选段边界，两者一起出现且必须匹配准备后的时长。预检不接媒体句柄、URL、凭据或原始字节。

本版预检只允许 host-log.log，其余 host ports 在 reference-host 和 runtime 两层拒绝；包不能访问 HTTP、媒体、凭据、状态或 checkpoint。未知依赖返回 blocked，不借预检执行生成、扣费、上传或内容分析。HTTP declarative profile 尚无此计划，显式返回 UNSUPPORTED_FEATURE，不借用 connectivity。两种新 feature 必须列为 requiredFeatures。用户接受建议后更新实际区间，再预检；ready 必须给预计规格和音视频尾差上限；adjustment_required 必须给非空原因与候选区间；blocked 不可带可提交的 ready 结果。

已写入但未运行：control-operations oneOf 分支（旧两类闭集结构保留）、服务端响应种类/摘要匹配、host-feature catalog、输入媒体摘要/时段语义、reference-host/CLI 路由、执行端口限制，以及五项行为测试源码。类型生成器已登记 public preflight 与 speech input/output；未执行生成器，其入口含确定性/冻结摘要断言，保留在禁测试解除后运行。待完成：生成类型及模板受控同步、SDK版本核定、包声明和submit/result完整profile校验、实际行为测试、适配器仓镜像。host catalog 源码有 feature 登记不代表当前发行或安装实例已支持。

摘要边界：包回显 requestDigest 只用于关联，不是授权令牌。应用服务自行产生 preflightDigest，绑定包/操作/schema/configurationRevision、媒体身份与内容摘要、选段和参数；submit 重新校验。Schema 无法表达 startMs < endMs、区间落在原时长内、候选不得超原音频、输出回执与请求一致等关系，须在唯一 Rust owner 实现，前端不得另写权威规则。

环境回执：请求 environmentReady 由原生目录的所选配置连通性回执核对，不能由UI自行声明。未知环境不能返回ready。CLI在明确的operation_preflight调用内先运行既有connectivity probe，保存其回执，再注入实测目录状态；预检guest仍仅允许日志，不上传原始素材。
