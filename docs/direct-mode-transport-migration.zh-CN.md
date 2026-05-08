# CloudAgent 直连模式传输层切换与分阶段迁移方案

## 目的

本文档用于把当前 CloudAgent 的本地 CLI / app-server / daemon 结构，逐步迁移到更接近 Codex 的分层形态，并优先完成 `Direct Mode` 开发所需的传输层切换。

本文档关注的不是 Hub 本身，而是为下一步 Hub 接入打好边界。

本文档完成后，系统应先具备以下能力：

- CLI 不再直接耦合 `agentd` 的本地运行方式
- CLI 可以通过统一 client 抽象连接不同目标
- 本地存在一个轻量常驻 `node`
- `node` 可以按需拉起 `worker`
- `Direct Mode` 可以基于 `node -> worker` 结构继续开发
- `Hub Mode` 可以在后续作为新的 target 和 transport 接入，而不重写 CLI 主逻辑
- `Hub Mode` 下 CLI 必须能够查看在线设备并选择目标节点

## 范围边界

本文档的实施范围分为两层：

- 本轮必须完成：`Direct Mode`
- 本轮必须预留：`Hub Mode` 所需的 CLI / target / 路由边界

本轮不做的事情：

- Hub 服务本体实现
- 节点注册与心跳完整闭环
- 远程文件中转
- 多节点 attach 的最终产品化

本轮必须禁止的做法：

- 长期保留多套并行主路径
- 用“兼容模式”替代真正的切换
- 用“回退到旧实现”掩盖新架构未完成
- 在 CLI、node、worker 三层同时保留两套常驻主语义

允许存在的仅有一种过渡：

- 某些旧入口可在极短迁移阶段存在
- 但必须有明确删除阶段和删除提交

## 当前现状

当前仓库已经具备以下基础：

- `agent-core` 已经统一了 `conversation / turn / item` 的核心模型
- `agent-protocol` 已经定义了 `AppClientCommand` 和 `AppServerMessage`
- `agent-app-server` 已经具备 worker 对外会话协议边界
- `agent-app-server-client` 已经具备 `InProcess` 和 `Stdio` 两种接法
- `cli` 已经以事件驱动方式消费 app-server 返回的数据

当前关键文件：

- CLI 连接配置：[cli/src/app/core/types.rs](/D:/learn/gifti/cloudagent/cli/src/app/core/types.rs:1)
- CLI 连接创建：[cli/src/transport/client.rs](/D:/learn/gifti/cloudagent/cli/src/transport/client.rs:1)
- CLI 启动参数：[cli/src/main.rs](/D:/learn/gifti/cloudagent/cli/src/main.rs:1)
- worker 宿主入口：[apps/agentd/src/main.rs](/D:/learn/gifti/cloudagent/apps/agentd/src/main.rs:1)
- app-server 边界：[crates/agent-app-server/src/lib.rs](/D:/learn/gifti/cloudagent/crates/agent-app-server/src/lib.rs:1)
- gateway 占位入口：[apps/gatewayd/src/main.rs](/D:/learn/gifti/cloudagent/apps/gatewayd/src/main.rs:1)

## 与 Codex 的主要差距

当前实现与 Codex 风格最主要的差距不在 UI，而在 client 和 target 分层不完整。

Codex 风格具备以下特征：

- TUI 只依赖统一的 `AppServerClient`
- `AppServerClient` 同时支持 embedded 和 remote
- 目标选择与传输实现分离
- 远程连接是一等实现，不是本地 stdio 的变体
- app-server client 负责初始化、事件语义、回压、断连、server request 回传

当前 CloudAgent 还缺：

- 一等的 `LocalNode` client
- 类似 `AppServerTarget` 的目标抽象
- 常驻 `node`
- `node -> worker` 生命周期管理
- 统一的本地 node 监听入口

但与 Codex 的一个重要策略差异是：

- Codex 会保留 embedded / remote 共存能力
- CloudAgent 在本轮直连模式切换后，不应长期保留多条并行主路径

本项目的目标不是“支持尽可能多的旧入口”，而是“尽快把主路径切换为 `cli -> node -> worker`”。

## 目标架构

第一阶段目标架构如下：

- CLI
- Local Node
- Worker

调用链：

