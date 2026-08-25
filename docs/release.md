# 构建与发布

GBLab 不使用交叉编译生成正式桌面产物：

- macOS 在 macOS runner 构建 `.app`，正式分发时完成签名与公证。
- Windows 在 Windows runner 构建 NSIS 与 MSI，正式分发时完成代码签名。
- FFmpeg 按目标平台准备，版本、来源、许可证和 SHA-256 必须可追踪。

本地开发使用：

```bash
pnpm install
pnpm run tauri:dev
```

当前平台原生构建使用：

```bash
pnpm run tauri:build
```

CI 中的 macOS 与 Windows 构建工作流可手动触发。签名凭据不得进入仓库、日志或 `.codex`。
