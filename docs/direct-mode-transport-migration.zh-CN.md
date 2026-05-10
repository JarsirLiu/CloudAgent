# CloudAgent 直连模式传输层切换与分阶段迁移方案

## 文档定位

本文档是 CloudAgent 传输层迁移的**主实施文档**。

本文档同时约束两件事：

1. 产品目标
2. 实现路径

产品目标不能跑偏，实现路径也不能再偏离 `Codex` 的稳定传输层做法。

本文档替代“只讨论 node/worker 分层，不约束 client/transport 形状”的旧理解。

本文档同时吸收并替代此前单独存在的 `codex-transport-alignment-remediation.zh-CN.md`。

也就是说：

- 传输层的长期约束以本文档为准
- 不再维护一份平行的“对齐整改清单”

本文档默认参考以下实现基线：

- [D:\\learn\\AIbac\\JiangFang\\codex\\codex-rs\\app-server-client\\src\\lib.rs](/D:/learn/AIbac/JiangFang/codex/codex-rs/app-server-client/src/lib.rs:1)
- [D:\\learn\\AIbac\\JiangFang\\codex\\codex-rs\\app-server-client\\src\\remote.rs](/D:/learn/AIbac/JiangFang/codex/codex-rs/app-server-client/src/remote.rs:1)
- [D:\\learn\\AIbac\\JiangFang\\codex\\codex-rs\\tui\\src\\lib.rs](/D:/learn/AIbac/JiangFang/codex/codex-rs/tui/src/lib.rs:284)

本文档的核心要求只有一句话：

- **CloudAgent 的 CLI / Web / App / IM 入口，最终都必须通过统一稳定的 app-server client surface 与 core 对接，而不能各自发明一套协议。**

## 目标

本文档完成后，系统必须支持以下能力：

### Direct Mode

`Direct Mode` 不是本机 CLI 连本机 agent。

`Direct Mode` 指的是：

- 远程用户通过 `IM / App / Web` 等入口
- 直接连接某一台具体设备上的 `node`
- 由该设备上的 `node` attach 或拉起本地 `worker`
- 远程用户与该远程设备上的 agent 进行交互

调用链：

- `IM/App/Web -> target node -> worker`

### Hub Mode

`Hub Mode` 指的是：

- 用户先登录统一入口
- 查看在线设备
- 选择目标设备
- 由 `hub` 路由到目标设备上的 `node`
- 再由目标 `node` attach 或拉起本地 `worker`

调用链：

- `CLI/Web/App/IM -> hub -> target node -> worker`

### 同账号多设备

系统最终必须支持：

- 用户在任意设备登录
- 与自己账号下任意在线设备上的 agent 交互
- 同时也能与当前登录设备自己本机的 agent 交互

## 范围边界

本轮必须完成：

- 统一稳定的 app-server transport surface
- 可支撑 `Direct Mode` 的 `node -> worker` 执行底座
- 为 `Hub Mode` 预留稳定的 target / remote / routing 边界

本轮必须预留但不必完整实现：

- hub 服务本体
- 节点注册与心跳闭环
- 在线设备目录的完整产品化
- Web/App/IM 平台 adapter 的最终实现

本轮明确不允许：

- 让 CLI、Web、IM 各自直连 `core`
- 长期保留 `LocalNode`、`Hub`、`Stdio` 三套外部协议语义
- 用“旧路径自动回退”掩盖新架构未完成
- 把 `node` 对外暴露成私有业务命令代理，而不是正式 remote app-server host

## 最终架构

长期稳定目标如下：

- `surface -> AppServerClient::{InProcess|Remote} -> app-server host -> core`

其中：

- `surface` 指 CLI / Web / App / IM adapter
- `AppServerClient` 是统一 facade
- `InProcess` 仅用于本地嵌入式调试或开发场景
- `Remote` 是长期稳定的一等传输形态
- `app-server host` 在本项目里可由 `gatewayd` 或未来 `hubd` 对外提供
- `core` 指 `agent-core + agent-app-server`

对本项目的落地映射是：

- `Direct Mode`：`surface -> Remote -> gatewayd(node) -> worker(agentd) -> core`
- `Hub Mode`：`surface -> Remote -> hubd -> gatewayd(node) -> worker(agentd) -> core`

## 与 Codex 的对齐原则

CloudAgent 不要求目录和名字逐字复制 Codex，但传输层原则必须对齐：