- `cli -> local node -> worker`

其中：

- CLI 负责展示、输入、会话交互
- Local Node 负责常驻监听、attach / spawn / recycle
- Worker 负责真正执行 agent turn

第二阶段再继续：

- `IM adapter -> local node -> worker`

第三阶段再继续：

- `cli/web/im -> hub -> node -> worker`

第四阶段可自然扩展为：

- `web/app -> hub -> target node -> worker`

## 迁移原则

迁移过程中必须保持以下原则：

1. 不重写 CLI 主交互逻辑
2. 不破坏现有 `Conversation / Turn / Item` 模型
3. 不让 Hub 设计反向污染 Direct Mode 的最小闭环
4. `node` 只承担控制面和路由面，不承担重执行
5. `worker` 仍然复用 `agent-app-server` 和 `agent-core`
6. transport 切换过程中尽量保持 `AppClientCommand` 和 `AppServerMessage` 不变
7. 先补 target 和 client facade，再补 node 进程
8. 迁移完成后，CLI 主路径只能保留 `local-node`
9. 旧路径只能作为短期施工脚手架，不能成为长期降级方案

## 新的概念映射

### 术语约定

为了避免与操作系统线程或语言线程模型混淆，CloudAgent 在本文档中统一使用：

- `conversation`
- `session`
- `turn`
- `item`

其中：

- `conversation` 表示长期会话
- `session` 表示某个客户端或入口对 conversation 的一次连接、附着或交互上下文
- `turn` 表示一次输入到输出完成的执行轮次
- `item` 表示 turn 内部的消息、工具调用、审批请求、工具结果等事件

如果提到 Codex 中的 `thread`，在 CloudAgent 的语境里应映射为 `conversation`。

需要特别避免的混淆：

- `conversation` 不是 `session`
- 一个 `conversation` 可以同时被多个 `session` attach
- `session` 可以断开和重连，但 `conversation` 仍然存在

建议引入三个层次。

### 1. Target

Target 是 CLI 想要连接的目标，而不是底层传输实现。

第一阶段建议形态：

```rust
pub enum AppServerTarget {
    LocalNode,
    HubNode {
        node_id: String,
    },
}
```

含义：

- `LocalNode`：CLI 连接本地常驻 node，由 node 决定 attach 或 spawn worker
- `HubNode`：CLI 通过 Hub 连接某个远端节点上的 conversation

说明：

- `Embedded`
- `WorkerStdio`

不应成为长期 target。

它们若在迁移中短暂存在，只能作为内部施工态，不应作为最终公开主入口。

### 2. Client

Client 是 CLI 上层真正依赖的统一接口。

建议形态：

```rust
pub enum AppServerClient {
    LocalNode(...),
    Hub(...),
}
```

本轮必须完成：

- `LocalNode`

本轮必须预留但不实现完整能力：

- `Hub`

说明：

- `InProcess`
- `Stdio`

若在迁移施工期继续存在，也应下沉为 node 或测试内部使用的实现细节，而不是 CLI 长期对外模型。

### 3. Node

Node 是常驻目标，不是 CLI transport 的别名。

职责：

- 监听本地 CLI 或 IM adapter 请求
- 维护 `conversation_id -> worker handle` 映射
- 在需要时拉起 worker
- 将 worker 事件流转发给连接方
- 管理 worker 空闲回收

## 推荐目录映射

### 保留

- `crates/agent-core`
- `crates/agent-protocol`
- `crates/agent-app-server`
- `cli`

### 演进

- `apps/agentd` 逐步演进为 `worker`
- `apps/gatewayd` 逐步演进为 `node`
- `crates/agent-app-server-client` 增加 `LocalNode` 实现

### 后续可重命名

- `apps/agentd` -> `apps/cloudagent-worker`
- `apps/gatewayd` -> `apps/cloudagent-node`

第一阶段不强制改名，先改职责。

## 阶段总览

建议按 7 个阶段完成。

### Phase 0：冻结协议边界与术语

目标：

- 先定义哪些协议短期内不变
- 给 Direct Mode 切换留稳定基线

需要保持稳定的部分：

- `AppClientCommand`
- `AppServerMessage`
- `AppServerNotification`
- `AppServerRequest`
- CLI 对 `conversation_id` 的使用方式

