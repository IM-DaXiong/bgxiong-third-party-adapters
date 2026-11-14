# 全景编辑器接入

状态：P0 浏览器可行性及隔离框架已验证，P1 宿主接线进行中，1.1.0 仅为开发候选。当前公开的 1.0.3 安装包保持原字节；新候选尚未完成实际开发EXE/生成验收。

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

未验：实际开发EXE工程/安装、Comfy多图生成、全部3D/视频最终输出、安装升级及跨机恢复。`/panorama_suite/file_3d` 等剩余路由仍待接通；完整功能计划尚未完成，不能把编辑器能打开当作上游功能全量可用。
