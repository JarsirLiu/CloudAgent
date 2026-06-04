# Node-Worker 协议与 Rust 骨架草案

## 文档定位

这是 `node-worker` 改造的协议与代码骨架文档。

本文负责：

- transport 与 command 协议扩展
- node 到 worker 的内部包络模型
- Rust 侧核心类型与 trait 草案

本文不再单独拆出“模块架构约束”或“骨架碎文档”，相关内容已经并入本文和主文档。

如果你想先确认“目前代码已经做到哪里”，请先看：

- [`docs/node-worker-current-status.zh-CN.md`](D:/learn/gifti/cloudagent/docs/node-worker-current-status.zh-CN.md)

## 目标

这份草案定义 node-worker 改造第一阶段需要稳定下来的协议边界。

重点解决三个问题：

- `cli/web/im` 接入 node 时，如何显式声明本次会话的来源和启动上下文
- node 路由请求到 `agentd` 时，如何显式携带执行上下文
- 哪些信息属于“连接握手级别”，哪些信息属于“每次 turn / command 级别”

本草案优先服务第一阶段落地，不追求一步做完 hub 和多实例 pool 的全部需求。

## 设计原则

协议层采用两级上下文：

- `session bootstrap context`
  在 transport initialize 阶段传一次，描述该连接的默认会话上下文
- `execution context snapshot`
  在需要 worker 执行的命令上显式传递，描述该次执行的真实上下文

规则：

- `source domain` 是连接级信息
- `workspace_root` / `cwd` / permission 默认从 session bootstrap 继承
- 实际执行以 command 上携带的 execution context 为准
- 不允许 worker 仅靠进程 `current_dir()` 猜会话上下文

## 第一阶段协议范围

第一阶段只要求覆盖：

- transport initialize
- submit turn
- conversation status/history typed read
- interrupt / compact / reset
- subscribe / unsubscribe conversation

其余命令先可以继续走旧路径，但要预留扩展位。

## 一、Transport 握手层

### 现状

当前 [`TransportInitializeParams`](D:/learn/gifti/cloudagent/crates/agent-protocol/src/messages.rs) 只有：

```rust
pub struct TransportInitializeParams {
    pub client_info: TransportClientInfo,
    pub capabilities: Option<TransportInitializeCapabilities>,
}
```

这导致 node 只能从：

- client name
- 连接来源
- 本地进程状态

去猜这个 session 属于哪个目录、默认权限是什么。

### 第一阶段新增结构

建议新增：

```rust
pub struct SessionBootstrapContext {
    pub session_id: Option<String>,
    pub source_domain: Option<String>,
    pub workspace_root: Option<String>,
    pub cwd: Option<String>,
    pub permission_mode: Option<String>,
    pub data_root_dir: Option<String>,
    pub metadata: Option<std::collections::BTreeMap<String, String>>,
}
```

然后扩展：

```rust
pub struct TransportInitializeParams {
    pub client_info: TransportClientInfo,
    pub capabilities: Option<TransportInitializeCapabilities>,
    pub session_context: Option<SessionBootstrapContext>,
}
```

### 字段说明

`session_id`

- 可选
- 允许 surface 自带一个稳定 session id
- 未提供时由 node 分配

`source_domain`

- 可选但强烈建议传
- 示例：
  - `local:cli`
  - `local:web`
  - `im:feishu`
  - `im:wecom`

`workspace_root`

- 默认工作区根目录
- 这是逻辑 workspace 身份的输入，不是 worker 身份

`cwd`

- 当前 session 的默认当前目录
- 如果缺失，则默认回退到 `workspace_root`

`permission_mode`

- 会话默认权限模式
- 推荐直接传 UI 层看到的 canonical mode，例如：
  - `ReadOnly`
  - `WorkspaceWrite`
  - `FullAccess`

`data_root_dir`

- 第一阶段可选
- 主要用于 node 校验连接上下文是否与预期持久化根一致

`metadata`