本阶段产出：

- 本文档
- 迁移分支

### Phase 1：引入 Target 抽象并宣布唯一主路径

目标：

- 让 CLI 先从“按 transport 选择连接”切换到“按 target 选择连接”

目标：

- 对外目标模型只保留 `LocalNode`
- 为未来 Hub 预留 `HubNode`
- `ConsoleConnection` 不再作为长期对外语义

需要修改：

- [cli/src/app/core/types.rs](/D:/learn/gifti/cloudagent/cli/src/app/core/types.rs:1)
- [cli/src/main.rs](/D:/learn/gifti/cloudagent/cli/src/main.rs:1)
- [cli/src/transport/client.rs](/D:/learn/gifti/cloudagent/cli/src/transport/client.rs:1)

建议动作：

1. 新增 `AppServerTarget`
2. CLI 参数正式切为 `--target`
3. 不再新增任何 `--transport` 兼容语义
4. 旧 `--transport` 若仍存在，只允许在极短施工阶段内部转接，并在后续提交删除
5. 文档和帮助文本从这一阶段开始将 `local-node` 设为唯一目标方向

验收标准：

- CLI 对外语义已从 transport 切到 target
- UI 层无感知 target 内部细节
- 文档中已不再把 embedded 视为长期路径

### Phase 2：扩展 agent-app-server-client facade 并移除 CLI 对旧接法的直接依赖

目标：

- 让 `agent-app-server-client` 成为真正统一的 client facade

当前不足：

- 目前 client 较薄
- 主要针对本地直接接法
- 没有一等 `LocalNode`
- 没有为未来 `Hub` 保留统一事件面

建议新增：

- `crates/agent-app-server-client/src/local_node.rs`

建议补齐能力：

- `send_command`
- `next_event`
- `try_next_event`
- `shutdown`
- 断连语义
- 初始化握手
- 事件回压策略

建议动作：

1. 在 `agent-app-server-client` 中新增 `LocalNodeClient`
2. 在 `AppServerClient` 中增加 `LocalNode` variant
3. 预留 `HubClient` variant，但本轮不接真实 Hub
4. 把事件丢失 / lagged / disconnect 语义统一下来
5. 让 CLI 不关心它是 worker 还是 node

建议优先使用的本地传输：

- 第一优先：loopback WebSocket
- 第二优先：named pipe

不建议第一阶段用复杂的跨平台 socket 抽象把自己拖住。

验收标准：

- `LocalNode` 已成为 CLI 的正式连接面
- `Hub` 所需事件面已有预留
- CLI 不再直接依赖旧接法语义

### Phase 3：把 gatewayd 实做为 Local Node

目标：

- 把 `apps/gatewayd` 从占位程序变成真正的本地 node

当前状态：

- [apps/gatewayd/src/main.rs](/D:/learn/gifti/cloudagent/apps/gatewayd/src/main.rs:1) 仍是 placeholder

本阶段职责：

- 监听本地连接
- 接收来自 CLI 的命令
- 找到会话对应 worker
- 不存在则 spawn worker
- 透传 worker 事件

建议拆分模块：

- `apps/gatewayd/src/main.rs`
- `apps/gatewayd/src/node/mod.rs`
- `apps/gatewayd/src/node/server.rs`
- `apps/gatewayd/src/node/conversation_registry.rs`
- `apps/gatewayd/src/node/worker_manager.rs`
- `apps/gatewayd/src/node/transport.rs`

建议最小内部数据结构：

```rust
struct WorkerConversationHandle {
    conversation_id: String,
    client: AppServerClient,
    last_active_at: Instant,
}
```

```rust
struct ConversationRegistry {
    by_conversation: HashMap<String, WorkerConversationHandle>,
}
```

如果未来需要追踪连接方，再单独增加：

```rust
struct ClientSessionHandle {
    session_id: String,
    conversation_id: String,
    target_node_id: Option<String>,
}
```

这里要刻意保持：

- `ConversationRegistry` 管会话本体和 worker 归属
- `ClientSessionHandle` 管连接方上下文

建议最小 node 行为：

1. `SwitchConversation`
2. `SubmitTurn`
3. `InterruptTurn`
4. `ListConversations`
5. `RequestConversationHistory`
6. `ResolveServerRequest`

