# 项目骨架

GBLab 使用单仓库结构，由 Vue 前端、薄 Tauri 桌面壳和独立 Rust 核心库组成。Rust 核心不依赖 Tauri，可脱离桌面 UI 运行单元测试、协议测试和性能基准。

## 根目录

```text
GBLab/
├── .codex/                         # AI 长期项目知识
│   ├── README.md
│   └── knowledge/
├── .github/
│   └── workflows/                  # macOS、Windows 原生 CI
├── crates/
│   └── gblab-core/                 # 独立 Rust 模拟核心
├── docs/                           # 面向开发者和用户的正式文档
├── src/                            # Vue 3 前端
├── src-tauri/                      # Tauri 2 桌面壳
├── Cargo.toml                      # Rust workspace
├── Cargo.lock
├── CHANGELOG.md                    # Release Please 自动维护的变更日志
├── release-please-config.json      # 自动版本与发布规则
├── .release-please-manifest.json   # 当前发布版本清单
├── package.json
├── pnpm-lock.yaml
├── pnpm-workspace.yaml
├── tsconfig.json
├── vite.config.ts
├── vitest.config.ts
├── eslint.config.js
├── .prettierrc.json
├── .editorconfig
├── .npmrc
├── .nvmrc
├── rust-toolchain.toml
├── rustfmt.toml
├── clippy.toml
├── Justfile                        # 统一开发与验证入口
└── README.md
```

依赖锁文件必须提交。前端只使用 pnpm，Rust workspace 只维护根目录一个 `Cargo.lock`。

## Vue 前端

```text
src/
├── app/                            # 应用启动与全局装配
│   ├── router/
│   └── bootstrap.ts
├── features/                       # 按业务领域组织
│   ├── simulator/                  # 设备配置、派生通道与运行时状态；通过类型化 IPC 读写
│   └── settings/                   # 全局平台/设备配置类型、读取与保存 API
├── infrastructure/                 # 技术适配，不包含业务规则
│   └── tauri/                      # 类型化 command/event 客户端
├── layouts/                        # 桌面应用布局
├── pages/                          # 路由页面与业务编排
├── styles/                         # Naive UI 主题与全局样式
├── App.vue
├── main.ts
└── vite-env.d.ts
```

每个 `features/<name>/` 按实际需要包含 `api/`、`components/`、`composables/`、`stores/`、`types/` 和 `index.ts`，不为了目录对称创建空文件夹。跨 feature 只能从对方 `index.ts` 导入。

前端依赖方向：

```text
app → pages / layouts → features → infrastructure / shared
```

前端只持有展示状态和用户交互状态；设备权威运行状态、并发调度和资源生命周期均由 Rust 核心管理。

## Tauri 桌面壳

```text
src-tauri/
├── capabilities/                   # Tauri 权限能力配置
├── icons/                          # 应用图标
├── binaries/                       # CI 准备的平台 FFmpeg sidecar
│   └── README.md                   # 来源、版本、许可证与校验规则
├── src/
│   ├── commands/                   # 面向前端的薄 IPC 命令
│   │   └── mod.rs                  # 应用信息、SIP 配置和设备配置命令
│   ├── app_state.rs                # 桌面壳持有的核心句柄
│   ├── dto.rs                      # IPC DTO 与领域类型转换
│   ├── lib.rs                      # Tauri Builder 与插件装配
│   └── main.rs                     # 最小进程入口
├── Cargo.toml
├── build.rs
└── tauri.conf.json
```

`commands/` 只负责参数校验、调用核心 API 和结果映射，不实现 SIP、JSON 配置读写、FFmpeg 或设备状态机逻辑。新增业务命令按领域拆分文件；后端事件能力出现后，通过独立事件投影模块只发布适合 UI 消费的降频、批量快照。

`binaries/` 下的平台 FFmpeg 由准备脚本或 CI 放入，不直接维护来历不明的二进制；其版本、许可证、来源和哈希必须可追踪。

## Rust 模拟核心

