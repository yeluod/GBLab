# FFmpeg Native Libraries

GBLab 使用 `rsmpeg` 在进程内调用 FFmpeg Native Libraries，不启动 FFmpeg sidecar
进程。开发机可以安装 FFmpeg 的 headers/libraries 供 Cargo 编译和调试；发布包必须将
FFmpeg Native Libraries 不手工放入仓库。平台 workflow 通过
`.github/actions/prepare-ffmpeg` 准备固定 SDK，并在 Tauri bundle 前将 macOS dylib 放入
`Contents/Frameworks`、Windows DLL 放到可执行文件同目录。

目录中的库文件不直接提交仓库。准备流程必须固定 FFmpeg 版本，记录官方来源、许可证和
SHA-256，并分别在 macOS 与 Windows 原生 runner 验证。用户安装 GBLab 后不需要预装 FFmpeg。