第一阶段不必一次性支持所有未来的远程文件和附件中转。

验收标准：

- `gatewayd` 可以常驻启动
- CLI 可以连上 `LocalNode`
- 首次 turn 会触发 worker 启动
- 已有会话会复用现有 worker

### Phase 4：收敛 agentd 为 Worker 角色并删除 CLI 对旧路径的主依赖

目标：

- `agentd` 从多角色入口收敛成 worker 宿主

当前问题：

- `agentd` 同时承担 ready/console/app-server-stdio 多角色

建议保留的能力：

- `app-server-stdio`

建议弱化的能力：

- `console`

建议处理方式：

1. `agentd` 继续保留 `app-server-stdio`
2. `gatewayd` 内部通过 stdio 拉起 `agentd`
3. CLI 彻底不再直接连接 worker
4. `console` 只保留给开发调试
5. `InProcess` 和 CLI 直连 `Stdio` 不再作为用户主入口

如果后续重命名：

- `agentd` => `cloudagent-worker`

这一阶段先不强制改名，先确保职责清晰。

验收标准：

- worker 只负责执行
- node 只负责路由和生命周期
- CLI 不再直接依赖 runtime
- 从用户视角，旧直连 worker 路径已退出主流程

### Phase 5：CLI 默认切到 LocalNode

目标：

- 正式完成“传输层切换”

这一步才算你要的核心落地。

建议默认策略：

- CLI 唯一正式主路径为 `--target local-node`
- 不再把 `embedded` 和 `worker-stdio` 作为正式用户模式保留

CLI 需要调整的点：

- 启动帮助文本
- 参数解析
- 错误提示
- node 不可用时的显式失败提示

建议行为：

1. CLI 只尝试连接本地 node
2. 若 node 不存在，应显式报错或由 CLI 拉起 node 后再连接
3. 不允许静默回退到 embedded
4. 不允许为了“先跑起来”偷偷切回旧路径

建议修改文件：

- [cli/src/main.rs](/D:/learn/gifti/cloudagent/cli/src/main.rs:1)
- [cli/src/transport/client.rs](/D:/learn/gifti/cloudagent/cli/src/transport/client.rs:1)
- [cli/src/app/core/types.rs](/D:/learn/gifti/cloudagent/cli/src/app/core/types.rs:1)

验收标准：

- 用户不需要关心 worker 进程存在与否
- 本地 CLI 已默认走 `cli -> node -> worker`
- 现有 CLI 交互体验不发生明显退化
- 用户侧不再暴露旧 transport 语义

### Phase 6：为 Direct Mode adapter 接入预留接口

目标：

- 在 node 稳定后，为 Telegram / 企业微信等直连平台适配做准备

本阶段不要求立刻实现平台 adapter，但要留好接口。

建议新增统一输入输出模型：

```rust
struct GatewayMessage {
    conversation_id: String,
    sender_id: String,
    content: Vec<InputItem>,
}
```

```rust
enum GatewayOutbound {
    Text(String),
    ApprovalRequest { ... },
    ToolNotice(String),
}
```

建议新建：

- `crates/agent-gateway/src/lib.rs`
- `crates/agent-gateway/src/message.rs`
- `crates/agent-gateway/src/adapter/mod.rs`

本阶段目标不是做完 Telegram，而是让 `node` 知道“CLI 和 IM 最终都会被映射成同一种会话输入”。

验收标准：

- CLI 以外的入口可以复用 node 的会话路由
- `Direct Mode` 开发不再需要改 CLI 主逻辑

### Phase 7：为 Hub Mode 的 CLI 在线设备视图预留协议

目标：

- 明确 Hub 模式下 CLI 必须支持在线设备列表
- 在本轮迁移中把所需协议边界预留出来

本阶段不实现真实 Hub，但必须定义清楚未来 CLI 需要什么。

Hub 模式下 CLI 至少要支持：

1. 查看在线节点列表
2. 查看节点标签、版本、能力摘要
3. 选择目标节点
4. attach 到目标节点上的 conversation
5. 创建新 conversation 并显式指定目标节点

建议新增协议模型：

```rust
struct OnlineNodeSummary {
    node_id: String,
    display_name: String,
    labels: Vec<String>,
    version: String,
    online: bool,
}
```