1. surface 只依赖统一 `AppServerClient`
2. target 与 transport 分离
3. transport 长期只保留 `InProcess` 与 `Remote`
4. remote transport 必须有明确握手
5. request/response 与 event stream 必须分清
6. server request 的 resolve/reject 必须是统一 client API，不是 UI 手拼命令
7. 断连、回压、事件丢失语义必须统一

## 当前现状

当前仓库已经具备以下基础：

- `agent-core` 已统一 `conversation / turn / item` 模型
- `agent-protocol` 已定义 `AppClientCommand`、`AppServerMessage`
- `agent-app-server` 已具备 worker 对外协议边界
- `gatewayd` 已具备 resident node、conversation registry、worker reuse、idle recycle 基础
- `gatewayd` 已具备 remote transport 握手与 typed request/response 基础
- `cli` 已完成 target 化，并通过 `agent-app-server-client` 消费事件
- `local-node` 已收敛为 target/deployment 语义，CLI 连接本地 node 已通过统一 `Remote` client 进行

关键文件：

- [cli/src/app/core/types.rs](/D:/learn/gifti/cloudagent/cli/src/app/core/types.rs:1)
- [cli/src/transport/client.rs](/D:/learn/gifti/cloudagent/cli/src/transport/client.rs:1)
- [cli/src/main.rs](/D:/learn/gifti/cloudagent/cli/src/main.rs:1)
- [crates/agent-app-server-client/src/lib.rs](/D:/learn/gifti/cloudagent/crates/agent-app-server-client/src/lib.rs:1)
- [crates/agent-app-server-client/src/local_node.rs](/D:/learn/gifti/cloudagent/crates/agent-app-server-client/src/local_node.rs:1)
- [apps/gatewayd/src/node/server.rs](/D:/learn/gifti/cloudagent/apps/gatewayd/src/node/server.rs:1)
- [apps/gatewayd/src/node/command_router.rs](/D:/learn/gifti/cloudagent/apps/gatewayd/src/node/command_router.rs:1)

## 当前差距

当前实现距离本文档目标，剩余的核心差距主要有 3 个：

1. `Direct Mode` 的平台 adapter 仍未完全收敛到统一 `AppServerClient` surface，`agent-gateway` 侧还有继续长出一层 direct 专用 client contract 的风险
2. `Hub Mode` 只有 target、typed request 预留和错误语义，还没有正式的 `hubd`、在线节点目录与 target node 路由闭环
3. `Web/App/IM` adapter 仍未作为正式产品入口落到同一套 remote surface 上

## 术语约定

为了避免概念混淆，本文档统一使用：

- `conversation`：长期会话本体
- `session`：某个入口对 conversation 的一次连接或附着上下文
- `turn`：一次输入到输出完成的执行轮次
- `item`：turn 内部事件
- `target`：surface 想要连接的目标
- `transport`：底层承载方式
- `client surface`：surface 依赖的统一客户端 API
- `data_root_dir`：应用数据根目录，承载 conversations / logs / memory，和 workspace_root 分离

特别说明：

- `conversation` 不是 `session`
- `local-node` 是 target/deployment 概念，不是长期 client 协议名
- `hub-node` 是 routing target 概念，不是长期 client 协议名
- `local-node` 与 `hub-node` 不能重新长成第二套 client surface 名词

## 数据根约定

为了和 Codex 一样把“工作区语义”和“应用数据语义”分开，CloudAgent 现在要求：

- `workspace_root` 只表示工具执行、仓库读写、相对路径解析的根
- `data_root_dir` 表示应用数据根
- `conversation_store_dir` 与 `memory.root_dir` 默认从 `data_root_dir` 派生
- `logs` 也必须写入 `data_root_dir/logs`

默认规则：

- dev 模式：`data_root_dir = <workspace_root>/data`
- release 模式：`data_root_dir = ~/.cloudagent/data`

默认派生目录：

- `conversation_store_dir = <data_root_dir>/conversations`
- `memory.root_dir = <data_root_dir>/state/memory`
- `logs = <data_root_dir>/logs`

只有在高级场景下，才应该单独覆盖：

- `conversation_store_dir`
- `memory.root_dir`

明确不允许：

- 让日志、会话、memory 隐式跟随启动目录漂移
- 让不同 surface 各自定义不同的数据根默认语义

## 目标分层

