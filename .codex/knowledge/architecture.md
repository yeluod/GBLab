# 架构与交付

## 产品定位

GBLab 是面向开发联调与压测的 GB28181 多设备模拟器桌面应用，支持 macOS 和 Windows。产品以高密度信令模拟为核心，并支持受控数量的真实媒体模拟。

## 技术栈

- 桌面 UI：Tauri 2、Vue 3、TypeScript、Naive UI。
- 核心：Rust、Tokio、`siprs`（`letmlook/sip`）提供 SIP 与 GB28181 协议能力。
- 配置：JSON 配置文件；持久化唯一 SIP 服务配置、设备配置和唯一一次批量添加标记。设备注册状态、派生通道、平台订阅、SIP 交互与日志不落盘。模拟器配置在 UI、IPC 和 JSON 中均使用原始明文，不进行加密或脱敏；Unix/macOS 配置文件仅允许当前用户读写。
- 媒体：FFmpeg 作为按需启动的外部平台资源。
- 工具链：Rust 1.98.0、Node.js 26.7.0、pnpm 11.19.0；TypeScript 固定为 6.0.x。

## 核心分层

- `Device Manager`：管理设备生命周期、分组、批量操作与状态聚合。
- `Scenario Engine`：编排注册、保活、目录、掉线、重连与点播等场景。
- `SIP Adapter`：隔离业务代码与 `siprs` API。
- `Configuration`：加载并保存应用配置；不承担运行时状态持久化。

当前阶段只实现信令模拟。媒体/FFmpeg adapter 和独立可观测性模块作为后续扩展点，进入音视频阶段时再按实际能力新增，不保留无行为的占位模块。

运行时 SIP 核心使用共享 `SipRegistrationClient`、明确的入站 Method Dispatcher 和事务键匹配；出站交换区分原始事务响应与仅接受 2xx 的业务交换，所有新的 SIP Request（包括 Query Response MESSAGE）统一进入 `TransactionManager`。事务键由 Call-ID、CSeq 序号、CSeq Method 和 Via branch 组成；入站 UDP Non-INVITE Response 使用带 TTL 的 Server Transaction 缓存处理重复请求，INVITE 关联使用完整事务键并在终态 TTL 后清理。设备会话的 CSeq、MANSCDP SN、Digest nonce count 使用原子计数器，网络等待期间不持有设备状态锁。注册刷新、Keepalive、重试和注销由 Scheduler + 有界 transient operation executor 驱动，Runtime owner 只处理状态和 OperationCompleted 事件。

## 设备与订阅展示约束

- 应用通过“全局配置”页面只维护一份共享配置，页面按“平台配置”和“设备配置”分组。全部模拟设备共享平台地址、传输协议、平台 ID、域、Digest 认证密码、本地监听地址、对外通信地址、本地 SIP 端口、注册有效期、心跳间隔和信令字符集；配置由 Rust 核心校验并写入应用数据目录下的 JSON 文件。对外通信地址为空时，运行时根据到平台的本地路由自动探测。信令字符集支持 GB2312、GBK、UTF-8，默认 GB2312；旧 JSON 缺少字符集字段时使用该默认值。
- 设备需要维护制造商、设备型号、固件版本和通道数量；应用内只允许执行一次批量新增，批量新建设备默认未注册，不提供单设备新增或再次追加入口。设备注册与停止注册只能对当前全部设备执行，不提供单设备注册操作；注册状态属于运行时数据。仅当注册生命周期空闲时允许清空全部设备配置；清空会删除持久化设备并重置一次性批量添加标记，使设备可重新批量添加，但保留 SIP 服务配置和交互日志。若历史配置出现设备为空但批量标记为已完成，读取时按可重新批量添加处理。
- 通道从设备管理页进入展示，列表显示通道及其运行时平台订阅项；设备详情抽屉不展示服务订阅内容。设备 ID 由 `siprs-gb28181-codec` 校验为合法的 20 位国标编码。通道不持久化，仅在用户打开单台设备通道列表时由 Rust 核心按需生成，设备列表加载不得预先跨 IPC 传输全部通道。编号生成保留设备 ID 前 14 位，以设备 ID 后 3 位作为设备序号块，再追加从 `001` 开始的 3 位通道序号；结果必须保持 20 位并在全部设备间唯一。不设置独立的“服务订阅”页面。
- 交互日志作为独立的“交互日志”菜单和路由页面展示，设备管理页不嵌入日志表格。日志页面增加方向列，`send` 显示为“设备 → 服务”，`receive` 显示为“服务 → 设备”；支持按方向、设备 ID、通道 ID 和消息关键字筛选。日志列表首列提供单选和当前筛选结果全选；可按原始时间顺序将选中日志以 TSV（含表头、完整多行消息）复制到系统剪贴板。日志支持确认后清空全部运行时日志，不影响设备、SIP 配置、注册生命周期或后续日志接收。日志不分页、不展示记录数，并自动滚动至最新消息；日志至少包含时间、方向、设备 ID、通道 ID 和完整消息内容。设备与通道 ID 列居中，消息列左对齐。日志仍只保存在 Pinia/Rust 运行时内存，不写入 JSON。

