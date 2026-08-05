# ZhangHao 本地功能迁移设计

## 目标

把旧本地分叉 `C:\Users\ZhangHao\Desktop\Codex-Manager` 中仍有价值的二开功能迁移到当前新版仓库，保留新版作者代码结构、现有页面样式和布局，只恢复功能链路。

迁移后的本地版应以 `CodexManagerLocal` 作为独立桌面应用存在，默认数据目录和数据库不与作者版 `CodexManager` 冲突。IP 用量展示以“客户端 IP 总消耗”为主，不按 API Key 拆分展示。管理员 Dashboard 需要能实时看到按 IP 汇总的当前请求运行和排队情况。

## 当前依据

- 旧本地迁移文档为 `docs/zhanghao-local-features-migration.md`。
- 旧本地功能提交包含：
  - `83454653`：客户端 IP 统计。
  - `22704466`：Dashboard 活跃请求监控。
  - `5a38f09f`：API Keys 内网 IP 今日 Token。
  - `d0dc09a7`：按客户端 IP 并发排队。
  - `f82a826c`、`b8a60861`：`CodexManagerLocal` 发布工作流。
- 当前新版已经有 Tauri v2、Next.js 静态导出、service/web/core 分层、基础 `request_gate` 和 gateway metrics。
- 旧 patch 不能整文件套用到新版；迁移必须按当前新版模块手工接入。
- 当前新版桌面壳通过 `apps/src-tauri/src/app_storage/env.rs` 设置 `CODEXMANAGER_DB_PATH`，默认路径来自 Tauri `app_data_dir()`。
- 当前新版服务模式通过 `crates/service/src/runtime/process_env.rs` 设置默认数据库路径，默认文件名仍是 `codexmanager.db`。

## 非目标

- 不迁移旧分叉中的样式优化、弹窗居中、长弹窗底部遮挡等视觉类提交。
- 不重做页面设计，不调整整体布局，不更换现有组件体系。
- 不把 Dashboard 活跃请求改成数据库查询；它应保持内存实时快照。
- 不在前端按 API Key 拆分 IP Token 用量。
- 不信任外部客户端伪造的 IP header。

## 方案选择

选择“分阶段功能等价迁移”：

1. 先恢复本地版命名和数据库隔离，避免开发和安装时与作者版冲突。
2. 再迁移 IP 识别、日志和 Token 统计底座。
3. 然后对外提供按客户端 IP 汇总的用量接口和页面数据。
4. 再迁移 Dashboard 实时请求快照，增加按 IP 汇总的当前请求能力。
5. 最后接回按客户端 IP 并发排队和本地发布 workflow。

不采用整 patch 合入，因为新版模块边界已经变化，整文件覆盖会丢失作者新版行为。不采用“只改前端展示”的方式，因为没有后端 IP 可信链路时，统计结果没有安全和审计价值。

## 功能设计

### 1. CodexManagerLocal 命名和数据库隔离

新增本地 Tauri 配置 `apps/src-tauri/tauri.local.conf.json`：

- `productName` 为 `CodexManagerLocal`。
- `identifier` 为 `com.codexmanager.local`。
- 主窗口标题为 `CodexManager Local`。

桌面本地版默认数据目录应由 Tauri `identifier` 派生，因此本地版数据库默认落在 `com.codexmanager.local/codexmanager.db` 下，作者版仍使用 `com.codexmanager.desktop/codexmanager.db`。RPC token、installation id 等跟随数据库目录，避免多个应用实例互相读写。

服务模式保持兼容 `CODEXMANAGER_DB_PATH`。若通过本地 release workflow 发布独立服务包，workflow 或随包说明应明确本地版需要单独设置 `CODEXMANAGER_DB_PATH`，否则服务二进制独立运行时仍会默认使用可执行文件目录下的 `codexmanager.db`。

### 2. 客户端 IP 可信识别

新增或恢复服务端 `client_ip` 工具：

