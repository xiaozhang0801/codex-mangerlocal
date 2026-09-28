# CodexManagerLocal v0.6.2 Integration Design

## Goal

以作者 `v0.6.2`（`origin/main`）为新基础，把作者最近的功能和运行时架构接入本地版，同时完整保留 ZhangHao 已经验证过的本地功能。桌面 App 的现有样式、页面布局和操作习惯保持不变，只增加必要的数据、状态和功能入口。

## Scope

- 目标产品：Tauri 桌面 App 及其依赖的本地 Rust service/core。
- 作者基线：`09ba93db`，tag `v0.6.2`。
- 当前本地基线：`5037c1e2`。
- 本地发布远程：`local`，即 `xiaozhang0801/codex-mangerlocal`。
- 作者远程只作为同步来源，不直接作为本地版发布目标。
- 不单独开发 service-mode Web UI 的本地功能；Web 代码仅在作者架构编译链要求时保持同步。

## Non-Negotiable Local Features

以下行为是本次迁移的硬性验收项，不能因为作者重构而删除、降级或改回作者默认值：

1. 产品身份必须保持为 `CodexManagerLocal`。
2. Tauri identifier 必须保持为 `com.codexmanager.local`，确保数据库、RPC token、installation id 与作者版隔离。
3. 自动更新检查、Release 下载和更新资产解析必须指向 `xiaozhang0801/codex-mangerlocal`。
4. 请求日志必须记录可信客户端 IP，外部客户端不能直接伪造该 IP。
5. IP 用量必须按客户端 IP 汇总展示，同一个 IP 使用多个 API Key 时只显示一行，不按 Key 拆分。
6. IP 用量必须保留 Token、估算金额、请求数、成功/异常数、最近请求时间，并支持排序。
7. Dashboard 必须能看到管理员当前的实时请求，包含客户端 IP、运行中/排队中状态和按 IP 汇总信息。
8. 客户端 IP 并发规则必须保持：
   - 只有一个客户端 IP 正在请求时，该 IP 不受 4 个并发上限限制；
   - 同时存在两个或更多活跃客户端 IP 时，每个客户端 IP 最多 4 个运行中请求；
   - 不同 IP 之间互不占用对方的 4 个名额；
   - 没有可信 IP 时跳过 IP gate，但继续执行作者已有的其他 gate。
9. 本地 Windows/macOS 发布 workflow 和 `CodexManagerLocal` 资产命名必须保留。
10. 本地页面的视觉样式、整体布局和已有操作流程不重做。

## Author Features To Integrate

基于 `v0.6.2` 完整保留并验证以下作者能力：

- 五小时额度恢复后的账号自动唤醒，以及账号批量开关。
- 异步 service runtime、Axum HTTP 路由、请求超时、panic 防护、健康检查和 Prometheus 指标。
- SeaORM/异步存储迁移及其数据库迁移脚本。
- Codex 模型目录刷新、模型选择和价格同步。
- Codex 直接聚合 API 能力。
- 账号、配额、远程存储、代理测试和 RPC 的 `v0.6.1`/`v0.6.2` 修复。
- 作者已有的前端生命周期和生产构建改进。

作者的配置、服务生命周期和模型目录文件不能整体覆盖本地功能文件；发生冲突时按“作者新基础能力 + 本地功能适配层”的方式处理。

## Architecture

### 1. Upstream-first integration

不在 `5037c1e2` 上直接做普通 merge。先以作者 `origin/main` 的新架构建立迁移分支，再按功能边界重新接入本地能力。这样作者的异步服务、SeaORM、HTTP middleware 和模型目录保持原结构，本地行为通过当前模块的接口接入。

### 2. Trusted client IP pipeline

客户端 IP 必须从本地 HTTP connection peer 或受信任的 loopback 前置层获得：

1. 入口 middleware/代理先删除外部传入的同名内部 header。
2. 由本地连接 peer 注入内部 IP header，或直接放入当前请求上下文。
3. service 只在受信任的 loopback 入口接受该 header。
4. IP 沿请求上下文进入 gateway、request log、token usage 和 active request。
5. 缺失或解析失败时使用空 IP，不猜测默认地址。

### 3. Storage and usage aggregation

作者的新存储边界优先作为持久化入口。新增或迁移字段、索引和聚合查询时：

- raw usage 可以保留 `key_id + client_ip` 作为权限过滤和审计维度；
- 对管理员和 App UI 的 IP 用量接口必须按 `client_ip` 聚合；
- 同一 IP 的不同 Key、不同账号和不同模型的 Token、金额、请求数必须合并；
- hourly rollup 或异步 usage snapshot 也必须保留 IP 维度；
- 旧数据库迁移必须允许历史记录缺少 IP，并且不能阻塞已有账号和请求日志读取。

对普通成员的读取顺序固定为“先按可访问 Key/账号过滤，再按 IP 合并”，任何 UI 或 RPC 响应都不返回用于展示的 Key 拆分行。

