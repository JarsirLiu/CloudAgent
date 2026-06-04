# Node-Worker 第一阶段实施清单

## 文档定位

这是 `node-worker` 改造的第一阶段实施文档。

本文负责：

- 第一阶段完成标准
- 逐模块改造顺序
- 兼容策略
- 风险控制
- 第一阶段剩余收尾项

不再重复：

- 架构总览
- 长期运行约束
- 详细 Rust 骨架
- 已经落地的代码现状

这些内容统一看：

- [`docs/node-worker-rebuild-plan.zh-CN.md`](D:/learn/gifti/cloudagent/docs/node-worker-rebuild-plan.zh-CN.md)
- [`docs/node-worker-protocol-draft.zh-CN.md`](D:/learn/gifti/cloudagent/docs/node-worker-protocol-draft.zh-CN.md)
- [`docs/node-worker-current-status.zh-CN.md`](D:/learn/gifti/cloudagent/docs/node-worker-current-status.zh-CN.md)

## 目的

本文档是 [`docs/node-worker-rebuild-plan.zh-CN.md`](D:/learn/gifti/cloudagent/docs/node-worker-rebuild-plan.zh-CN.md) 的实施补充，专门回答第一阶段“先改什么、改到什么程度、哪些地方先兼容保留”。

第一阶段不追求一次完成全部多 worker pool 和 execution runner 隔离，而是先把最容易导致上下文串味的作用域模型修正过来。

第一阶段目标：

- 去掉“目录或连接天然等于 worker 身份”的假设
- 让 `node -> agentd` 链路开始显式传递 session / workspace / cwd / permission 上下文
- 让 `agentd` 具备最小可用的多 session 控制能力
- 保留旧协议兼容，避免一次性重写全部 surface

## 第一阶段完成标准

满足以下条件即可认为第一阶段完成：

- 不同目录启动多个 `cli` 时，新会话不会继承旧 `cwd`
- `node` 内部不再用 connection id 或目录字符串直接充当 worker 身份
- `agentd` 至少可以区分多个 session 的默认 workspace / cwd / permission
- 权限变更按 turn 边界生效
- 旧 `cli` 路径在兼容层下仍可运行

## 第一阶段推荐顺序

建议顺序：

1. 先改协议层数据结构
2. 再改 `apps/node` 的 session/source/runtime 结构
3. 再改 `worker_manager` 的键模型
4. 再改 `agentd` 的启动与 session registry
5. 最后补 `cli` 初始化参数

原因：

- 先把“要传什么”定下来
- 再改 node 和 worker 的内部模型
- 最后再接 surface，风险最小

## 模块清单

### 1. `crates/agent-protocol`

涉及文件：

- [`crates/agent-protocol/src/messages.rs`](D:/learn/gifti/cloudagent/crates/agent-protocol/src/messages.rs)

第一阶段要做的事：

- 为 transport initialize 增加可选的 session bootstrap 上下文字段
- 为 node status / worker status 增加新的命名字段，减少继续扩散 `worker_scope_key`
- 保留旧字段一段时间用于兼容

建议新增结构：

```rust
pub struct SessionBootstrapContext {
    pub source_domain: Option<String>,
    pub workspace_root: Option<String>,
    pub cwd: Option<String>,
    pub permission_mode: Option<String>,
}
```

建议修改：

- `TransportInitializeParams`
  增加 `session_context: Option<SessionBootstrapContext>`
- `NodeWorkerStatus`
  新增 `worker_domain: Option<String>`
- `NodeStatusResponse`
  后续可扩展 `worker_instances`

第一阶段兼容策略：

- `worker_scope_key` 不立刻删除
- 新字段先可选
- 旧客户端未传 `session_context` 时，node 走 fallback 推导

### 2. `crates/agent-app-server-client`

涉及文件：

- [`crates/agent-app-server-client/src/remote.rs`](D:/learn/gifti/cloudagent/crates/agent-app-server-client/src/remote.rs)

第一阶段要做的事：

- `RemoteClientConfig` 支持带入 session bootstrap 上下文
- `initialize_params()` 在握手时把当前 `workspace_root` / `cwd` / permission 传给 node

建议改动：

- `AppServerConnectInfo` 或 `RemoteClientConfig` 增加：
  - `source_domain`
  - `workspace_root`
  - `cwd`
  - `permission_mode`

要求：

- 初始化时显式发送
- 不再让 node 只能通过 client name 猜本次会话在哪个目录启动

### 3. `cli`

涉及文件：

- [`cli/src/console_entry.rs`](D:/learn/gifti/cloudagent/cli/src/console_entry.rs)
- [`cli/src/local_node.rs`](D:/learn/gifti/cloudagent/cli/src/local_node.rs)
- [`cli/src/transport/client.rs`](D:/learn/gifti/cloudagent/cli/src/transport/client.rs)
- [`cli/src/main.rs`](D:/learn/gifti/cloudagent/cli/src/main.rs)

