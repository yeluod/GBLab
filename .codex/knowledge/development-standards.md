# 开发规范

本规范适用于 GBLab 的 Rust 核心、Tauri 桌面层、Vue 前端、测试、构建配置与 CI。规则优先级依次为：用户当前明确指令、当前代码和配置、本文档、同模块既有惯例。代码和配置始终是最终事实源。

## 通用原则

- 先明确需求边界、输入输出、失败路径、资源影响和验证场景；一个改动不夹带无关重构。
- 先定义外部契约、状态边界和模块归属，再编写实现；不依赖隐式约定。
- 新增依赖前确认标准能力和现有依赖不能合理解决问题，并评估维护状态、许可证、体积、性能与安全风险。
- 复用已存在的领域类型和模块能力；只有存在真实复用需求时才抽象，不引入单实现 trait、薄 wrapper 或未使用的预留配置。
- 删除或重构功能时同步删除失效代码、配置、测试、注释和依赖。
- 代码、日志、测试样例、`.codex` 与提交记录中不得出现密钥、令牌、密码、证书、私钥或真实敏感媒体地址。

## 项目结构与依赖方向

Rust 核心按业务和技术边界组织：设备管理、场景编排、SIP 适配、媒体适配、JSON 配置与桌面命令层各自拥有明确职责。业务层不直接依赖 Tauri UI、FFmpeg 进程细节或 `siprs` 的内部 API；通过明确的 adapter 接口协作。

Vue 前端按以下方向组织新增能力：

```text
app → pages / layouts → features → infrastructure / shared
```

- `app` 负责应用装配与全局能力；`pages`、`layouts` 只负责编排。
- `features` 按业务领域拥有 API、状态、服务和持久化规则，并通过 `index.ts` 暴露公开能力。
- `infrastructure` 提供 Tauri IPC、存储、日志等技术适配，不包含设备、通道和场景等业务规则。
- `shared` 只放稳定、无业务归属的组件、组合式函数、类型和纯函数，不能成为公共堆放区。
- 跨模块只能导入公开入口，底层模块不得反向依赖上层模块。

## Rust 核心规范

- 使用 Rust 2024 edition；具体 MSRV 和依赖版本以项目 `Cargo.toml` 与工具链文件为准。
- 默认禁止 `unsafe`、`unwrap()`、`expect()`、`panic!()`、`todo!()`、`unimplemented!()`、`dbg!()`、`println!()` 与 `eprintln!()`；错误使用明确错误类型并保留足够上下文。
- 库和领域代码使用 `thiserror` 定义错误；桌面应用边界可使用 `anyhow` 聚合和呈现错误，但不得丢失可诊断上下文。
- module/file、函数、字段使用 `snake_case`；struct、enum、trait 使用 `UpperCamelCase`；常量使用 `SCREAMING_SNAKE_CASE`。协议缩写采用 Rust 风格，例如 `SipMessage`、`RtpPacket`。
- 具有稳定业务语义的设备 ID、通道 ID、平台 ID、场景 ID 等优先使用 newtype，避免裸字符串在模块间传播。
- 参数优先借用；仅在跨 task 持有或需要持久化时取得所有权。热路径和循环中避免无意义 clone，引用计数使用 `Arc::clone`。
- `pub` 表示稳定公开 API；能私有则私有，按需使用 `pub(super)`、`pub(crate)`。不得只为测试扩大生产可见性。
- 注释和公共文档使用中文，解释原因、边界、并发顺序或风险；`TODO` 必须有明确跟踪编号或归属。

## 异步、资源与性能规范

