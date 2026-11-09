# Wan S2V 语音驱动（隔离候选）

一个受管人物图和一段已准备的驱动音频，经原生 Wan2.2-S2V 工作流生成口型视频。当前源码候选 `0.1.1`，未公开发行，未签软件工作台验收。

使用新 `video.speech` / `video-speech-v1` / `operation-preflight-v1`，不接受旧Avatar协议、TTS音色参考或普通背景音用途。用户的原音频选段、权限和目标版本由宿主负责；本包只接受实际选段的受管引用与真实时长/内容SHA256。

目前操作：`local.bgx.wan22-s2v/speak-standard`，固定认证图标准分支、20步采样（CFG6，不加载T2V加速LoRA）、16fps和原声合成。宽高分别256–640、64对齐，种子显式传入；音频1–10秒。原图在包内由认证图原有VAE路径适配尺寸，原媒体不改写。可选动作要求完整传入，不截断。额外参数会拒绝。

预检不上传、不生成，仅使用已绑定的资源探测回执和媒体摘要。依赖未知时阻断；有原生资源回执且输入合法才报告ready。三段续接时序来自原图：每段N个latent，最终帧数12N-2；向上量化视频长度，预计尾差不超过750ms，真实尾差必须在最终文件验证。不会裁音、变速或补静音。

包结果为一个受管视频及冻结输入来源回执。音轨存在性、音/视频各自时长、实际帧数/fps由应用finish owner在下载后探测，不能把本包预检预计值当作实测数据。SDK工具出片同样需要外部ffprobe/解码及完整听看验收；不代表软件用户路径已通过。

## 认证来源

`workflows/source.json` 是原始 `角色替换/video_wan2_2_14B_s2v.json` 的逐字节副本，SHA256 `9b017cde7f0564968cc7d2d2d3d044741a991e6662674b3913d5d77f92e9af71`。完整保留两个分支。`tools/compile_workflow.py` 从标准分支SaveVideo 168反向展开可达节点，解析原子图输入/输出、primitive与显式bypass，产物24个原生执行节点；未用简化测试图替代。原图负向文本存在编码损坏，当前原样保留，未暗中换提示词。

`workflows/node-map.json` 保存每个展开节点的原图作用域和节点ID。`object-info.json` 保存编译时显式依赖声明快照。当前SaveVideo的COMFY_DYNAMICCOMBO_V3由原auto/auto值编码为format="auto"和format.codec="auto"点路径，交由服务执行层重建正式嵌套结构；不是换视频算法或换模型。

`src/comfy.rs` 源自同仓seedvr2-image包的受控SDK HTTP交换实现，本包独立维护视频结果检查。只通过SDK管理端口联网/读取媒体；上传到Comfy的两个素材均在明确submit动作内完成。远端prompt受理后先提交SDK checkpoint；丢失响应不重发submit。取消不受支持，由SDK如实呈现。

需要现有Comfy服务提供：WanSoundImageToVideo/Extend、AudioEncoderLoader/Encode、原生采样/VAE/视频节点及主模型、wav2vec2、umt5、VAE。模型不入Git，不自动安装或下载。纯本地无认证服务配置credentialRef=null；有认证服务保留自己的认证引用。

## 构建和安装边界

`build.ps1` 在本包/同级公共SDK内构建wasm，调用公共SDK pack生成新目录候选包。构建源码不依赖产品应用私有模块。安装模板在installation中；manifest摘要必须由实际构建生成，模板不能直接当可安装成品。无需自定义节点companion；模型/Comfy资源按现有产品包另行安装和验收。


标准分支在原UI中整体关闭；明确选择终端168时启用该组，但保留可选第四段174的旁路，维持原三段拓扑。此前0.1.0加速候选的工具链通过，但明确人声样本口型未签通过，源码及实物保全，不用标准分支的结果替它签通过。

标准候选已通过标准 SDK 工具实跑：两秒明确人声生成 512×512、34 帧、16 fps 的视频，视频 2.125 秒、原声音轨 2.000 秒，完整解码通过。抽帧可见嘴部开合；逐音素同步和应用工作台未验收。产物摘要与实测数据见 `SOURCE.json`。