```text
crates/gblab-core/
├── src/
│   ├── domain/                     # 纯领域类型与规则
│   │   ├── ids.rs                  # GB28181 DeviceId 与编码校验
│   │   ├── devices.rs              # 设备配置、批量规则与运行时通道派生
│   │   └── mod.rs
│   ├── application/                # 用例与业务编排
│   │   └── mod.rs                  # 当前提供 CoreService
│   ├── runtime/                    # Tokio 生命周期与性能机制
│   │   ├── mod.rs                  # 运行时公开契约与资源限制
│   │   ├── handle.rs               # 面向应用层的异步运行时句柄
│   │   ├── registration.rs         # 单 owner 监督器、命令与内部事件路由
│   │   ├── operations.rs           # 注册/注销 transient operation 执行器
│   │   ├── business.rs             # Alarm、MobilePosition、控制与通知业务
│   │   ├── platform.rs             # 平台请求、CmdType 与订阅状态机
│   │   ├── scheduler.rs            # 共享节拍调度器
│   │   ├── state.rs                # 运行态聚合与交互日志存储
│   │   ├── types.rs                # 运行态 DTO、事件与错误类型
│   │   └── time.rs                 # 运行时统一时间工具
│   ├── sip/                        # siprs 隔离层
│   │   ├── mod.rs
│   │   ├── registration.rs         # 共享 UDP 客户端、连接与 SIP 错误类型
│   │   ├── charset.rs              # GB2312、GBK、UTF-8 XML 编解码适配
│   │   ├── transport.rs            # UDP 收发、入站请求与服务端事务缓存
│   │   ├── dispatcher.rs           # Method/CmdType 分派与结构化响应
│   │   ├── session.rs              # 设备 CSeq、Digest、REGISTER、MESSAGE、NOTIFY
│   │   ├── transaction.rs          # Call-ID/CSeq/Method/branch 事务匹配
│   │   ├── dialog.rs                # INVITE 前置对话状态
│   │   ├── notify.rs                # NOTIFY 对话上下文
│   │   └── time.rs                  # SIP 热路径时间工具
│   ├── configuration/              # JSON 配置读写
│   │   └── mod.rs
│   ├── error.rs
│   └── lib.rs
└── Cargo.toml
```

核心依赖方向：

```text
domain
  ↑
application ← runtime
  ↑
sip / configuration
  ↑
src-tauri
```

`domain` 不依赖 Tokio、Tauri、JSON 配置实现或 FFmpeg；仅允许使用 `siprs-gb28181-codec` 这类纯协议编码类型校验国标编号，不直接依赖 SIP 传输、事务或 UA。`application` 定义设备生命周期和场景语义；外部适配模块实现协议、进程和配置读写能力。`src-tauri` 只装配并调用 `gblab-core`。

初始阶段只使用一个 `gblab-core` crate，通过 Rust module 保持边界。只有出现独立复用、独立发布或编译隔离的真实需求时，才拆分为多个 crate。

新增设备、通道、场景、调度、协议、仓储和媒体行为时，在对应模块内按单一职责拆分文件。跨模块行为测试放入 `tests/`，性能基准放入 `benches/`；没有真实测试或基准前不创建空目录和占位文件。

## 文档、脚本与 CI

```text
docs/
├── architecture.md                 # 对外架构说明
├── gb28181-compatibility.md        # 标准能力与平台兼容矩阵
├── performance.md                  # 指标定义、基准方法和结果
└── release.md                      # macOS、Windows 发布说明

.github/workflows/
├── ci.yml                          # 前端与平台无关 Rust 检查
├── release.yml                     # Release PR、Tag、双平台构建与 GitHub Release
├── build-macos.yml                 # macOS 手动原生构建入口
└── build-windows.yml               # Windows 手动原生构建入口
```

通用开发命令通过 `Justfile`、pnpm scripts 和 Cargo workspace 暴露。只有出现无法由这些入口表达的真实平台资源准备流程时，才新增 `scripts/`。
