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
- 无法天然支持 attach 到其他机器上的 conversation
- 跨节点远程互传能力弱
- 多平台适配要么在每个 node 上配置，要么只在某些 node 上启用

## 平台云与自建 Hub 的关系

在 `Direct Mode` 下，Telegram / Discord / WeCom / Weibo / Lark 这类平台云本质上承担的是“远程消息入口”。

它们可以解决：

- 用户消息如何到达本机 agent
- 本机 agent 如何把回复发回用户

但它们通常不能统一解决：

- 多节点服务发现
- 某个 conversation 当前位于哪个 node
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
- 维护 conversation 到 node 的路由映射
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
- 上报本机能力、标签、版本、当前活跃 conversation
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
- 执行 conversation / turn / item 生命周期
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
- 客户端选择目标 node 或 conversation
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

建议沿用统一的核心会话模型：

- `Conversation`
- `Turn`
- `Item`

定义：

- `Conversation` 表示一个长期会话
- `Turn` 表示一次用户输入到 agent 输出完成的执行轮次
- `Item` 表示 turn 内部的消息、工具调用、审批请求、工具结果等事件

这样可以统一承载：

- CLI 对话
- Web 对话
- IM 对话
- 远程 attach
- 本地恢复
- 审批与通知

## 控制面与数据面分离

长期应明确区分：

- 控制面
- 数据面

### 控制面

控制面关注：

- 节点注册
- 会话路由
- attach / interrupt / resume / close
- 在线状态与能力上报
- 权限和审计

典型由 `Hub` 和 `Node` 协同承担。

### 数据面

数据面关注：

- turn/item 事件流
- 工具输出
- 文件与附件传输
- 最终消息回传

典型由 `Worker` 产出，再由 `Node` / `Hub` 中转。

这样分离后，后续才能逐步支持：

- 只中转控制面，不中转大文件
- 大文件走对象存储
- 不同 gateway 共享同一套会话控制协议

## 网关抽象

无论是 `Hub Mode` 还是 `Direct Mode`，都应共享同一套 gateway 抽象。

统一 gateway 抽象至少应解决：

- 用户消息输入
- agent 输出文本
- 工具结果与卡片降级
- 文件/图片/附件发送
- 审批请求
- 按钮回调

这样未来才能同时支持：

- CLI
- Web
- Telegram
- Discord
- 企业微信
- 飞书

而不用为每个平台重写会话主逻辑。

## 第一阶段建议实现顺序

### Phase 1：本机 node / worker 边界稳定

目标：

- 在单机环境中先跑通 `node -> worker`
- 沿用现有 runtime，不先引入公网 Hub

建议：

- 先把 `cloudagent-worker` 设计成现有 agent runtime 的宿主
- `cloudagent-node` 只负责生命周期管理和本地 gateway 接入

### Phase 2：Direct Mode 跑通

目标：

- 不依赖 Hub，先让本机 node 能通过至少一个 IM 平台对话

建议：

- 先支持一个最简单的平台 adapter
- 统一消息模型和会话恢复逻辑

### Phase 3：Hub Mode 最小闭环

目标：

- 让多节点可注册、可发现、可 attach

建议：

- `cloudagent-node -> cloudagent-hub` 注册与心跳
- Hub 维护在线节点与 conversation 路由表
- CLI / Web 通过 Hub attach 到目标会话

### Phase 4：跨节点中转与远程文件

目标：

- 支持更完整的远程体验

建议：

- 中转 turn/item 实时流
- 中转图片和附件
- 补统一审计与权限边界

## 目录与组件建议

长期可考虑以下组件布局：

- `apps/cloudagent-hub`
- `apps/cloudagent-node`
- `apps/cloudagent-worker`
- `crates/agent-gateway`
- `crates/agent-runtime`
- `crates/agent-remote-protocol`

其中：

- `cloudagent-worker` 可以逐步复用当前 `agentd` / `app-server-stdio` 逻辑
- `cloudagent-node` 作为新的轻量守护进程
- `cloudagent-hub` 作为公网控制面

如果只运行 `Direct Mode`：

- 可仅运行 `cloudagent-node`
- 由 `cloudagent-node` 按需拉起 `cloudagent-worker`
- 不要求部署 `cloudagent-hub`

## 必须保持的约束

后续无论怎么演进，至少要保持以下约束：

1. agent 执行核心仍在端侧 worker，不迁到中心 Hub
2. Hub 优先承担控制面，不承担端侧私有工具逻辑
3. node 必须轻量常驻，可低成本保持在线
4. worker 必须按需拉起，可 attach / interrupt / recycle
5. conversation / turn / item 必须保持统一模型
6. Direct Mode 和 Hub Mode 必须共享同一套 runtime 与 gateway 抽象
7. Hub Mode 和 Direct Mode 必须共享同一套会话协议

## 当前建议

当前建议是：

- 对单机和轻量个人场景，使用 `Direct Mode`，直接在 IM 平台与本机 agent 对话
- 对多节点互连和统一控制场景，使用 `Hub Mode`，由 Hub 负责发现、路由和中转

这两者不是竞争关系，而是同一个系统的两种部署形态。

## 备注

这份文档是未来开发计划，不代表这些组件已经全部存在或已经进入当前主线实现。

它的作用是：

- 约束未来远程架构演化方向
- 避免把远程接入能力做成一次性的临时拼接
- 让 `Hub Mode` 与 `Direct Mode` 从一开始就沿同一套模型演进
