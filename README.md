# GBLab

GBLab 是面向开发联调与压测的 GB28181 多设备模拟器桌面应用，支持 macOS 与 Windows。

## 技术栈

- Tauri 2、Vue 3、TypeScript、Naive UI
- Rust、Tokio、siprs
- JSON 配置文件（运行时数据不落盘）
- `rsmpeg` 进程内调用 FFmpeg Native Libraries，不启动 FFmpeg 子进程

开发机可以使用系统 FFmpeg 与 `pkg-config` 编译调试。正式发布固定 FFmpeg Native SDK
版本并随 macOS/Windows 安装包分发，终端用户无需预装 FFmpeg；发布库保持 LGPL
许可与来源、版本、校验值可追踪。

## 媒体核心

应用只维护一套全局媒体源，所有模拟设备和通道共享。`GlobalMediaRuntime` 的专用 owner
线程独占 FFmpeg 输入、解码器、重采样器和编码器；Tauri 命令只通过有界命令队列控制
打开、播放、暂停、停止、关闭、Seek 和单帧操作，不直接持有 FFmpeg context。

- MP4 只解封装一次；H.264/H.265 通过 FFmpeg bitstream filter 输出 Annex-B，AAC 直接透传。
- Camera 只采集和编码一次；视频输出 H.264/H.265，麦克风可关闭或编码为 AAC/G711A/G711U。
- 编码音视频统一归一化到 90 kHz 单调时间线，再通过有界 fan-out 独立提供给 Preview、Recorder 和 Live consumer。
- Preview 使用可丢帧的独立队列和二进制 Tauri IPC，不以 JSON 数组传输 RGBA，也不会阻塞录像或直播消费者。
- macOS 使用稳定 AVFoundation `uniqueID` 保存摄像头选择；Windows 使用 FFmpeg DirectShow 枚举设备并通过 DirectShow `IAMStreamConfig` 读取真实采集模式。

当前媒体核心不包含 MPEG-PS、RTP、SIP `INVITE` 媒体会话、实际录像文件写入和历史回放；这些能力应直接消费现有 `EncodedMediaPacket`，不得重新打开全局 MP4 或 Camera 源。

## 开发环境

- Rust 1.98.0
- Node.js 26
- pnpm 11
- just

## 开始开发

```bash
nvm use
pnpm install
pnpm run tauri:dev
```

## 验证

```bash
just verify
```

macOS 与 Windows 应用分别在对应原生平台构建和验证。

## 发布

`main` 分支使用 Release Please 自动维护版本号、`CHANGELOG.md`、`v<version>` Tag 和 GitHub Release：

1. 功能提交合并到 `main` 后，Release Please 创建或更新 Release PR。
2. 合并 Release PR 后，GitHub Actions 分别在 macOS 与 Windows 原生 runner 构建 DMG、NSIS 和 MSI。
3. 两个平台的安装包全部上传成功后，草稿 Release 自动发布；任一构建失败时保留草稿，不发布不完整版本。

提交信息使用 Conventional Commits，例如 `feat: ...`、`fix: ...`。