- 扩展位
- 预留给后续 `hub/web/im` 的来源标记、tab id、device id 等

### 第一阶段 node 行为

node 收到握手后：

1. 解析 `source_domain`
2. 建立或确认 `session_id`
3. 建立 session registry 条目
4. 创建默认 workspace 记录
5. 为该 session 绑定默认 worker instance

握手后生成的 session 上下文只代表默认值，不代表后续每次执行一定不变。

## 二、命令包络层

### 现状

当前 [`AppClientCommandEnvelope`](D:/learn/gifti/cloudagent/crates/agent-protocol/src/wire.rs) 只有：

```rust
pub struct AppClientCommandEnvelope {
    pub request_id: RequestId,
    pub command: AppClientCommand,
}
```

问题是：

- command 只有 conversation id
- node 很难分清本次命令应该用哪个 session / workspace / cwd
- worker 更看不到显式 execution context

### 第一阶段新增结构

建议新增：

```rust
pub struct CommandExecutionContext {
    pub session_id: Option<String>,
    pub workspace_id: Option<String>,
    pub workspace_root: Option<String>,
    pub cwd: Option<String>,
    pub permission_mode: Option<String>,
}
```

并扩展 envelope：

```rust
pub struct AppClientCommandEnvelope {
    pub request_id: RequestId,
    pub command: AppClientCommand,
    pub context: Option<CommandExecutionContext>,
}
```

### 为什么不只放在 command 里

不建议把这些字段直接塞进每个 `AppClientCommand` 变体里，因为：

- 大量命令本质共享相同上下文
- 会造成 enum 每个分支都膨胀
- wire 层统一扩展更便于兼容

所以第一阶段推荐：

- command 继续描述“做什么”
- envelope context 描述“在哪个上下文里做”

## 三、Node 到 Worker 的内部协议

### 目标

node 发给 `agentd` 时，不能再只靠：

- `conversation_id`
- `worker_scope_key`

而要显式给出执行上下文。

### 建议新增结构

建议在 node 内部或协议层新增：

```rust
pub struct WorkerExecutionContext {
    pub session_id: String,
    pub source_domain: String,
    pub worker_instance_id: String,
    pub workspace_id: String,
    pub workspace_root: String,
    pub cwd: String,
    pub permission_mode: String,
}
```

以及内部 worker 请求包络：

```rust
pub struct WorkerCommandEnvelope {
    pub conversation_id: String,
    pub context: WorkerExecutionContext,
    pub command: AppClientCommand,
}
```

typed request 也同理：

```rust
pub struct WorkerJsonRpcRequest {
    pub conversation_id: String,
    pub context: WorkerExecutionContext,
    pub request: JsonRpcRequest,
}
```

### 第一阶段实现建议

第一阶段不一定要把这些结构全部提升到公共 wire 协议里。

可以先：

- `surface -> node`
  用正式公共协议
- `node -> worker`
  先作为 node 内部调用结构

这样能更快收敛实现。

## 四、字段优先级

需要明确一套统一优先级，否则后面会再次串味。

第一阶段推荐优先级：

1. command envelope 中的 `context`
2. transport initialize 中的 `session_context`
3. node 为该 session 持久化的默认值
4. 最后的保底 fallback

具体规则：

- `cwd`
  `command.context.cwd` > `session_context.cwd` > `workspace_root`
- `permission_mode`
  `command.context.permission_mode` > `session_context.permission_mode` > node session default
- `workspace_root`
  `command.context.workspace_root` > `session_context.workspace_root`

禁止的旧行为：

- 直接拿 `agentd` 进程启动时 `current_dir()` 作为当前 turn 的 workspace
- 直接拿 node 启动时 `current_dir()` 作为所有 session 的 workspace

## 五、兼容策略

第一阶段必须兼容旧客户端。

兼容规则：

- 如果 `TransportInitializeParams.session_context` 缺失：
  node 继续从 client name 和连接上下文做 fallback
