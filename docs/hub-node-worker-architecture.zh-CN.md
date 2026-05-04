# CloudAgent Hub / Node / Worker 架构草案

## 目标

本文档定义 CloudAgent 在多端互连场景下的第一阶段总体架构。

目标不是做一个传统监控平台，而是让部署在不同端侧的 CloudAgent：

- 可以被统一发现和查看在线状态
- 可以从 CLI / Web / IM 等入口远程接入
- 可以将请求路由到目标端侧的活跃 agent 会话
- 可以在端侧保持轻量常驻，并按需拉起真正执行对话的 worker
- 为未来的远程互传、审批、通知、计划任务留出稳定扩展点
- 同时支持有 Hub 和无 Hub 的两种部署模式

## 设计结论

CloudAgent 不应只支持单一部署方式，而应支持两种运行模式：

- `Hub Mode`
- `Direct Mode`

其中：

- `Hub Mode` 面向多节点统一发现、统一路由、统一 Web / CLI / IM 入口
- `Direct Mode` 面向单机或少量节点场景，允许本机直接接入 IM 平台，不依赖自建 Hub

对于“多端互连、统一发现、远程互传”目标，推荐使用 `Hub Mode`。
对于“像 cc-connect 一样，直接在 IM 平台与本机 agent 对话”的目标，使用 `Direct Mode`。

这意味着 CloudAgent 的远程架构应是混合架构，而不是只能选择其中一种。

在 `Hub Mode` 下，CloudAgent 需要引入一个公网可达的 `Hub`。

这不是为了把 agent 执行迁到中心，而是为了集中解决以下问题：

- 服务发现
- 节点在线状态维护
- 会话路由
- 无法直连时的消息中转
- IM / Web / CLI 的统一远程入口
- 远程文件与附件中转

系统的核心运行角色仍然分为三个：

- `cloudagent-hub`
- `cloudagent-node`
- `cloudagent-worker`

其中：

- `Hub` 常驻在公网服务器，负责控制面与消息中转
- `Node` 常驻在每个端侧，负责本机注册、心跳、拉起 worker
- `Worker` 按需启动，负责真正执行 agent 会话

在 `Direct Mode` 下，不启动 `Hub`，而是由本机 `Node` 直接承担本地 gateway 入口职责。

## 为什么不能只靠密钥直接互连

单个密钥只能解决身份认证，不能解决连接建立。

如果没有 Hub 或同类能力，系统仍然会面对：

- NAT / 防火墙导致的入站不可达
- 动态 IP 导致的地址变化
- 不同端侧之间缺少统一发现入口
- 无法直连时缺少回退路径
- IM 平台无法直接与所有端侧稳定通信

因此，第一阶段不采用纯 P2P 直连作为主方案。

## 两种部署模式

### `Hub Mode`

适用于：

- 有多台端侧设备
- 需要统一查看在线节点
- 需要 CLI / Web / IM 统一入口
- 需要远程互传
- 需要统一审计与权限控制

特点：

- node 主动连接 hub
- 客户端先连接 hub，再由 hub 路由到目标 node
- IM 平台优先接 hub
- 多节点发现和会话路由由 hub 统一解决

### `Direct Mode`

适用于：

- 单机部署
- 用户不希望购买服务器
- 只希望通过某个 IM 平台直接与本机 agent 对话
- 不要求统一多节点发现

特点：

- 不启动 hub
- 本机 node 直接连接 Telegram / Discord / 企微 / 微博等远程平台
- 平台云本身承担消息入口和中转
- 会话只在本机路由，不涉及跨节点发现

限制：

- 无法天然支持统一节点列表
- 无法天然支持 attach 到其他机器上的 thread
- 跨节点远程互传能力弱
- 多平台适配要么在每个 node 上配置，要么只在某些 node 上启用

## 平台云与自建 Hub 的关系

在 `Direct Mode` 下，Telegram / Discord / WeCom / Weibo / Lark 这类平台云本质上承担的是“远程消息入口”。

它们可以解决：

- 用户消息如何到达本机 agent
- 本机 agent 如何把回复发回用户

但它们通常不能统一解决：