```rust
enum AppClientCommand {
    // existing...
    ListOnlineNodes,
    SelectTargetNode { node_id: String },
}
```

说明：

- `ListOnlineNodes` 在 Direct Mode 下不应实现为“伪造一个单节点列表”
- 它是 Hub Mode 专属能力
- Direct Mode 下调用该能力应明确报“当前模式不支持”

验收标准：

- 文档层已明确 CLI 在 Hub Mode 下必须支持在线设备列表
- 当前协议设计不阻碍后续新增该能力

## Web / App 多目标节点切换预留

这部分不是当前直连模式的立即交付范围，但必须在本次迁移中提前预留边界。

### 目标

未来 Web 或 App 接入后，应支持：

- 查看在线节点列表
- 选择目标节点
- 在不同目标节点间切换
- attach 到目标节点上的既有 `conversation`
- 在当前选中节点上发起新的 `conversation`

### 设计要求

为了支持多节点切换，本次迁移阶段应避免把“当前会话一定属于本机”写死在协议和状态里。

至少要预留以下模型：

```rust
struct ConversationTarget {
    node_id: Option<String>,
    conversation_id: String,
}
```

含义：

- `node_id = None` 表示本地或当前默认目标
- `node_id = Some(...)` 表示明确指定某个远端节点

若需要描述某次前端连接本身，建议单独使用：

```rust
struct FrontendSession {
    session_id: String,
    active_target: ConversationTarget,
}
```

这样可以明确区分：

- `ConversationTarget` 是“要连接哪个 conversation”
- `FrontendSession` 是“谁正在连接它”

### Direct Mode 下的表现

在 `Direct Mode` 下：

- Web/App 若直接接入本机 node，只能操作该 node 管理的会话
- 不天然支持跨机器切换
- 不提供在线设备列表
- 不提供多节点选择器

因此 Direct Mode 解决的是：

- 如何与一台机器上的 `conversation` 稳定交互

### Hub Mode 下的表现

在 `Hub Mode` 下：

- Web/App 通过 Hub 获取在线节点列表
- 用户选择目标 node
- Hub 按 `node_id + conversation_id` 路由到对应节点
- CLI 同样必须通过 Hub 获取在线节点列表
- CLI 同样必须能够切换目标节点

因此 Hub Mode 解决的是：

- 如何在多台机器之间切换目标节点并继续对话

### 本次迁移需要预留的边界

虽然当前只做直连模式，但本次改造中应尽量满足：

1. `conversation_id` 不被写死为“仅本机唯一”
2. node 内部会话注册表未来可扩展为带 `node_id` 的路由信息
3. CLI 的 target 设计未来能自然扩展到 Web/App 的节点选择器
4. `agent-gateway` 的输入输出模型不依赖 CLI 私有语义

### 对前端的直接收益

如果本次迁移按本文档执行，未来 Web/App 侧不需要重做会话主逻辑，只需要新增：

- 节点列表接口
- 节点选择 UI
- 当前目标节点状态展示

而不需要重写：

- `conversation` 输入协议
- turn 事件流处理
- worker attach / resume 主逻辑

## 具体文件映射

### 一、CLI 层

建议修改：

- [cli/src/main.rs](/D:/learn/gifti/cloudagent/cli/src/main.rs:1)
  - 增加 `--target`
  - 支持 `local-node`
  - 为未来 `hub-node:<id>` 预留解析入口

- [cli/src/app/core/types.rs](/D:/learn/gifti/cloudagent/cli/src/app/core/types.rs:1)
  - 新增 `AppServerTarget`
  - 逐步弱化 `ConsoleConnection` 作为对外入口的地位

- [cli/src/transport/client.rs](/D:/learn/gifti/cloudagent/cli/src/transport/client.rs:1)
  - 新增 `create_client_from_target`
  - 接入 `LocalNodeClient`

大概率不需要大改：

- `cli/src/app/runtime/*`
- `cli/src/app/conversation/*`
- `cli/src/state/*`
- `cli/src/ui/*`

后续在 Hub Mode 必改：

- 节点列表视图
- 节点选择状态
- 当前目标节点展示

### 二、Client facade 层

建议修改：

