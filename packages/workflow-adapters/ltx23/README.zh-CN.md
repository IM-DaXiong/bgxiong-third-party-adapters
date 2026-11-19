# LTX 2.3

生产包 `local.bgx.ltx23` 0.1.2。四个操作：文生视频、图生视频、图音频生视频、首尾帧生视频。预检按 `selection.operationId` 识别操作，受理回执写入 `result`。已经选过更早版本的节点，安装后请再选一次。

图来自主仓已关闭 Prompt Enhance 的 `ComfyUI-API/LTX2.3`。教学包 `local.example.comfy-ltx23-ia2v` 的 8 秒 / 768 / 24fps 不是本包的参数表面。

默认 15 秒、25fps、1280×704。帧率 6–120。宽高为 64 的倍数，范围 256–2048，因此包含 704×1280。

取消声明为 `unsupported`。缺 Comfy 节点或权重时由连通性探针报错，不卸载与 2.5 共用的权重。