- 多节点服务发现
- 某条 thread 当前位于哪个 node
- 任意节点之间的文件中转
- 跨节点 attach
- 统一权限与审计

因此：

- `Direct Mode` 可以替代 Hub 的“消息入口”
- `Direct Mode` 不能完整替代 Hub 的“多节点控制面”

## 角色划分

### `cloudagent-hub`

Hub 是系统的公网控制面与中转面。

职责：

- 接收各端侧 node 的注册和心跳
- 维护在线节点目录与能力信息
- 维护 thread 到 node 的路由映射
- 为 CLI / Web / IM 提供统一接入入口
- 在客户端与目标 node 之间中转实时事件流
- 承载远程文件与附件的中转能力
- 记录审计日志和访问记录

Hub 不直接承担 agent 的核心推理执行，也不持有端侧具体工具实现。

### `cloudagent-node`

Node 是每个端侧的轻量常驻守护进程。

职责：

- 启动后主动连接 Hub
- 定期发送心跳
- 上报本机能力、标签、版本、当前活跃 thread
- 接收 Hub 下发的 attach / start / interrupt / file fetch 等请求
- 维护本机 worker 生命周期
- 在无活跃 worker 时按需拉起 worker
- 将 worker 的事件流、状态、输出转发给 Hub

Node 应尽量保持低内存占用，不长期持有大上下文和重工具运行时。

在 `Direct Mode` 下，Node 还需要承担：

- 本机 IM gateway 接入
- 平台消息到本地会话的映射
- 平台能力的文本 / 按钮 / 卡片降级输出

### `cloudagent-worker`

Worker 是真正执行 agent 会话的进程。

职责：

- 承载 `agent-runtime`
- 执行 thread / turn / item 生命周期
- 调用 tools
- 写入本地持久化状态
- 在被 attach 时恢复已有会话
- 支持中断、超时、回收

Worker 不直接暴露公网入口。

## 总体连接模型

### 节点与 Hub

每个 node 启动后主动与 Hub 建立出站长连接。

建议：

- 传输层使用 `WebSocket`
- 协议层使用 `JSON-RPC`
- 连接认证使用长期节点密钥签名或 Hub 下发 token

这样可以避免要求每个端侧具备公网入站能力。

### 客户端与 Hub

CLI、Web、IM 都先接入 Hub，而不是直接接端侧 node。

流程：

- 客户端先向 Hub 查询在线节点
- 客户端选择目标 node 或 thread
- Hub 将请求路由到目标 node
- node attach 或拉起 worker
- worker 事件流经 `worker -> node -> hub -> client` 返回

### 客户端与 Node（Direct Mode）

在 `Direct Mode` 下，客户端并不先连接自建 Hub，而是通过平台云接入本机 node。

流程：

- 用户在 Telegram / Discord / 企微 / 微博等平台发送消息
- 平台云将消息推送或暴露给本机 node
- node 将消息转换为统一 `GatewayMessage`
- node attach 或拉起本机 worker
- worker 输出通过 node 再返回平台云

这一路径的关键点不是 node 公网暴露，而是 node 主动连接平台云。

### Node 与 Worker

Node 与 Worker 建议先采用本机 IPC 通信。

候选方式：

- stdio
- 本机 Unix Socket / Named Pipe
- 本机 loopback WebSocket

第一阶段建议优先沿用现有 `app-server-stdio` 思路，降低实现成本。

## 会话模型

建议沿用 Codex 风格的核心模型：

- `Thread`
- `Turn`
- `Item`

定义：

- `Thread` 表示一个长期会话
- `Turn` 表示一次用户输入到 agent 输出完成的执行轮次
- `Item` 表示 turn 内部的消息、工具调用、审批请求、工具结果等事件

这样可以统一承载：

- CLI 对话
- Web 对话
- IM 对话
- 审批事件
- 文件传输事件
- 后续 review / schedule / notification

同一套会话模型必须同时服务：

- `Hub Mode`
- `Direct Mode`

## 第一阶段最小数据模型

### Node

- `node_id`
- `label`
- `public_key`
- `version`
- `status`
- `capabilities`
- `tags`
- `last_seen_at`
- `active_threads`

### Thread

