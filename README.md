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
线程独占 FFmpeg 输入、解码器、重采样器、tempo 和 bitstream filter；Tauri 命令只通过有界命令队列控制
打开、播放、暂停、停止、关闭、Seek 和单帧操作，不直接持有 FFmpeg context。

- MP4 只解封装一次；H.264/H.265 通过 FFmpeg bitstream filter 输出 Annex-B，AAC 直接透传。
- 编码音视频统一归一化到 90 kHz 单调时间线，再通过有界 fan-out 提供给后续 GB28181 Live consumer。
- Preview 使用独立的有界可丢帧队列和二进制 Tauri IPC，不以 JSON 数组传输 RGBA，也不会阻塞 Live consumer。

当前媒体核心只提供 MP4 播放、预览和编码 Live consumer，不提供本地录制、录像文件写入、历史回放或上级平台 `RecordInfo` 录像查询。后续 MPEG-PS、RTP 和 SIP `INVITE` 媒体会话应直接消费现有 `EncodedMediaPacket`，不得重复打开全局 MP4 源。

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

桌面构建统一由 .github/workflows/build-desktop.yml 提供；FFmpeg SDK 的版本、来源策略、架构和许可证统一记录在 toolchains/ffmpeg-sdk.lock.json。Windows 构建按锁定的 FFmpeg 8.1、win64、LGPL shared 资产名解析 BtbN 当前发布，并使用 GitHub 提供的 SHA-256 做内容寻址缓存；实际 Release、资产 ID、下载地址和摘要写入 SDK manifest。CI 在缓存恢复后重新校验 SDK，不依赖会被清理的日期型 autobuild URL。

## 发布

`main` 分支使用 Release Please 自动维护版本号、`CHANGELOG.md`、`v<version>` Tag 和 GitHub Release：

1. 功能提交合并到 `main` 后，Release Please 创建或更新 Release PR。
2. 合并 Release PR 后，Release 工作流以 Release Please 输出的精确提交 SHA 调用统一桌面构建，在 macOS 与 Windows 原生 runner 构建 DMG、NSIS 和 MSI。
3. Builder 只上传 Actions artifact；Publisher 校验完整性、生成 SHA256SUMS.txt，再一次性上传并发布草稿 Release。任一构建失败时保留草稿，不发布不完整版本。

提交信息使用 Conventional Commits，例如 `feat: ...`、`fix: ...`。