- 已注册设备按共享配置周期发送 MANSCDP \`MESSAGE Keepalive\`，SN 按设备独立递增并将真实 SIP 2xx/失败结果投影到在线状态；共享 UDP 接收循环统一识别平台 SIP 方法、CmdType、Call-ID、Expires、From tag、Request-URI 和通道归属，平台请求回送结构化关联响应，Query/Response 通过设备会话构造完整的 SIP 路由、Via、From/To、Contact、Call-ID、CSeq 和平台请求 SN，对 Catalog、DeviceInfo、DeviceStatus、DeviceControl、RecordInfo 生成基础响应。Catalog Query Response 与 Catalog NOTIFY 的 DeviceList 只包含实际派生通道，设备本体只作为顶层 DeviceID 和通道 ParentID，不得作为通道条目重复上报。SUBSCRIBE 建立、刷新、取消和过期由内存订阅状态机维护，保存平台 Call-ID、From tag、设备 To tag、Event、Expires 和 NOTIFY CSeq；SUBSCRIBE 响应及 NOTIFY 的 Call-ID、CSeq、From、To、Event、Contact 等单值对话头必须唯一，NOTIFY 必须复用对应订阅对话上下文。只有 Catalog 订阅建立后自动发送首个目录 NOTIFY；Alarm 和 Mobile Position 订阅只进入有效状态，等待用户在设备管理页右侧通道卡片中手动触发。Alarm 通知使用 `<Notify>` 根节点的 AlarmPriority、AlarmMethod、AlarmTime、AlarmDescription、Longitude、Latitude 与 Info 字段，Mobile Position 使用根节点 Time、Longitude、Latitude 等字段，以兼容目标平台的字段映射。设备级订阅适用于所属全部通道，通道级订阅只适用于目标通道。设备控制和 PTZ 命令均通过有界业务 executor 发送并更新运行时状态，报文和响应进入交互日志。

## 性能约束

- 设备使用轻量状态机与 Tokio task，不为每台设备创建 OS 线程、独立 socket 或 FFmpeg 进程。
- 网络 socket 按平台或本地端口复用；使用有界队列与有界并发控制突发操作。
- 高频 SIP 消息、运行状态和日志不写入 JSON 配置；SIP 热路径只对日志使用 try-send/有界队列，队列满时增加 dropped counter 而不阻塞事务。单条日志与协议内容保持原文，不做加密、脱敏、掩码或字段改写。
- UI 应批量、降频刷新运行状态，避免每条协议消息触发界面更新。
- FFmpeg 仅在需要真实媒体的通道上启动；信令设备数与媒体并发数分别配置和统计。

## 设备注册生命周期

- 设备注册使用 GBLab 自有注册监督器和 `siprs` 消息、解析与 Digest 能力。全部设备复用一个 UDP socket，按设备独立维护 Call-ID、CSeq、From tag、nonce count 和有效期，不为每台设备创建独立 socket。
- 注册只能全量启动和全量停止。状态包括未注册、排队中、注册中、已注册、注销中和失败；只有收到平台 2xx 响应后才标记为已注册。401/407 使用设备 ID 作为用户名、共享密码完成 Digest，423 按 Min-Expires 重试；403 不自动重试。
- 首轮注册和后续失败重试均受有界并发控制；已注册设备按平台确认的有效期在 80% 时间点自动刷新。全量停止取消排队、重试和刷新，并为全部设备发送 Expires 为 0 的 REGISTER，完成后释放共享 socket 和异步任务。
- 注册生命周期运行时禁止修改 SIP 配置、编辑、删除或清空设备。应用退出时最多等待 5 秒执行全量注销，超时后结束进程。
- 注册状态、事务和完整 SIP 日志只保存在内存。轻量 `RegistrationSnapshot` 只保存聚合计数，设备运行态和订阅通过独立事件/查询获取；设备页和交互日志页直接从各自 Store 分页，不复制完整快照。日志窗口最多保存最近 10,000 条，状态和日志以 50 毫秒批次推送到 UI；REGISTER 日志不设置通道 ID。
- 当前真实注册传输只支持 UDP。配置为 TCP 时必须明确返回不支持错误，不允许静默降级为 UDP。
- GB28181 XML 出站报文的 XML 声明、Content-Type charset、实际正文编码和 Content-Length 必须一致；入站正文按 Content-Type、XML 声明、全局设备配置的顺序确定字符集。SIP 适配层统一支持 GB2312、GBK 和 UTF-8，并允许平台 XML 声明包含 standalone 属性。

## 构建与发布

- 不采用跨平台交叉编译交付。
- macOS 应用只在 macOS 构建、签名与公证。
- macOS 最低系统版本为 11.0。
- Windows 应用只在 Windows 构建、签名并生成安装包。
- Release Please 在 `main` 上维护 Release PR、Rust workspace 与桌面前端版本、`CHANGELOG.md`、`v<version>` Tag 和草稿 GitHub Release。合并 Release PR 后，CI 使用 macOS 与 Windows 原生 runner 分别构建 DMG、NSIS 和 MSI；所有安装包上传成功后才发布 Release，任一平台失败时保留草稿。
- 应用只维护一个发布版本。Release Please 以根 `package.json` 为发布组件，并通过精确 `extra-files` 同步根 Cargo workspace、`Cargo.lock` 中的两个本地 crate 以及 `src-tauri/tauri.conf.json`；根 Cargo 清单是纯 workspace，不作为独立 Rust package 发布。
- `build-macos.yml` 与 `build-windows.yml` 保留为手动构建和平台排查入口。
- 签名凭据配置完成前，自动发布产物为未签名安装包；签名凭据不得进入仓库、日志或 `.codex`。
- FFmpeg 按 macOS 与 Windows 目标平台分别打包和验证。
