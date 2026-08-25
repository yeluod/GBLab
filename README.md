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

## 发布

`main` 分支使用 Release Please 自动维护版本号、`CHANGELOG.md`、`v<version>` Tag 和 GitHub Release：

1. 功能提交合并到 `main` 后，Release Please 创建或更新 Release PR。
2. 合并 Release PR 后，GitHub Actions 分别在 macOS 与 Windows 原生 runner 构建 DMG、NSIS 和 MSI。
3. 两个平台的安装包全部上传成功后，草稿 Release 自动发布；任一构建失败时保留草稿，不发布不完整版本。

提交信息使用 Conventional Commits，例如 `feat: ...`、`fix: ...`。仓库设置与完整发布流程见 [docs/release.md](docs/release.md)。
