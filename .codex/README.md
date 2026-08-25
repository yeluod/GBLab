# GBLab 项目知识库

`.codex` 保存 GBLab 当前有效且具有长期价值的项目知识，用于辅助实现、排查与发布决策；它不替代代码、配置或用户当前指令。知识内容只记录已经确认的结论，不记录过程、临时方案或敏感信息。

## Knowledge 索引

| Knowledge | File | Code Path | Scope |
|---|---|---|---|
| architecture | knowledge/architecture.md | 全局 | 产品定位、技术栈、核心分层与构建策略 |
| development-standards | knowledge/development-standards.md | 全局 | 前后端编码、测试、质量门禁与交付规范 |
| project-structure | knowledge/project-structure.md | 全局 | 项目骨架、目录职责与依赖边界 |

## Knowledge 依赖关系

| Knowledge | Depends On |
|---|---|
| architecture | — |
| development-standards | architecture |
| project-structure | architecture, development-standards |

## Workspace 规则

- 开始项目任务时先阅读本文件，再按索引读取与任务直接相关的 Knowledge。
- 代码、配置与用户当前明确指令优先于 Knowledge。
- 仅在本次工作确认了会持续影响实现、测试、部署或团队协作的结论时更新 Knowledge。
- 优先更新已有主题；新增主题时同步维护本索引与依赖关系。
- 不保存密钥、令牌、密码、证书或私钥。