第一阶段要做的事：

- 在建立 remote client 时显式带上当前目录启动得到的 session context
- 明确区分：
  - node 的驻留地址
  - CLI 会话的启动目录
  - conversation 的执行目录

重点修改：

- `build_local_node_bootstrap()`
  继续负责连 node，但不再暗示“这个目录决定 worker 身份”
- `default_node_addr()`
  第一阶段建议保留现状
- `workspace_scoped_node_port()`
  第一阶段可以先不删，但要在文档和代码注释里降级为“开发期本地 node 发现策略”，不是 worker scope 策略

注意：

- 第一阶段先解决“会话上下文显式传递”
- 不强行同时改本地 node 发现模型

### 4. `apps/node/src/node/source.rs`

涉及文件：

- [`apps/node/src/node/source.rs`](D:/learn/gifti/cloudagent/apps/node/src/node/source.rs)

当前问题：

- `NodeSource` 里 `domain_id` 和 `worker_scope_key` 仍然绑定
- `placeholder()` 直接把 connection 风格字符串变成 worker identity

第一阶段要做的事：

- 把 `NodeSource` 改成只表达 source 信息，不直接持有 worker scope

建议结构：

```rust
pub struct NodeSource {
    pub domain_id: String,
    pub client_name: String,
}
```

建议新增：

```rust
pub struct SessionContextSeed {
    pub workspace_root: Option<PathBuf>,
    pub cwd: Option<PathBuf>,
    pub permission_mode: Option<String>,
}
```

第一阶段要求：

- `source.rs` 只负责来源归类
- worker instance 选择移到 `worker_manager` 或新的 binding registry

### 5. `apps/node/src/node/session_state.rs`

涉及文件：

- [`apps/node/src/node/session_state.rs`](D:/learn/gifti/cloudagent/apps/node/src/node/session_state.rs)

当前问题：

- `NodeSessionState::new(..., worker_scope_key)` 把 session 和 worker 绑定死了

第一阶段要做的事：

- 把 `NodeSessionState` 改成真正的 session state
- 内部保存 session 上下文和当前绑定的 worker instance id

建议新增字段：

- `session_id`
- `source: NodeSource`
- `workspace_root`
- `cwd`
- `permission_mode`
- `bound_worker_instance`

建议删除的隐式语义：

- `worker_scope_key()` 作为 session 原生属性

第一阶段兼容方案：

- 可以保留 `worker_scope_key()` 方法
- 但实现改成从 `bound_worker_instance` 映射读取

### 6. `apps/node/src/node/server.rs`

涉及文件：

- [`apps/node/src/node/server.rs`](D:/learn/gifti/cloudagent/apps/node/src/node/server.rs)

当前问题：

- node 进程启动时读取 `current_dir()`，并把它作为 node workspace root 的重要输入
- 新连接的 session 初始状态仍偏“连接占位”

第一阶段要做的事：

- 明确区分 node 自身 workspace 与 client session workspace
- 在 transport initialize 时读取 `session_context`
- 用它初始化 `NodeSessionState`

重点改动：

- `run_connection()`
  创建 session 时不再只传 placeholder scope
- `handle_handshake_message()`
  初始化完成后写入：
  - source domain
  - workspace_root
  - cwd
  - permission_mode
- `load_node_skill_runtime()`
  第一阶段先保留 node 级 workspace root

注意：

- skill catalog 仍可能依赖 node 级 workspace root
- 这是后续阶段再进一步拆的点

### 7. `apps/node/src/node/runtime.rs`

涉及文件：

- [`apps/node/src/node/runtime.rs`](D:/learn/gifti/cloudagent/apps/node/src/node/runtime.rs)

第一阶段要做的事：

- 增加 session registry / workspace registry 的入口
- 不要求第一阶段完成全量持久化

建议新增：

- `SessionRegistry`
- `WorkspaceRegistry`
- `WorkerBindingRegistry`

第一阶段最小实现：

- 内存态 registry 即可
- 保证：
  - `session_id -> context`
  - `session_id -> worker_instance`
  - `workspace_id -> root_path`

### 8. `apps/node/src/node/worker_manager.rs`

涉及文件：

- [`apps/node/src/node/worker_manager.rs`](D:/learn/gifti/cloudagent/apps/node/src/node/worker_manager.rs)

这是 node 侧第一阶段主改点。

当前问题：

- 公开接口全部按 `worker_scope_key` 驱动
- `ensure_worker()` 的 key 本质就是 scope identity

第一阶段要做的事：

- 从“按 worker scope key 找 worker”改成“按 worker instance id 找 worker”
- 增加“按 source domain 分配 worker instance”的逻辑

建议新增概念：

```rust
pub struct WorkerInstanceId(pub String);
pub struct WorkerDomainId(pub String);
```

建议新增接口：

