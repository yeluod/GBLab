# GBLab

GBLab 是面向开发联调与压测的 GB28181 多设备模拟器桌面应用，支持 macOS 与 Windows。

## 技术栈

- Tauri 2、Vue 3、TypeScript、Naive UI
- Rust、Tokio、siprs
- JSON 配置文件（运行时数据不落盘）
- 按需使用 FFmpeg sidecar

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