- [crates/agent-app-server-client/src/lib.rs](/D:/learn/gifti/cloudagent/crates/agent-app-server-client/src/lib.rs:1)
  - 增加 `LocalNode`
  - 统一事件语义

建议新增：

- `crates/agent-app-server-client/src/local_node.rs`

### 三、Node 层

建议修改：

- [apps/gatewayd/src/main.rs](/D:/learn/gifti/cloudagent/apps/gatewayd/src/main.rs:1)

建议新增：

- `apps/gatewayd/src/node/mod.rs`
- `apps/gatewayd/src/node/server.rs`
- `apps/gatewayd/src/node/conversation_registry.rs`
- `apps/gatewayd/src/node/worker_manager.rs`
- `apps/gatewayd/src/node/local_transport.rs`

### 四、Worker 层

建议复用：

- [apps/agentd/src/main.rs](/D:/learn/gifti/cloudagent/apps/agentd/src/main.rs:1)
- [crates/agent-app-server/src/lib.rs](/D:/learn/gifti/cloudagent/crates/agent-app-server/src/lib.rs:1)

第一阶段只做收敛，不做重写。

## Transport 选择建议

### 第一阶段推荐

本地 node transport 选：

- loopback WebSocket

原因：

- 便于和后续 Hub / Remote 模型保持接近
- 调试方便
- CLI 和 node 分层清晰
- 后续 `RemoteNodeClient` 复用思路多

### 第一阶段不推荐

- 一上来用 Named Pipe 作为唯一实现

原因：

- Windows 友好，但后续与 hub/ws 思维割裂
- 调试成本高
- 更容易把问题埋进 transport 细节

可接受策略：

- 第一阶段先用 loopback WebSocket
- 后续若需要，再补 Named Pipe 优化本地体验

## 参数策略

建议最终 CLI 参数：

- `--target local-node`
- `--target hub-node:<node-id>`

施工阶段允许的短期内部参数：

- `--target embedded`
- `--target worker-stdio`

但要求：

1. 只用于施工与测试
2. 不作为最终文档主入口
3. 必须在迁移后续提交中删除或隐藏

建议环境变量：

- `CLOUDAGENT_APP_SERVER_TARGET`
- `CLOUDAGENT_LOCAL_NODE_ADDR`
- `CLOUDAGENT_HUB_ADDR`

## 风险点

### 1. 事件顺序风险

若 `node` 转发事件时顺序变化，会导致 CLI transcript 异常。

要求：

- `TurnStarted`
- `ItemStarted`
- `Delta`
- `ItemCompleted`
- `TurnCompleted`

必须维持相对顺序。

### 2. 断连语义风险

CLI 现在依赖明确的断连信号。

要求：

- 本地 node 断开时，要产生和现有 client 兼容的断连事件

### 3. 会话复用风险

`conversation_id -> worker` 的复用如果实现错误，会导致串会话。

要求：

- node 内部必须显式维护映射表
- 切会话时不能只靠“当前活跃会话”猜测

### 4. 审批请求风险

审批请求是 Direct Mode 和 Hub Mode 以后都会敏感的部分。

要求：

- node 不能吞掉 `ServerRequest`
- 请求超时、worker 结束、CLI 断连时要有明确清理逻辑

## 测试与验收建议

### 单元测试

优先补：

- target 解析
- local-node client 收发
- node conversation registry
- worker spawn / reuse / cleanup
- disconnect / lagged 语义

### 集成测试

至少覆盖：

1. CLI `embedded`
2. CLI `worker-stdio`
3. CLI `local-node`
4. `local-node` 下重复提交同一会话
5. `local-node` 下切换不同会话
6. `local-node` 下审批请求往返
7. `local-node` 下 worker 异常退出

### 手工验收

必须能演示：

1. 启动本地 node
2. CLI 连接 node
3. 发送首次消息，node 自动拉 worker
4. 第二条消息复用同一 worker 会话
5. 切到另一个会话，node 拉起另一 worker 或 attach
6. 中断 turn
7. worker 退出后再次发送消息，node 能恢复

## 推荐实施顺序

按 commit 粒度建议如下。

### Commit 01

目标：

- 新增迁移文档
- 固化术语
- 明确唯一主路径目标

内容：