### 1. Target

Target 表达“我要连谁”，不表达“我怎么传”。

建议长期形态：

```rust
pub enum AppServerTarget {
    LocalNode,
    HubNode { node_id: String },
}
```

含义：

- `LocalNode`：连接本地或已知单设备 node
- `HubNode`：通过 hub 路由到目标 node

### 2. Client Surface

Client surface 表达“surface 如何使用 app-server”，必须长期稳定。

建议长期形态：

```rust
pub enum AppServerClient {
    InProcess(...),
    Remote(...),
}
```

这一点必须与 Codex 对齐。

说明：

- `LocalNode` 不应成为长期 `AppServerClient` variant
- `Hub` 也不应成为长期 `AppServerClient` variant
- `local-node` 与 `hub-node` 只影响 target 与连接参数，不应分裂 client 协议
- `Direct Mode` adapter 也不应定义长期独立的 node client surface；它可以封装统一 client，但不能重新发明一层并行协议

### 3. App-Server Host

对外提供正式 remote app-server 协议的宿主。

本项目中：

- `gatewayd` 是 node 侧 remote app-server host
- `hubd` 是 hub 侧 remote routing/app-server host

### 4. Worker

执行宿主，负责真正的 agent turn。

本项目中：

- `agentd` 逐步收敛成 worker

## 稳定传输层规范

为了让 CLI、Web、App、IM 都能稳定接 core，必须先冻结统一协议面。

### 必须具备的握手

- `initialize`
- `initialized`

### 必须具备的 request/response 面

- `list_conversations`
- `request_conversation_history`
- `request_conversation_history_page`
- `request_conversation_status`
- `list_online_nodes`
- 后续 hub 所需的 target/select/read 能力

补充强约束：

- `conversation/list`
- `conversation/status`
- `conversation/history`
- `conversation/historyPage`

这四类能力的初始化读取与显式读取必须走 typed request/response。

同名 notification 只保留以下职责：

- 增量同步
- 重连后的状态投影视图刷新
- node 内部 registry / UI projection 的更新来源

明确不允许：

- typed request 成功后，再向同一 client 回放同名 notification 当初始化面
- CLI/UI 把同名 notification 当首屏 bootstrap 主来源
- 让同一能力同时承担“读取面”和“事件面”，形成双源语义

### 必须具备的 command/notification 能力

- `submit_turn`
- `interrupt_turn`
- `switch_conversation`
- `set_conversation_title`
- `archive_conversation`
- `delete_conversation`

### 必须具备的 server-request 能力

- `resolve_server_request`
- `reject_server_request`

### 必须具备的事件语义

- `delta`
- `item_started`
- `item_completed`
- `turn_started`
- `turn_completed`
- `server_request_requested`
- `server_request_resolved`
- `disconnected`
- `lagged`
- `error`

### 必须具备的错误分层

- transport error
- server error
- deserialize error

这应对应类似 Codex 的 `TypedRequestError`。

## 传输承载选择

本文档不把传输承载和协议面绑死。

允许的承载：

- loopback websocket
- loopback tcp
- named pipe

但必须先满足：

- 统一 remote client surface
- 统一 initialize/initialized
- 统一 request/response correlation
- 统一 event stream

也就是说，先定协议，再选 carrier。

## 推荐目录映射

### 保留并复用

- `crates/agent-core`
- `crates/agent-protocol`
- `crates/agent-app-server`
- `crates/agent-app-server-client`
- `cli`

### 继续演进

- `apps/agentd` 逐步收敛为 worker
- `apps/gatewayd` 逐步收敛为 node 侧 remote app-server host
- `crates/agent-gateway` 用于 Direct Mode / IM adapter 统一抽象

### 后续新增

- `apps/hubd`：Hub Mode 远端路由与在线设备控制面

## 分阶段迁移

### Phase 0：冻结目标、术语与验收标准

目标：

- 统一 Direct Mode 的定义
- 统一 Hub Mode 的定义
- 明确长期 client surface 必须对齐 Codex

本阶段产出：

- 本文档
- 对齐整改文档
- 验收标准

### Phase 1：冻结 target，停止继续发明新外部语义

目标：

- CLI 对外只暴露 target
- target 只表达连接目标

必须完成：

- `AppServerTarget::{LocalNode, HubNode}`
- 帮助文本与参数不再继续扩散旧 transport 语义
- `Embedded/WorkerStdio` 仅保留为内部开发路径

