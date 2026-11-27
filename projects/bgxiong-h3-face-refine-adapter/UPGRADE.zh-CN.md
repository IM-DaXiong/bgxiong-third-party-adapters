# 升级指南（作者）

1. 只改本包与受管 companion；不要改主程序/EXE。
2. 上游节点：更新 `locks/upstream-comfyui-h3-facerefine.sha` 与 `companion/upstream/` 到新 commit；同步 `companion/upstream-snapshot.json`。
3. 若 1.1.2→更新版变更 picker 路由或 `H3PerFrameDenoise` 接线，同步 `workflows/` 与 `src/lib.rs` 映射。
4. 重新 prepare → probe → 原子激活；失败不得自动回退执行用户新请求。
5. 当前基线：`d8521d14fe0d721d80cd9417fff5a559cbc21aba`。

### 1.0.2

使用同一安装信封更新 WASM 和 Comfy 节点。安装后待当前 Comfy 任务结束，再由宿主重载节点。健康接口须报告 1.0.2 与 sharedTracking，并能读取 BGXFaceTrack 的节点声明。新工作台支持选脸后直接生成。主动预览与精修输入一致时共用逐帧轨迹。遮挡帧保留原画，跟踪命中的源帧贴回精修结果。旧任务按原包版本和任务 ID 查询。
