# 构建与发布

GBLab 使用 Release Please 和 GitHub Actions 自动维护版本并发布桌面安装包。正式产物不使用交叉编译：macOS 在 macOS runner 构建，Windows 在 Windows runner 构建。

## 自动发布流程

`.github/workflows/release.yml` 在每次推送 `main` 后运行：

1. 普通功能提交只创建或更新 Release PR，不立即创建 Tag 或构建安装包。
2. Release PR 根据 Conventional Commits 更新 Rust workspace、`package.json`、`src-tauri/tauri.conf.json`、`Cargo.lock` 和 `CHANGELOG.md`。
3. 合并 Release PR 后，Release Please 创建 `v<version>` Tag 和草稿 GitHub Release。
4. macOS runner 构建 DMG，Windows runner 构建 NSIS 和 MSI，并将安装包上传至该草稿 Release。
5. 两个平台全部成功后发布 Release；任一平台失败时草稿保持未发布状态，避免提供不完整版本。

项目只维护一个应用版本。根 `package.json` 是 Release Please 的发布组件，配置中的 `extra-files` 同步 Cargo workspace、`Cargo.lock` 中的本地 crate 和 Tauri 配置版本。

版本变化由提交类型决定：

- `fix: ...`：补丁版本。
- `feat: ...`：次版本。
- `feat!: ...` 或提交正文包含 `BREAKING CHANGE:`：主版本。
- `docs:`、`test:`、`chore:` 等默认不改变版本，也通常不写入变更日志。

## 仓库设置

工作流需要 `contents: write`、`pull-requests: write` 和 `issues: write` 权限。仓库必须允许 GitHub Actions 创建 Pull Request。

默认使用 `GITHUB_TOKEN`。建议创建仓库 Secret `RELEASE_PLEASE_TOKEN`，值为具有当前仓库 Contents、Pull requests 和 Issues 写权限的 fine-grained PAT 或 GitHub App Token。使用该 Secret 后，Release PR 的创建和更新可以触发仓库中的其他 Pull Request 检查；没有配置时仍可完成发布流程，但 GitHub 的防递归机制可能不会为机器人创建的 Release PR 自动触发其他工作流。

## 手动构建

`.github/workflows/build-macos.yml` 和 `.github/workflows/build-windows.yml` 保留为手动构建与平台排查入口。也可以在当前平台本地执行：

```bash
pnpm install
pnpm run tauri:build
```

## 签名与外部资源

- macOS 正式分发前需要配置 Apple 签名与公证。
- Windows 正式分发前需要配置代码签名。
- FFmpeg 按目标平台准备，版本、来源、许可证和 SHA-256 必须可追踪。
- 签名凭据不得进入仓库、构建日志或 `.codex`。

当前自动工作流尚未配置签名凭据，因此生成的是未签名安装包。