- 如果 `AppClientCommandEnvelope.context` 缺失：
  node 用 session registry 里的默认值补齐
- `worker_scope_key` 暂时保留
  但只作为兼容字段和调试字段

### 建议的弃用节奏

第一阶段：

- 新字段可选
- 旧字段保留

第二阶段：

- surface 默认发送新字段
- node 主逻辑切换到新字段

第三阶段：

- `worker_scope_key` 退化为 status/debug 字段或彻底删除

## 六、Node Status 扩展建议

当前 [`NodeWorkerStatus`](D:/learn/gifti/cloudagent/crates/agent-protocol/src/messages.rs) 里只有 `worker_scope_key`。

第一阶段建议扩成：

```rust
pub struct NodeWorkerStatus {
    pub worker_scope_key: String,
    pub worker_domain: Option<String>,
    pub worker_instance_id: Option<String>,
    pub bound_session_count: Option<usize>,
    pub health: NodeWorkerHealth,
    pub detail: Option<String>,
    pub idle_for_ms: Option<u64>,
    pub last_failure_at_ms: Option<u64>,
}
```

这样调试时你能看见：

- 这是 `cli` 还是 `im:feishu`
- 它是不是一个实例池里的 `#0`
- 当前绑了多少 session

这对排查“多个目录多个会话到底谁在管”很重要。

## 七、第一阶段建议覆盖的命令

优先覆盖这些命令的 context 透传：

- `SubmitTurn`
- `InterruptTurn`
- `CompactConversation`
- `ResetConversation`
- `RequestConversationStatus`
- `RequestConversationHistory`
- `RequestConversationHistoryPage`
- `SubscribeConversation`
- `UnsubscribeConversation`

原因：

- 这些命令最直接依赖 conversation runtime
- 也是最容易暴露旧 workspace 串味的地方

## 八、Rust 骨架草案

### 8.1 Node 侧核心模型

```rust
pub struct SourceDomainId(String);
pub struct SessionId(String);
pub struct WorkspaceId(String);
pub struct WorkerDomainId(String);
pub struct WorkerInstanceId(String);
```

```rust
pub struct NodeSource {
    pub domain_id: SourceDomainId,
    pub client_name: String,
    pub client_version: String,
}
```

```rust
pub struct SessionDefaults {
    pub workspace_id: WorkspaceId,
    pub workspace_root: std::path::PathBuf,
    pub cwd: std::path::PathBuf,
    pub permission_mode: String,
}

pub struct SessionState {
    pub session_id: SessionId,
    pub source: NodeSource,
    pub defaults: SessionDefaults,
    pub active_conversation_id: String,
    pub subscribed_conversations: std::collections::HashSet<String>,
}
```

```rust
pub struct WorkspaceRecord {
    pub workspace_id: WorkspaceId,
    pub root_path: std::path::PathBuf,
    pub repo_fingerprint: Option<String>,
    pub data_root_dir: Option<std::path::PathBuf>,
}
```

```rust
pub struct ExecutionContextSnapshot {
    pub session_id: SessionId,
    pub conversation_id: String,
    pub workspace_id: WorkspaceId,
    pub workspace_root: std::path::PathBuf,
    pub cwd: std::path::PathBuf,
    pub permission_mode: String,
}
```

### 8.2 Node 侧核心接口

```rust
#[async_trait::async_trait]
pub trait SessionRegistry: Send + Sync {
    async fn create(&self, state: SessionState) -> anyhow::Result<()>;
    async fn get(&self, session_id: &SessionId) -> Option<SessionState>;
    async fn update(&self, state: SessionState) -> anyhow::Result<()>;
    async fn remove(&self, session_id: &SessionId) -> anyhow::Result<()>;
}
```

```rust
#[async_trait::async_trait]
pub trait WorkspaceRegistry: Send + Sync {
    async fn get(&self, workspace_id: &WorkspaceId) -> Option<WorkspaceRecord>;
    async fn find_by_root(&self, root: &std::path::Path) -> Option<WorkspaceRecord>;
    async fn upsert(&self, record: WorkspaceRecord) -> anyhow::Result<WorkspaceId>;
}
```