### Phase 2：统一 app-server client facade

目标：

- 把 `agent-app-server-client` 收敛成长期统一 surface

必须完成：

- `AppServerClient::{InProcess, Remote}`
- `AppServerRequestHandle`
- `request_typed`
- `resolve_server_request`
- `reject_server_request`
- `TypedRequestError`

本阶段要求：

- `LocalNode` 只能作为过渡 shim 或 connect helper
- 不能再作为长期公开 variant

### Phase 3：把 gatewayd 对外边界改成正式 remote app-server host

目标：

- 让 `gatewayd` 对外不再表现为 node 私有命令代理

必须完成：

- 正式握手
- request/response correlation
- event stream
- transport disconnect semantics
- `conversation/list` 等 request 的正式远端响应

保留内部职责：

- `conversation registry`
- `worker manager`
- `idle recycle`
- `shared conversation list`

### Phase 4：让 CLI 通过统一 Remote client 连接 local-node

目标：

- CLI 继续只依赖统一 `AppServerClient`
- `local-node` 只是 connect target，不是协议特例

必须完成：

- CLI 连接本地 node 时走 `Remote`
- CLI 初始化、切会话、请求历史、审批响应全部通过统一 client API
- CLI 启动 bootstrap 使用 typed `conversation/history` / `conversation/status`
- CLI 不再依赖同名 `ConversationHistory` / `ConversationStatus` notification 作为首屏初始化来源

补充约束：

- `conversation/list`
- `conversation/status`
- `conversation/history`
- `conversation/historyPage`

这四类同名 notification 只保留“增量同步 / 投影视图刷新”语义。

它们不再承担：

- typed request 的镜像回放
- 首屏 bootstrap 的主来源
- 同一 client 上 request/response 成功后的重复确认

### Phase 5：实现 Direct Mode adapter 接入边界

目标：

- 让 IM/App/Web adapter 也走同一套 remote surface

必须完成：

- adapter 不直连 core
- adapter 不发明新协议
- adapter 通过 `gatewayd` 的正式 remote app-server host 与 worker 交互
- adapter 可以有平台侧 message/outbound 抽象，但 node/client 边界必须复用统一 `AppServerClient` 或其等价 facade，而不是长期保留 direct 专用 node client contract

### Phase 6：实现 Hub Mode host

目标：

- 支持在线设备发现和目标节点切换

必须完成：

- `hubd`
- node 注册与心跳
- 在线设备列表
- target node 路由
- CLI/Web/App 的 hub 连接目标

## 详细 Commit 方案

下面的 commit 顺序是**强约束实施顺序**。除非出现重大阻塞，不应跳步。

### Commit 01

- `docs(architecture): redefine direct mode and codex-aligned transport goals`

内容：

- 修正文档中 `Direct Mode` 的定义
- 明确 `Direct Mode = IM/App/Web -> target node -> worker`
- 明确 `Hub Mode = surface -> hub -> target node -> worker`
- 明确本文档是主实施文档

### Commit 02

- `refactor(cli): freeze target model as local-node and hub-node`

内容：

- 收敛 `AppServerTarget`
- 对外仅保留 target 语义
- 清理公开帮助文本中的旧 transport 词汇

涉及文件：

- [cli/src/app/core/types.rs](/D:/learn/gifti/cloudagent/cli/src/app/core/types.rs:1)
- [cli/src/main.rs](/D:/learn/gifti/cloudagent/cli/src/main.rs:1)

### Commit 03

- `refactor(app-server-client): introduce codex-style typed request facade`

内容：

- 新增 `TypedRequestError`
- 新增统一 `request_typed`
- 统一 `AppServerRequestHandle`
- 把 `resolve_server_request/reject_server_request` 提升成正式 API

涉及文件：

- [crates/agent-app-server-client/src/lib.rs](/D:/learn/gifti/cloudagent/crates/agent-app-server-client/src/lib.rs:1)
- [crates/agent-app-server-client/src/in_process.rs](/D:/learn/gifti/cloudagent/crates/agent-app-server-client/src/in_process.rs:1)
- [crates/agent-app-server-client/src/stdio.rs](/D:/learn/gifti/cloudagent/crates/agent-app-server-client/src/stdio.rs:1)
- [crates/agent-app-server-client/src/local_node.rs](/D:/learn/gifti/cloudagent/crates/agent-app-server-client/src/local_node.rs:1)