- 定义内部 header，例如 `x-codexmanager-client-ip`。
- 前置 HTTP/Web gateway 从真实 peer address 写入内部 header。
- 外部传入的同名 header 必须过滤掉。
- tiny_http gateway 入口只在请求来自 loopback 的前提下信任该内部 header。
- 解析失败或缺失时，`client_ip` 为空，不伪造默认 IP。

`client_ip` 需要贯穿 local validation、upstream proxy、request log、token stats 和 active request trace。任何错误日志和 trace 不应输出 API Key 明文或 token。

### 3. 请求日志和 Token 统计

`request_logs` 和 `request_token_stats` 都需要存储 `client_ip` 字段。请求结束写日志时，同一份 trace context 同时写入请求日志和 Token 统计，保证日志页和用量统计口径一致。

Token 统计底层可以保留 `key_id + client_ip` 的原始维度，用于权限过滤、审计和未来排查。但对 RPC 和 UI 的默认展示，必须按 `client_ip` 合并：

- 一个客户端 IP 只显示一行。
- 汇总请求数、成功数、异常数、输入 Token、输出 Token、总 Token、估算费用、最近出现时间。
- 今日 Token 通过当天 `startTs/endTs` 查询后按 IP 合并。
- 管理员按全部 API Key 聚合。
- 普通成员先按其可访问 API Key 过滤，再按 IP 聚合，不暴露 key 维度。

若新版仍有 raw stats 清理或 hourly rollup 逻辑，IP 维度也需要进入 rollup，否则历史 raw 数据清理后 IP 用量会丢失。

### 4. Logs 和 API Keys 功能接入

Logs 页面恢复客户端 IP 数据：

- 日志记录类型增加 `clientIp`。
- 表格可显示客户端 IP。
- 搜索可匹配 IP。
- 保持现有表格组件和页面布局，不做视觉重构。

API Keys 页面恢复“内网 IP 用量”能力，但展示口径改为按 IP 总消耗：

- 显示客户端 IP、累计 Token、今日 Token、请求数、成功/异常、最近出现时间。
- 不按 API Key 拆分，也不展示每个 IP 使用了哪些 Key。
- 若普通成员访问，只看到自己权限范围内 Key 合并后的 IP 总量。

### 5. Dashboard 实时 IP 请求

恢复 Dashboard active requests，并把“实时 IP 请求”作为明确能力：

- gateway 请求进入后创建 activity entry。
- entry 包含 trace id、client IP、API Key 标识、路径、模型、来源、状态、创建时间、进入运行时间。
- 状态至少包含 queued 和 running。
- 请求结束后通过 RAII guard 自动移除 entry，避免残留。
- Dashboard RPC 仅管理员可读。
- 默认 limit 为 50，并限制最大值，防止快照过大。

前端 Dashboard 保持现有布局和组件风格，只补功能数据：

- 显示当前活跃请求列表。
- 同时提供按客户端 IP 汇总的实时视图数据：每个 IP 当前 running 数、queued 数、总数、最长等待时长、最长运行时长。
- 支持在现有卡片内切换全部、运行中、排队中；如果现有布局空间不足，优先保留列表和 IP 汇总的核心字段，不新增大范围页面结构。
- 该功能只在管理员可见；成员 Dashboard 不加载该轮询。

轮询间隔沿用旧实现约 1500ms，只有 service 已连接、页面可见、当前用户为管理员时启用。

### 6. 按客户端 IP 并发排队

在新版 `request_gate` 基础上恢复按客户端 IP 的并发排队：

- 同一个客户端 IP 默认最多 4 个 running 请求。
- 不同 IP 之间互相隔离。
- 缺少 `client_ip` 时跳过 IP gate，但仍走原有 key/path/model gate。
- 等待 IP gate 时 active request 标记为 queued。
- 获得 IP gate 后 active request 标记为 running。
- 等待超时返回明确的 504 错误。
- lock poisoned 或 gate 不可用返回明确的 503 错误。

并发上限先保持旧本地实现的默认值 4。若新版已有设置系统可承载该项，实施计划可以把它设计成后续可配置项，但本次迁移不扩大设置 UI。