- 新增 [docs/direct-mode-transport-migration.zh-CN.md](/D:/learn/gifti/cloudagent/docs/direct-mode-transport-migration.zh-CN.md:1)
- 修订 [docs/hub-node-worker-architecture.zh-CN.md](/D:/learn/gifti/cloudagent/docs/hub-node-worker-architecture.zh-CN.md:1)

### Commit 02

目标：

- 引入 `AppServerTarget`

内容：

- 修改 [cli/src/app/core/types.rs](/D:/learn/gifti/cloudagent/cli/src/app/core/types.rs:1)
- 从 `ConsoleConnection` 外显语义切换到 `AppServerTarget`
- 保留旧结构仅作为内部施工细节

### Commit 03

目标：

- CLI 参数切到 `--target`

内容：

- 修改 [cli/src/main.rs](/D:/learn/gifti/cloudagent/cli/src/main.rs:1)
- 新增 `local-node`
- 预留 `hub-node:<id>`
- 帮助文本不再强调 `--transport`

### Commit 04

目标：

- 扩展 client facade

内容：

- 修改 [crates/agent-app-server-client/src/lib.rs](/D:/learn/gifti/cloudagent/crates/agent-app-server-client/src/lib.rs:1)
- 新增 `crates/agent-app-server-client/src/local_node.rs`
- 统一 `disconnect / lagged / shutdown` 语义

### Commit 05

目标：

- 创建本地 node 基础骨架

内容：

- 扩展 [apps/gatewayd/src/main.rs](/D:/learn/gifti/cloudagent/apps/gatewayd/src/main.rs:1)
- 新增：
  - `apps/gatewayd/src/node/mod.rs`
  - `apps/gatewayd/src/node/server.rs`
  - `apps/gatewayd/src/node/conversation_registry.rs`
  - `apps/gatewayd/src/node/worker_manager.rs`
  - `apps/gatewayd/src/node/local_transport.rs`

### Commit 06

目标：

- 打通 node 拉起 worker

内容：

- `gatewayd` 通过 stdio 拉起 `agentd app-server-stdio`
- 管理 `conversation_id -> worker` 映射
- 支持首次启动 worker

### Commit 07

目标：

- 打通 `cli -> local node -> worker`

内容：

- CLI 通过 `LocalNodeClient` 发命令
- node 转发到 worker
- worker 事件回流到 CLI

### Commit 08

目标：

- 删除 CLI 对旧主路径的依赖

内容：

- CLI 不再默认 `Embedded`
- CLI 不再默认 `WorkerStdio`
- 旧路径若仍保留，只允许在测试或内部调试中使用

### Commit 09

目标：

- 清理旧参数和旧帮助文案

内容：

- 删除或隐藏 `--transport` 主文档入口
- 清理旧错误提示
- 清理旧路径说明

### Commit 10

目标：

- 收敛 `agentd` 为 worker 角色

内容：

- 调整 [apps/agentd/src/main.rs](/D:/learn/gifti/cloudagent/apps/agentd/src/main.rs:1)
- 明确 `console` 只用于开发
- 主路径仅为 node 拉 worker

### Commit 11

目标：

- 增加 Direct Mode 所需测试

内容：

- local-node client 测试
- node conversation registry 测试
- worker spawn / reuse / cleanup 测试
- CLI 集成测试

### Commit 12

目标：

- 为 Hub Mode CLI 在线设备视图预留协议

内容：

- 在协议层预留 `ListOnlineNodes`
- 在 target 层预留 `HubNode`
- 不实现真实 Hub

### Commit 13

目标：

- 开始 Direct Mode 平台 adapter

内容：

- `agent-gateway` 初始抽象
- `GatewayMessage`
- `GatewayOutbound`

## 本文档对应的结论

这次“传输层切换”不应理解为重写 CLI，而应理解为：

- 保持现有 CLI 事件驱动结构
- 把 CLI 底下的连接目标从“本地 runtime/worker”逐步切到“本地 node”
- 让 node 成为 Direct Mode 的真正入口

完成这份方案后，下一步就可以专注开发：

- `Direct Mode`

而不是继续在 CLI 和 worker 的耦合关系上反复返工。

Hub 应作为下一阶段，在 `node` 和 `client` 已经分层稳定之后再接入。
