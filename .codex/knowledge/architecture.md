# 架构与交付

## 产品定位

GBLab 是面向开发联调与压测的 GB28181 多设备模拟器桌面应用，支持 macOS 和 Windows。产品以高密度信令模拟为核心，并支持受控数量的真实媒体模拟。

## 技术栈

- 桌面 UI：Tauri 2、Vue 3、TypeScript、Naive UI。
- 核心：Rust、Tokio、`siprs`（`letmlook/sip`）提供 SIP 与 GB28181 协议能力。
- 持久化：SQLite 单文件数据库；JSON 用于配置导入与导出。
- 媒体：FFmpeg 作为按需启动的外部平台资源。
- 工具链：Rust 1.98.0、Node.js 26.7.0、pnpm 11.19.0；TypeScript 固定为 6.0.x。

## 核心分层

- `Device Manager`：管理设备生命周期、分组、批量操作与状态聚合。
- `Scenario Engine`：编排注册、保活、目录、掉线、重连与点播等场景。
- `SIP Adapter`：隔离业务代码与 `siprs` API。
- `Media Adapter`：按需启动并管理 FFmpeg 媒体任务。
- `Persistence`：保存平台、设备、通道、场景和应用设置。

## 性能约束

- 设备使用轻量状态机与 Tokio task，不为每台设备创建 OS 线程、独立 socket 或 FFmpeg 进程。
- 网络 socket 按平台或本地端口复用；使用有界队列与有界并发控制突发操作。
- 高频 SIP 消息、运行状态和日志不在消息路径上同步写入 SQLite；日志应异步、批量并可采样。
- UI 应批量、降频刷新运行状态，避免每条协议消息触发界面更新。
- FFmpeg 仅在需要真实媒体的通道上启动；信令设备数与媒体并发数分别配置和统计。

## 构建与发布

- 不采用跨平台交叉编译交付。
- macOS 应用只在 macOS 构建、签名与公证。
- macOS 最低系统版本为 11.0。
- Windows 应用只在 Windows 构建、签名并生成安装包。
- CI 使用 macOS 与 Windows 原生 runner 分别完成构建、测试、签名与发布。
- FFmpeg 按 macOS 与 Windows 目标平台分别打包和验证。