- `thread_id`
- `node_id`
- `title`
- `status`
- `source`
- `created_at`
- `updated_at`
- `last_activity_at`

### Turn

- `turn_id`
- `thread_id`
- `status`
- `started_at`
- `completed_at`

### Attachment

- `blob_id`
- `name`
- `content_type`
- `size`
- `created_at`
- `source_node_id`

## 第一阶段最小 RPC

### Hub-facing

- `initialize`
- `node/register`
- `node/heartbeat`
- `node/status/update`
- `thread/route/update`

### Client-facing

- `node/list`
- `node/read`
- `thread/list`
- `thread/start`
- `thread/resume`
- `thread/attach`
- `turn/start`
- `turn/interrupt`
- `blob/put`
- `blob/get`

### Direct gateway-facing

- `gateway/message/inbound`
- `gateway/session/start`
- `gateway/session/attach`
- `gateway/approval/respond`
- `gateway/blob/send`

### Event notifications

- `node/online`
- `node/offline`
- `thread/started`
- `turn/started`
- `item/started`
- `item/completed`
- `item/agent_message/delta`
- `turn/completed`

## 核心流程

### 1. Node 上线

1. `cloudagent-node` 启动
2. 读取本机身份密钥与配置
3. 主动连接 Hub
4. 发送 `node/register`
5. 定期发送 `node/heartbeat`
6. Hub 将节点标记为 `online`

### 2. 客户端查看在线节点

1. CLI / Web 连接 Hub
2. 调用 `node/list`
3. Hub 返回在线节点、能力、标签、活跃 thread 摘要

### 2A. Direct 模式下 IM 发起对话

1. 用户在 IM 平台向机器人发送消息
2. 平台云将消息推送给本机 node
3. node 解析平台上下文并映射到本地 session key
4. 若已有活跃 worker，则 attach
5. 若无，则拉起 worker 并开始本机会话
6. worker 事件流经 node 转换后返回平台

### 3. attach 到远端会话

1. 客户端选择目标 node 或 thread
2. 调用 `thread/attach`
3. Hub 将请求路由到目标 node
4. node 查找本机是否已有对应 worker
5. 若已有，则 attach
6. 若无，则拉起 worker 并恢复 thread
7. worker 的事件流通过 node 转发给 Hub
8. Hub 将流式结果推送给客户端

### 4. 远程发起新对话

1. 客户端调用 `thread/start`
2. Hub 为目标 node 选择路由
3. node 拉起 worker
4. worker 创建新 thread
5. 后续 `turn/start` 在该 thread 上继续执行

### 5. 远程互传

第一阶段不要求 node 之间直传大文件。

流程：

1. 发送方向 Hub 执行 `blob/put`
2. Hub 返回 `blob_id`
3. 客户端消息中引用 `blob_id`
4. 目标 node 收到请求后向 Hub 执行 `blob/get`
5. node 将文件交给 worker 或本地处理逻辑

这样可以统一支持：

- IM 附件
- Web 上传
- CLI 文件引用
- 以后 node 间互传

## IM 接入策略

CloudAgent 不应只支持一种 IM 接入方式，而应同时支持：

- `Hub IM Mode`
- `Direct IM Mode`

### `Hub IM Mode`

IM 平台集中接在 Hub 层。

适合：

- 多节点系统
- 统一入口
- 平台消息可能路由到任意 node

优点：

- 只需实现一次平台适配
- 无需每个端侧都配置 Telegram / Discord / 企微机器人
- 平台消息可以统一路由到任意 node
- 审批、按钮、降级文本交互都能统一抽象
- 审计和权限更集中

Hub 应将平台消息转换为统一 `GatewayMessage`，再路由到目标 node。

### `Direct IM Mode`

IM 平台直接接在某个 node 上。

适合：

- 单机运行
- 轻量个人场景
- 不希望部署自建 Hub

优点：

- 无需额外服务器
- 使用体验接近 cc-connect
- 可以快速获得“手机或 IM 远程对话本机 agent”的能力

缺点：

- 会话只能落到本机，不能统一发现其他节点
- 同一个平台适配可能需要在多个 node 重复配置
- 审计、权限、文件中转等能力会更分散