- `bind_session(session_id, source_domain, session_context) -> WorkerInstanceId`
- `worker_for_session(session_id) -> WorkerInstanceId`
- `ensure_worker_instance(worker_instance_id, source_domain)`

第一阶段策略：

- 每个 source domain 先只保留一个 worker instance
- 也就是：
  - `local:cli -> cli#0`
  - `local:web -> web#0`
  - `im:feishu -> feishu#0`

这样第一阶段就能满足你的目标方向，同时不把 pool 调度复杂度一次拉满。

### 9. `apps/node/src/node/command_router.rs`

涉及文件：

- [`apps/node/src/node/command_router.rs`](D:/learn/gifti/cloudagent/apps/node/src/node/command_router.rs)

当前问题：

- 业务路由时仍通过 `session.worker_scope_key()` 调 worker

第一阶段要做的事：

- 改成通过 session binding 找 worker instance
- 在发给 worker 的命令或 request 中补 execution context

建议：

- 所有需要 worker 的命令都在 node 侧先补齐：
  - `session_id`
  - `workspace_id`
  - `cwd`
  - permission profile

第一阶段可接受方案：

- 不必一次改完所有 command type
- 先覆盖：
  - submit turn
  - request conversation status/history
  - interrupt
  - compact/reset

### 10. `apps/agentd`

涉及文件：

- [`apps/agentd/src/main.rs`](D:/learn/gifti/cloudagent/apps/agentd/src/main.rs)

当前问题：

- 启动时直接 `current_dir()` + `AgentConfig::load(workspace_root)`
- 天然带单 workspace / 单会话心智

第一阶段要做的事：

- 把 `agentd` 从“进程启动即绑定 workspace”改成“进程启动后等待 session/context 注入”
- 增加最小 session registry

建议第一阶段最小模型：

- `agentd` 进程级只加载通用 runtime
- 每个 session 首次请求到来时建立：
  - session state
  - workspace context
  - permission state

第一阶段不要求：

- 一次做完 execution subprocess 拆分
- 一次做完真正的 worker pool

### 11. `agent-app-server`

涉及文件：

- `crates/agent-app-server` 下 turn / routing 相关模块

第一阶段要做的事：

- 允许 command envelope 或 typed request 带 execution context
- turn service 优先使用显式上下文

目标：

- 不再依赖 `agentd` 进程级默认目录来推断当前会话在哪个 workspace

### 12. `agent-core`

涉及文件：

- `crates/agent-core/src/context/*`
- `crates/agent-core/src/turn/*`

第一阶段要做的事：

- 核查所有通过 host/context 获取 `workspace_root`、`cwd`、permission 的入口
- 确保它们来自 session/turn context，而不是进程初始化状态

第一阶段重点不是大改逻辑，而是补“显式输入优先级”。

## 第一阶段建议新增的数据结构

建议优先落在协议层和 node 层：

```rust
pub struct SessionContextSeed {
    pub workspace_root: Option<PathBuf>,
    pub cwd: Option<PathBuf>,
    pub permission_mode: Option<String>,
}

pub struct SessionBinding {
    pub session_id: String,
    pub source_domain: String,
    pub worker_instance_id: String,
    pub workspace_id: String,
}

pub struct ExecutionContextSnapshot {
    pub session_id: String,
    pub conversation_id: String,
    pub workspace_id: String,
    pub workspace_root: PathBuf,
    pub cwd: PathBuf,
    pub permission_mode: String,
}
```

## 兼容策略

第一阶段建议保留这些旧语义一段时间：

- `worker_scope_key`
- node status 中 `worker_running`
- 基于 client name 的 source 推断

但要降级为：

- 兼容字段
- fallback 路径
- 调试输出

不要再作为主逻辑的唯一依据。

## 第一阶段不做的事

避免范围失控，以下内容建议明确延后：

- 真正的多实例 worker pool 调度
- worker 热迁移
- 权限变化触发隔离 worker 重绑定
- 完整 execution subprocess 沙箱体系
- hub 模式的跨节点调度

## 推荐提交拆分

为了方便审查，建议按下面顺序拆 PR 或提交：

1. 协议与文档层
2. node session/source/runtime 重构
3. worker_manager 键模型替换
4. agentd session registry 最小实现
5. cli 初始化上下文透传

## 审查重点

你可以重点盯这几个问题：

- 有没有任何地方继续把 `cwd` 当 worker 身份
- 有没有任何地方继续把 connection id 当 session 的真实上下文
- `agentd` 是否仍在进程启动时绑定固定 workspace
- permission 是否已经改成 turn 边界生效
- 新结构是否能支撑“一个 cli worker 承载多个目录、多个会话”

## 结论

第一阶段不是把整个 node-worker 架构一次做完，而是先把“作用域模型”和“上下文传递模型”纠正过来。

只要这一阶段做对，后续再扩 worker pool、execution runner、hub mode，都会顺很多。
