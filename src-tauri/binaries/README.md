# FFmpeg Sidecar

本目录由平台资源准备流程或 CI 放入对应目标平台的 FFmpeg 可执行文件。二进制文件不直接提交仓库。

引入 FFmpeg 时必须固定版本，记录官方来源、许可证和 SHA-256，并分别在 macOS 与 Windows 原生 runner 验证。
