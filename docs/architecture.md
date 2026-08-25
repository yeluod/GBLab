# 架构

GBLab 采用单仓库、三层运行结构：

```text
Vue 3 + Naive UI
        ↓ 类型化 Tauri IPC
Tauri 2 桌面壳
        ↓
gblab-core
├── 领域与应用编排
├── Tokio 生命周期和资源控制
├── siprs 协议适配
├── SQLite 持久化
├── FFmpeg 媒体适配
└── 可观测性
```

`gblab-core` 不依赖 Tauri。桌面壳只负责应用目录初始化、IPC 参数与结果映射以及核心生命周期装配。前端只维护展示和交互状态，不持有设备权威运行状态。

详细目录职责与依赖约束见 `.codex/knowledge/project-structure.md`。