### 结论

IM 不是必须只能接 Hub，也不应被设计成只能接 Node。

CloudAgent 应支持：

- 在 `Hub Mode` 下让 IM 接入 Hub
- 在 `Direct Mode` 下让 IM 直接接入 Node

但无论哪种模式，平台适配层最终都应输出统一的 `GatewayMessage`，而不是把平台差异泄漏到 `agent-runtime`。

## 与现有 crate 边界的对应关系

### `agent-core`

继续负责：

- thread / turn / item 核心模型
- agent 编排与会话语义

### `agent-runtime`

继续负责：

- worker 执行生命周期
- 恢复会话
- 中断、超时、取消

### `agent-gateway`

负责新增的远程入口抽象：

- Hub / Node 之间的协议模型
- GatewayMessage
- 路由请求与事件模型
- IM / Web / CLI 统一消息模型
- Direct IM adapter 抽象
- Hub IM adapter 抽象

### `storage`

负责新增的持久化：

- node 注册状态
- thread 路由表
- blob 元数据
- 审计记录

## 二进制建议

建议新增三个入口：

- `apps/cloudagent-hub`
- `apps/cloudagent-node`
- `apps/cloudagent-worker`

其中：

- `cloudagent-worker` 可以逐步复用当前 `agentd` / `app-server-stdio` 逻辑
- `cloudagent-node` 作为新的轻量守护进程
- `cloudagent-hub` 作为公网控制面

如果只运行 `Direct Mode`：

- 可仅运行 `cloudagent-node`
- 由 `cloudagent-node` 按需拉起 `cloudagent-worker`
- 不要求部署 `cloudagent-hub`

## 安全边界

第一阶段至少需要：

- node 身份密钥
- Hub 访问 token 或签名认证
- 客户端访问 Hub 的鉴权
- 基于 node / thread / operation 的授权控制
- 审计日志

在 `Direct Mode` 下，还需要：

- 平台 bot token / app secret
- 平台级别的 allowlist / admin 控制
- 平台消息来源校验

不要采用“拿到一个总密钥即可控制所有节点”的粗粒度方案。

建议把权限拆为：

- 查看节点列表
- attach thread
- 启动新 thread
- 中断 turn
- 上传 / 下载 blob
- 执行高风险操作审批

## 设计约束

1. Hub 负责发现与路由，不直接侵入 agent 核心执行。
2. Node 应尽量保持低资源占用。
3. Worker 应按需拉起，空闲可回收。
4. CLI / Web / IM 必须共享统一会话协议。
5. 文件与消息通道要分离，避免大文件阻塞事件流。
6. 不以纯 P2P 直连作为第一阶段主路径。
7. Hub Mode 和 Direct Mode 必须共享同一套 gateway 抽象。

## 第一阶段实施顺序

1. 定义 `agent-gateway` 的统一协议模型
2. 实现 `cloudagent-node` 本地 worker 管理
3. 实现 `Direct Mode` 的第一个 IM adapter
4. 打通 `node -> worker` 的流式转发
5. 实现 `cloudagent-node -> cloudagent-hub` 注册与心跳
6. 实现 Hub 的 `node/list`
7. 实现 Hub 路由到 node 的 `thread/start` / `turn/start`
8. 接入第一个客户端入口，优先 CLI 或 Web
9. 最后实现 `blob/put` / `blob/get`

## 暂不处理的内容

第一阶段先不解决：

- 复杂 P2P 打洞
- 多 Hub 高可用
- 跨 Hub federation
- node 间直传文件
- 完整移动端 App
- 富交互审批 UI

这些都应建立在第一阶段的稳定 Hub / Node / Worker 模型之上。

## 总结

CloudAgent 的远程架构应是混合架构，而不是单一路径。

- 对单机和轻量个人场景，使用 `Direct Mode`，直接在 IM 平台与本机 agent 对话
- 对多节点互连和统一控制场景，使用 `Hub Mode`，由 Hub 负责发现、路由和中转

无论使用哪种模式，都不应从“每个端侧直接暴露完整 agent”开始，而应坚持“Node 轻量常驻、Worker 按需执行、Gateway 协议统一”的原则。
