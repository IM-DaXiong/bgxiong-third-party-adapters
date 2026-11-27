# 在独立启动的 ComfyUI 安装小脸精修

本适配器的 ZIP 同时包含比格熊适配器和 ComfyUI 所需的两个节点目录。用比格熊安装适配器后，在运行 ComfyUI 的机器上用同一个 ZIP 安装节点。保留当前的启动方式和服务地址。

先从可信发布入口下载完整 ZIP 并核对发布的 SHA-256，再解压到临时目录。下例第一项替换成当前 ComfyUI 启动时用的 Python，第二项指定解包后的脚本，`--comfy-root` 指向 ComfyUI 的 `main.py` 所在目录。

```powershell
& "D:\ComfyUI\.venv\Scripts\python.exe" -X utf8 ".\install-comfyui-nodes.py" --check --comfy-root "D:\ComfyUI"
```

如果当前服务设置了不同的 `custom_nodes` 路径，再加 `--custom-nodes "实际节点目录"`。服务器是 Linux 时同样用当前 ComfyUI 的 Python 运行该脚本。

等 ComfyUI 当前的运行和等待任务全部完成，再正常停止该实例。把上面的 `--check` 改成 `--install` 执行安装。脚本校验安装信封及所有节点文件的 SHA-256，仅更新小脸精修的两个节点目录。同名旧目录会移到 `custom_nodes` 同级的 `.bgx-face-refine-backups`。自动切镜用包内的离线依赖。模型文件和其他 Python 依赖沿用当前 ComfyUI。

按原方式启动服务器，再用同一个脚本确认实际服务已加载节点。填写比格熊里配置的 ComfyUI 地址。

```powershell
& "D:\ComfyUI\.venv\Scripts\python.exe" -X utf8 ".\install-comfyui-nodes.py" --verify-url "http://127.0.0.1:8188"
```

检查通过后，在比格熊点「分析目标」。如果当前 ComfyUI 正在生成，分析任务会进入同一队列等待执行。