```rust
#[async_trait::async_trait]
pub trait WorkerBindingRegistry: Send + Sync {
    async fn bind(&self, session_id: SessionId, worker_id: WorkerInstanceId)
        -> anyhow::Result<()>;
    async fn get(&self, session_id: &SessionId) -> Option<WorkerInstanceId>;
    async fn unbind(&self, session_id: &SessionId) -> anyhow::Result<()>;
}
```

```rust
pub trait ExecutionContextResolver: Send + Sync {
    fn resolve(
        &self,
        session: &SessionState,
        command_context: Option<&CommandExecutionContext>,
    ) -> anyhow::Result<ExecutionContextSnapshot>;
}
```

### 8.3 Agentd 侧核心模型

```rust
pub struct WorkerSessionState {
    pub session_id: String,
    pub source_domain: String,
    pub default_workspace_id: String,
    pub default_workspace_root: std::path::PathBuf,
    pub default_cwd: std::path::PathBuf,
    pub permission_mode: String,
}
```

```rust
pub struct AgentdExecutionContext {
    pub session_id: String,
    pub conversation_id: String,
    pub workspace_id: String,
    pub workspace_root: std::path::PathBuf,
    pub cwd: std::path::PathBuf,
    pub permission_mode: String,
}
```

```rust
pub struct ConversationRuntime {
    pub conversation_id: String,
    pub session_id: String,
    pub workspace_id: String,
    pub agent_host: std::sync::Arc<agent_core::AgentHost>,
}
```

### 8.4 Agentd 侧核心接口

```rust
#[async_trait::async_trait]
pub trait WorkerSessionRegistry: Send + Sync {
    async fn get(&self, session_id: &str) -> Option<WorkerSessionState>;
    async fn upsert(&self, state: WorkerSessionState) -> anyhow::Result<()>;
}
```

```rust
#[async_trait::async_trait]
pub trait ConversationRuntimeRegistry: Send + Sync {
    async fn get(&self, conversation_id: &str) -> Option<std::sync::Arc<ConversationRuntime>>;
    async fn upsert(
        &self,
        runtime: std::sync::Arc<ConversationRuntime>,
    ) -> anyhow::Result<()>;
}
```

```rust
#[async_trait::async_trait]
pub trait TurnQueue: Send + Sync {
    async fn enqueue(&self, conversation_id: &str, task: TurnTask) -> anyhow::Result<()>;
    async fn interrupt(&self, conversation_id: &str)
        -> anyhow::Result<InterruptDisposition>;
}
```

```rust
pub struct TurnTask {
    pub context: AgentdExecutionContext,
    pub command: agent_protocol::AppClientCommand,
}
```

### 8.5 代码组织约束

实现这套协议和骨架时，要求：

- `node` 按 `transport / routing / session / workspace / source / worker / runtime` 拆
- `agentd` 按 `controller / session / workspace / execution / conversation / transport` 拆
- `main.rs` 只做启动组装
- `worker_manager.rs`、`server.rs`、`command_router.rs` 不允许继续叠成全能大文件

## 九、审查重点

你审这份草案时，可以重点看这几个问题：

- `session bootstrap context` 和 `execution context snapshot` 的边界是否清楚
- 有没有把目录继续偷偷塞回 worker identity
- `context` 是不是应该放在 envelope，而不是每个 command 分支里
- 兼容策略是否足够平滑
- 第一阶段字段是否已经足够支撑 `cli` 多目录、多会话

## 结论

第一阶段协议层最关键的两个改动就是：

1. 给 `TransportInitializeParams` 增加 `session_context`
2. 给 `AppClientCommandEnvelope` 增加 `context`

只要这两个入口立住，node 和 `agentd` 就可以从“猜上下文”转向“消费显式上下文”，后续的 worker pool 和 hub 模式才有稳定基础。