- 长期运行模块采用单一 owner 的 Actor/Event Loop 或等价状态机；跨模块通信使用语义匹配的 Tokio 有界 channel。
- 所有 channel 必须明确容量、满载时的背压/拒绝语义以及关闭行为；禁止无界队列和无界 task 创建。
- 每个虚拟设备是轻量状态机，不创建 OS 线程、独立 socket 或常驻 FFmpeg 进程。socket 按平台或本地端口复用。
- 使用 `CancellationToken` 管理设备、场景和应用关闭；由 `JoinSet` 或等价机制监督 task。每个长期 task 都必须有退出路径，外部 IO 必须有 timeout。
- 不得跨 `.await` 持有锁；锁只覆盖最小临界区。优先使用消息传递而非共享可变状态。
- JSON 配置只保存可恢复的应用配置；SIP 消息、运行状态与高频日志不落盘。
- 运行日志异步批量写入、支持限量或采样；UI 通过批量和降频事件刷新状态，不能逐条协议消息触发渲染。
- FFmpeg 仅为已启用真实媒体的通道启动；其生命周期由 Media Adapter 管理，必须限制媒体并发数、超时与退出清理。

## 前端与 TypeScript 规范

- 使用 Vue 3 Composition API 和 `<script setup lang="ts">`；UI 优先使用 Naive UI 与项目已有封装。
- TypeScript 必须严格检查：禁止新增 `any`、`@ts-ignore`、无依据的断言和非空断言。外部输入用 `unknown` 接收，经校验后使用；仅类型导入使用 `import type`。
- 组件、类型、接口、枚举和类使用 `PascalCase`；组件文件和普通目录使用 `kebab-case`；变量、函数、Props、Emits 使用 `camelCase`；布尔值用 `is`、`has`、`can`、`should` 前缀；事件处理函数用 `handleXxx`，组合式函数用 `useXxx`。
- Props 只读，变更通过具名、类型化 Emits 表达；跨页面共享状态使用 Pinia，临时 UI 状态留在组件或 Hook 中。
- `computed` 表达派生状态，`watch` 仅处理必要副作用；订阅、计时器、事件监听和外部资源须在卸载时清理。
- 异步交互必须覆盖 loading、成功、空数据、失败、取消和重复提交；列表使用稳定业务键。
- Tauri command/event 的参数和返回值是明确、版本可控的 IPC 契约；前端不得绕过类型化封装直接散落调用。
- 用户可见文案默认使用中文；保持语义化结构、键盘可达性、表单标签、替代文本和图标按钮可访问名称。
- 样式优先使用 Naive UI 主题覆盖、CSS 变量和既有断点；避免硬编码主题色、脆弱 DOM 选择器与无理由的 `!important`。

## 数据、安全与诊断

- Tauri command 参数、文件路径、JSON 配置、SIP/SDP/XML 报文、FFmpeg 参数和平台响应均是不可信输入，进入领域逻辑前必须校验大小、格式、范围和业务关系。
- 不通过 shell 拼接启动 FFmpeg；使用 program/argv 边界。媒体路径、协议地址和端口必须经受控配置和校验。
- 日志使用结构化字段；错误保留操作、资源、超时和底层原因等诊断上下文，但不记录凭据、完整敏感报文或媒体访问令牌。
- 外部流程按“原始输入 → 已校验领域命令 → 执行规格 → Adapter”转换；校验失败不得部分执行或套用默认行为。

## 测试与质量门禁

- 为业务规则、外部输入校验、协议报文、状态机、取消/关闭、超时、背压、资源清理、JSON 配置读写、错误映射与已修复缺陷编写或更新自动化测试。
- Rust 私有行为的测试就近放在 `#[cfg(test)]`；公开协议和跨模块行为放在 `tests/`。测试不访问真实 GB28181 平台、真实媒体服务或真实网络资源。
- Rust 异步、时间、文件和进程测试须有 timeout，使用临时目录和 fake adapter，且不依赖执行顺序。
- Vue 测试使用 Vitest；测试名描述条件与可观察结果，覆盖正常、边界和失败路径。网络、时间、存储和全局对象需受控，异步测试等待最终可观察状态而非固定延时。
- 代码提交前应按改动范围运行格式检查、严格类型检查、Lint、相关单元/集成测试与构建；工具命令以项目脚本和 CI 配置为准。
- Rust 质量门禁默认包含 `cargo fmt --check`、`cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` 和 `cargo test --locked`；前端质量门禁默认包含 Prettier、ESLint、类型检查、Vitest 与生产构建。
- macOS 与 Windows 原生构建、平台打包与 FFmpeg 验证由各自平台的 CI runner 完成；本机验证不替代目标平台验证。