### Commit 04

- `refactor(app-server-client): rename local-node transport as transitional remote shim`

内容：

- 明确 `local_node.rs` 是过渡层
- 开始把公开语义收敛到 `Remote`
- 不再把 `LocalNode` 当长期 variant 宣传

### Commit 05

- `feat(gatewayd): expose remote app-server handshake and request host`

内容：

- `gatewayd` 对外具备正式 remote host 边界
- request/response correlation 正式化
- 初始化、断连、错误语义统一

涉及文件：

- [apps/gatewayd/src/node/server.rs](/D:/learn/gifti/cloudagent/apps/gatewayd/src/node/server.rs:1)
- [apps/gatewayd/src/node/command_router.rs](/D:/learn/gifti/cloudagent/apps/gatewayd/src/node/command_router.rs:1)
- [apps/gatewayd/src/node/message_sync.rs](/D:/learn/gifti/cloudagent/apps/gatewayd/src/node/message_sync.rs:1)

### Commit 06

- `refactor(gatewayd): move node-private routing behind remote host boundary`

内容：

- `conversation -> worker` 路由继续保留在 node 内部
- 对外不再暴露 node 私有业务 contract
- 内外边界彻底分清

### Commit 07

- `refactor(cli): connect local-node target through remote app-server client`

内容：

- CLI 连本地 node 改为统一 `Remote`
- CLI 的历史读取、列表请求、审批响应都走统一 facade

涉及文件：

- [cli/src/transport/client.rs](/D:/learn/gifti/cloudagent/cli/src/transport/client.rs:1)
- [cli/src/app/conversation/actions.rs](/D:/learn/gifti/cloudagent/cli/src/app/conversation/actions.rs:1)

### Commit 08

- `test(cli): add startup smoke coverage for remote local-node transport`

内容：

- 覆盖启动
- 覆盖初始化握手
- 覆盖请求历史
- 覆盖切会话
- 覆盖 server request 响应
- 覆盖断连提示

### Commit 09

- `feat(agent-gateway): route direct adapters through unified remote client surface`

内容：

- Direct Mode adapter 不直连 core
- 统一从 `agent-gateway` 走 remote client surface

### Commit 10

- `feat(protocol): reserve hub remote routing requests and online node reads`

内容：

- 预留 `Hub Mode` 的 request/response 面
- 包括在线设备读取和 target node 选择

### Commit 11

- `feat(hubd): add minimal online node registry and routing host`

内容：

- 新增 `hubd`
- 最小在线设备目录
- 最小 target node 路由

### Commit 12

- `refactor(app-server-client): collapse client variants to in-process and remote`

内容：

- 正式把长期 client surface 收敛为：
  - `InProcess`
  - `Remote`

### Commit 13

- `chore(transport): remove obsolete direct local-node protocol terminology`

内容：

- 清理遗留命名
- 删除不再需要的旧语义
- 文档、帮助、测试命名同步

## 验收标准

只有满足以下条件，才算本文档目标完成：

1. CLI、Web、App、IM 都不直接碰 `core`
2. CLI、Web、App、IM 都通过统一 app-server client surface 工作
3. 长期 transport 形态收敛为 `InProcess` 与 `Remote`
4. `local-node` 是 target/deployment 概念，不是长期 client 协议名
5. `gatewayd` 对外是正式 remote app-server host
6. `Direct Mode` 可通过 IM/App/Web 与远程设备上的 agent 稳定交互
7. `Hub Mode` 可查看在线设备并连接目标设备上的 agent
8. 初始化、请求历史、切会话、审批请求、断连、lagged 语义在各 surface 下一致
9. CLI 可以稳定启动并完成基本交互
10. 全量测试与 CI 通过

## 实施要求

后续所有代码提交必须遵守：

1. 不再把 `LocalNode` 当成长期协议终点
2. 不再新增绕过统一 client surface 的快捷实现
3. 每个 commit 只推进一个清晰目标
4. 每个 commit 都要带测试或验收说明
5. 每完成一个阶段，都要回到本文档核对是否偏离

## 一句话总结

本文档的目标不是“先把直连凑出来”，而是：

- **用 Codex 风格的统一传输层，把 Direct Mode 和 Hub Mode 都建立在同一套稳定 app-server surface 上。**
