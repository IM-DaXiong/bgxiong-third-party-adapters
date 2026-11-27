# 百炼 Qwen3-ASR-Flash

使用北京 OpenAI 兼容识别接口，发送已裁剪的单段 WAV。

使用「设置」中已配置的北京业务空间百炼账号。支持纯文本识别。

The adapter declares whole-file chat and progressive dictation policies through `std/speech.transcribe.policy@1`. The bundled file recognition operation returns one result per accepted batch.
