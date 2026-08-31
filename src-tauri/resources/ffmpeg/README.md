# FFmpeg Native Libraries

此目录只保留说明文件，实际 Native Libraries 不提交 Git。

发布 workflow 在构建前准备固定版本 FFmpeg SDK：

- macOS 使用官方 FFmpeg 8.1.2 源码构建共享库，校验源码 SHA-256 后将 dylib 作为
  `Contents/Frameworks` 组件打包，并将依赖重定位到 `@rpath`。
- Windows 使用固定资产 ID 的 FFmpeg 8.1 LGPL shared SDK，校验 ZIP SHA-256 后将 DLL
  作为安装包根目录资源，与 `GBLab.exe` 放在同一目录。

每个 SDK 都生成 `manifest.json`，记录来源、版本、架构和校验值。开发机可以继续通过
`pkg-config` 使用系统 FFmpeg；Release 构建必须使用 workflow 准备的 SDK，终端用户无需
预装 FFmpeg。
