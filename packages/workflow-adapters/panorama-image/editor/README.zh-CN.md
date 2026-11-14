# 全景编辑器接入

1.11.0候选：原始渲染移除无效种子/采样步数，尺寸倍数开放上游完整8–128、步长4范围。工程中没有扩展名的派生素材沿SDK已测量的文件名上传，不再因未使用的预览附件提前失败。13项native用例及派生素材反例通过，标准SDK build/check/pack/check通过。作者依赖恢复log0.4.34；SDK独立缓存缺项通过正式在线构建补齐，未更改锁定版本。实际新EXE与模型推理仍待验证。

1.10.0候选：工程生成改用显式file槽，SDK测量真实格式、保留PNG/视频/3D原件与原扩展名。增加原始Studio渲染、Qwen局部修补返回、三维场景和MoGe新视角四个图片操作；Frame/180/360声明使用SDK2.4的media-projection-v1。局部修补使用原示例scene/sticker/sticker_state连线；MoGe显式指定已保存源素材名称。13项native映射/端口检查、标准SDK build/check/pack/check和真实Frame/180编辑保存复开通过，尚未实际EXE提交或真实模型验收。1.10候选保留；原始渲染尚显示不适用的采样参数、尺寸倍数仅列常用值，下一候选修正，不同版本覆盖。

状态：全量接入仍在开发。1.7.0候选已在实际开发EXE走通受管3D转换的模拟HTTP链及原任务离线收取；1.8.0工程保全和1.9.0连线继承候选通过浏览器检查。1.9.0实际EXE已通过安装、连线读取、编辑/保存复开、父工程保全及过期输入拒绝。当前公开的1.0.3安装包保持原字节；候选不冒充正式可下载新版或真实模型推理验收。

`upstream.lock.json` 是本批上游来源锁。编辑器直接构建冻结的上游 Vue/Three 实现，`studio.js` 只提供宿主上下文、保存回执和资源加载，不复制场景、时间轴或渲染规则。上游 MIT 及字体 OFL 随资源交付；本仓代码许可不改变。

准备一个来源锁指定提交的上游 checkout，在其中按锁执行 `npm ci --ignore-scripts --no-audit --no-fund`。不需要 Python、ComfyUI、模型或浏览器下载。随后运行：

```powershell
node editor/build.mjs --source <upstream-checkout> --out <new-output-directory>
node editor/browser-check.mjs --source <upstream-checkout> --bundle <output-directory> --browser <installed-browser-executable> --results <evidence-directory>
node editor/check-upstream.mjs --source <upstream-checkout> --out <report.json> --remote
```

命令从本包根执行。构建拒绝错误提交、已修改的跟踪源码、摘要不一致和已存在的输出目录。构建清单逐文件包含摘要；不自动安装、替换运行包或重启服务。浏览器检查使用实际构建产物，覆盖三张图片三个方向、UI 方位编辑、撤销/重做、保存/复开、重复关闭无多余保存及控制台错误，保存两个窗口尺寸的截图。它不是生成效果验收。

同步检查只读比较提交，退出码 2 表示存在待审差异，网络失败不会记为最新。传入 `--compare <commit>` 可比较本地已有的候选提交；节点、编辑器、状态、时间、渲染、资源路由、工作流、依赖/许可和测试都纳入差异，不能仅比较版本号或 README。语义审核和受影响行为验证完成后再更新锁、升适配器版本并按正常安装流程交付。

当前已验：上游 2.1.1 的源工厂可独立打开；构建后的编辑器在 Windows Edge / AMD Radeon 610M 上通过上述场景检查。加 `--frame <compiled-sandboxFrame.js>` 可验证产品实际隔离框架：无父DOM/Tauri访问，三方向图像实际绘出，同名图片独立导入后复开，保存冲突保留编辑且可重试。编辑器铺满宿主内容区，布局偏好按包隔离保存。首次上游测试的 favicon 404 已由测试服务提供空图标响应解决；首份构建遗漏字体、blob相对样式路径和内嵌WASM被CSP阻止均经浏览器检出并定向修复。没有开放HTTP或脚本eval。

P1的标准SDK构建、源码包check、运行包pack/check及适配器原有9项行为测试通过。作者包已消费当前SDK生成镜像，SDK模板过期Cargo.lock源摘要在独立SDK仓修复并公开同步；没有放宽校验器。`Build.ps1` 接受明确的 `-Sdk`、`-Cli`、`-Worker`、`-EditorDirectory`、`-OutputDirectory`，校验编辑器来源与摘要后写入安装信封，附带字体和上游许可。不得覆盖已存在的输出目录。

实际开发EXE的工程/安装、MCP同核编辑和三方向生图协议/落盘链已验；多方向图与file_3d使用本机模拟服务，未验模型推理或资源机转换。file_3d现在通过唯一SDK调用，需主程序采用SDK2.3.0/de24abf3及协商能力opaque-file-upload-name-v1。

1.9.0将连线工程快照传入原上游继承编辑器；每个Studio只接受一个上游场景，多场景连线显式拒绝。下游只保存override，真实生成图以PANO_SCENE输出0连接多个Studio，不把场景对象接到图片端口。输入变更摘要由宿主公共owner核对。浏览器验证上游图层、下游覆盖、原文保全及复开；不能推广为所有3D/视频输入已验。

仍未完成：180°/视频完整生成链、Frame/局部修复返回、3D/深度/动画正式输出、其余示例的产品操作接线、安装升级当前引用及跨机恢复。完整功能计划尚未完成，不能把编辑器能打开或API映射通过当作上游全功能可用。