### 4. Active request snapshot

实时请求使用内存状态，不查询历史日志：

- 请求进入 gateway 时创建 RAII activity guard；
- 状态至少为 `queued` 或 `running`；
- 记录 trace、IP、路径、模型、来源和开始时间；
- gate 等待期间保持 `queued`，取得运行资格后改为 `running`；
- 请求结束或异常时由 guard 自动清理；
- Dashboard RPC 仅管理员可读；
- IP 汇总在截断请求明细之前完成，避免 limit 导致 IP 统计失真；
- App 轮询只在桌面运行时、service 已连接、页面可见且管理员登录时开启。

### 5. Conditional IP concurrency gate

IP gate 必须接入作者新的 gateway/proxy pipeline，不绕过作者已有的 deadline、取消和错误响应处理。

gate 状态维护当前活跃的 distinct IP 数和各 IP 的 running 数：

- distinct active IP 数小于等于 1：不限制该 IP 的运行数；
- distinct active IP 数大于 1：每个 IP 的运行数上限为 4；
- 当请求导致第二个 IP 出现时，后续同 IP 请求按 4 个上限排队；
- 当其他 IP 的请求结束、活跃 IP 数重新降到 1 时，原 IP 的等待请求可以继续；
- 等待超时返回明确的 504；
- gate 状态不可用返回明确的 503；
- 请求取消、超时和异常必须释放名额并唤醒等待者。

该行为需要用并发测试覆盖“单 IP 超过 4 个不阻塞”“多 IP 时每 IP 最多 4 个”“不同 IP 隔离”“活跃 IP 数变化后恢复放行”四类场景。

### 6. Local release identity

本地 Tauri 配置和发布 workflow 与作者版并存：

- `productName = CodexManagerLocal`
- `identifier = com.codexmanager.local`
- 窗口标题为 `CodexManager Local`
- Windows 产物使用 `CodexManagerLocal_*_x64-setup.exe`
- macOS 产物使用 `CodexManagerLocal_*_x64.dmg`
- 更新默认仓库固定为 `xiaozhang0801/codex-mangerlocal`

作者的 `CodexManager` 配置不能覆盖本地默认配置；作者更新版本号时，本地版本号可以同步，但产品名、identifier、更新仓库和数据库身份不能同步回作者值。

## Implementation Boundaries

- `crates/core/`: 数据结构、迁移、索引、usage/IP 汇总。
- `crates/service/`: IP 可信链路、异步 gateway 适配、active request、IP gate、RPC。
- `apps/src-tauri/`: 本地身份、命令注册、更新地址和桌面 IPC。
- `apps/src/`: 类型、API wrapper、日志/IP 用量和 Dashboard 已有页面中的功能补充。
- `.github/` 与 `assets/macos-local/`: 本地发布流程和 macOS 首启辅助。
- 不进行无关的 UI 重设计、页面重排或作者新页面的视觉重做。

## Validation

### Local feature regression

- IP header 外部伪造被忽略，loopback 注入可读取。
- request logs 和 token stats 同时保存 IP。
- 同 IP 跨多个 Key 的累计 Token 和金额只返回一行。
- 今日 Token 与累计 Token 按同一 IP 正确合并。
- 金额、Token、请求数和最近时间排序正确。
- active request guard 在成功、错误、超时和取消后均清理。
- active request RPC 拒绝非管理员。
- 单 IP无 4 并发上限，多 IP 时每 IP 上限为 4。
- `CodexManagerLocal` 和数据库 identifier 不回退。
- updater 和 release workflow 不指向作者仓库。

### Author feature regression

- 作者新增的账号自动唤醒和批量开关测试通过。
- 异步 service startup/shutdown、health、metrics、timeout 和 panic 保护测试通过。
- 模型目录、价格同步、直接聚合 API 测试通过。
- 作者新增数据库迁移和 storage tests 通过。
- App runtime tests、desktop build 和实际本地启动通过。

### Required commands

```powershell
pnpm -C apps run test:runtime
pnpm -C apps run build:desktop
cargo test --workspace -- --test-threads=1
git diff --check
```

如果完整 workspace 测试受 Windows 资源或共享测试数据库影响，必须保留失败命令和原始错误，并补充串行的最小相关测试；不能把未执行或失败的验证描述成通过。

## Completion Criteria

只有同时满足以下条件才允许提交最终合并：

1. `origin/main` 的作者功能已纳入当前代码。
2. 上述十项本地功能全部存在并通过定点回归。
3. 本地 App 身份、数据库隔离和更新地址仍指向用户仓库。
4. IP 用量不按 Key 拆分，且 Token、金额、排序可用。
5. IP 实时请求和“单 IP 放开、多 IP 每 IP 4 并发”行为可复现。
6. `CodexManagerLocal` 桌面构建和本地启动成功。
7. 所有验证结果、无法执行的命令和剩余风险都已明确记录。
