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
│   ├── stores/
│   ├── primevue.ts
│   └── bootstrap.ts
├── features/                       # 按业务领域组织
│   ├── dashboard/
│   ├── platforms/
│   ├── devices/
│   ├── channels/
│   ├── scenarios/
│   ├── media/
│   ├── logs/
│   └── settings/
├── infrastructure/                 # 技术适配，不包含业务规则
│   ├── tauri/                      # 类型化 command/event 客户端
│   ├── telemetry/
│   └── runtime/
├── layouts/                        # 桌面应用布局
├── pages/                          # 路由页面与业务编排
├── shared/                         # 无业务归属的稳定复用能力
│   ├── components/
│   ├── composables/
│   ├── constants/
│   ├── types/
│   └── utils/
├── styles/                         # PrimeVue token、主题与全局样式
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
│   │   └── mod.rs                  # 当前提供核心状态查询
│   ├── app_state.rs                # 桌面壳持有的核心句柄
│   ├── dto.rs                      # IPC DTO 与领域类型转换
│   ├── lib.rs                      # Tauri Builder 与插件装配
│   └── main.rs                     # 最小进程入口
├── Cargo.toml
├── build.rs
└── tauri.conf.json
```

`commands/` 只负责参数校验、调用核心 API 和结果映射，不实现 SIP、SQLite、FFmpeg 或设备状态机逻辑。新增业务命令按领域拆分文件；后端事件能力出现后，通过独立事件投影模块只发布适合 UI 消费的降频、批量快照。

`binaries/` 下的平台 FFmpeg 由准备脚本或 CI 放入，不直接维护来历不明的二进制；其版本、许可证、来源和哈希必须可追踪。

## Rust 模拟核心

```text
crates/gblab-core/
├── migrations/                     # 内嵌 SQLite schema migrations
├── src/
│   ├── domain/                     # 纯领域类型与规则
│   │   ├── ids.rs                  # 当前提供 DeviceId
│   │   └── mod.rs
│   ├── application/                # 用例与业务编排
│   │   └── mod.rs                  # 当前提供 CoreService
│   ├── runtime/                    # Tokio 生命周期与性能机制
│   │   └── mod.rs                  # 当前提供有界资源配置
│   ├── sip/                        # siprs 隔离层
│   │   └── mod.rs
│   ├── media/                      # FFmpeg 与媒体会话管理
│   │   └── mod.rs
│   ├── persistence/                # SQLite 仓储实现
│   │   ├── database.rs
│   │   └── mod.rs
│   ├── observability/              # tracing、指标与日志采样
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
sip / media / persistence / observability
  ↑
src-tauri
```

`domain` 不依赖 Tokio、Tauri、SQLite、FFmpeg 或 `siprs`。`application` 定义设备生命周期和场景语义；外部适配模块实现协议、进程和存储能力。`src-tauri` 只装配并调用 `gblab-core`。

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
├── build-macos.yml                 # macOS 原生构建、签名、公证
└── build-windows.yml               # Windows 原生构建与签名
```

通用开发命令通过 `Justfile`、pnpm scripts 和 Cargo workspace 暴露。只有出现无法由这些入口表达的真实平台资源准备流程时，才新增 `scripts/`。
