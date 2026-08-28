# Bundled FFmpeg Native Libraries

平台发布构建在此目录放置与当前目标架构匹配的 FFmpeg 8 Native Libraries：

- macOS：`libavcodec`、`libavformat`、`libavutil`、`libavdevice`、`libavfilter`、`libswresample`、`libswscale` 的 `.dylib`。
- Windows：对应 `.dll` 和链接阶段需要的 `.lib` 文件。

这些文件由平台 CI 的固定版本准备步骤生成或下载，不提交到 Git。CI 必须在目标平台锁定
FFmpeg 8 版本、记录来源和 SHA-256 后再复制到本目录；不要直接使用 runner 的系统 FFmpeg。
Tauri 会将本目录内容复制到应用资源根目录，macOS 通过应用内置 rpath 加载，Windows 将 DLL 放在
可执行文件旁边；终端用户无需单独安装 FFmpeg。