### 7. CodexManagerLocal 发布工作流

恢复本地 release workflow 和 macOS 首启辅助：

- `.github/workflows/release-local.yml`。
- `assets/macos-local/Open CodexManagerLocal.command`。
- `assets/macos-local/README-macOS-first-launch-local.txt`。
- Windows 产物使用 `CodexManagerLocal_*_x64-setup.exe`。
- macOS 产物使用 `CodexManagerLocal_*_x64.dmg`。
- workflow 构建 Tauri 时必须传入 `--config src-tauri/tauri.local.conf.json`。

本地 workflow 不替代作者版 `release-all.yml`，两套发布配置并存。

## 权限和安全

- IP header 只能由本地前置代理或 web gateway 写入。
- 外部请求不能通过 header 冒充其他客户端 IP。
- `client_ip` 可以出现在请求日志和管理员视图中，但不能导致 API Key、token、session 等敏感字段泄露。
- Dashboard active requests 只对管理员开放。
- 普通成员的 IP 用量必须先按 API Key 权限过滤，再合并 IP。
- 数据库迁移要兼容旧库：新增字段允许为空，历史数据没有 IP 时显示为空或未知，不阻塞旧数据读取。

## 测试设计

Rust/core：

- `request_logs` 写入和读取 `client_ip`。
- `request_token_stats` 写入 `client_ip`，并能按 IP 合并统计。
- hourly rollup 保留 IP 汇总能力。
- 迁移旧数据库时新增字段和索引可正常创建。

Rust/service：

- 前置代理注入真实 peer IP，并过滤外部同名 header。
- gateway 只信任 loopback 传入的内部 IP header。
- `requestlog/client_ip_usage` 管理员返回所有 IP 汇总。
- 普通成员返回权限过滤后的 IP 汇总，不按 key 拆行。
- active requests 能记录 running/queued 并按请求结束自动清理。
- active requests RPC 拒绝非管理员。
- client IP gate 限制同 IP 并发，不影响不同 IP。

Frontend/runtime：

- normalize 能兼容 snake_case/camelCase 的 IP 用量结果。
- Logs 页面类型和表格支持 `clientIp`。
- API Keys 页面展示按 IP 汇总的累计 Token 和今日 Token。
- Dashboard active requests hook 只在管理员、service connected、页面可见时轮询。
- Dashboard 实时 IP 汇总显示 running/queued/total，不按 API Key 拆分。
- Tauri command registry 包含 requestlog 和 dashboard 新命令。
- Web command map 在需要支持 Web UI 时包含对应 RPC 映射。

Release/local：

- `tauri.local.conf.json` 存在并包含 `CodexManagerLocal`、`com.codexmanager.local`、`CodexManager Local`。
- release-local workflow 使用本地 Tauri config。
- workflow 不引用作者版 `CodexManager.app` 或作者版资产名来打本地包。

## 验证命令

按实际改动范围执行：

```powershell
cargo test -p codexmanager-core
cargo test -p codexmanager-service
cargo test -p codexmanager-web
pnpm -C apps run test:runtime
pnpm -C apps run build:desktop
```

若环境中 `pnpm` 或 `cargo` 不在 PATH，先记录精确错误，再尝试项目已有的明确运行时路径；不能把 PATH 缺失直接当成未安装。

## 完成条件

- 本地桌面版名称和安装身份为 `CodexManagerLocal` / `com.codexmanager.local`。
- 本地桌面版默认数据库目录不与作者版冲突。
- 请求日志能记录客户端 IP。
- IP Token 用量按客户端 IP 汇总展示，不按 API Key 拆分展示。
- 管理员能看到实时 IP 请求：每个 IP 当前 running、queued、total 和请求详情。
- 同 IP 并发排队生效，不同 IP 互不阻塞。
- 本地 release workflow 能构建命名正确的 Windows/macOS 产物。
- 相关 Rust、前端 runtime、desktop build 验证通过，或记录不能执行的准确原因。
