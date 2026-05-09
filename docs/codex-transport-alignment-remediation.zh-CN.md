# Codex Transport 对齐整改清单

本文档用于纠正当前 `cloudagent` 传输层实现与参考 `Codex` 源码之间的偏差。

目标不是继续在现有 `local-node` 自定义协议上修补，而是把本地 node / 远端 node 的接入方式收敛回 **Codex 风格的统一 app-server client surface**。

## 当前结论

当前实现存在以下关键偏差：

1. `cli` 侧的 `local-node` 不是 Codex 风格的 `RemoteAppServerClient`
   现在的 [crates/agent-app-server-client/src/local_node.rs](/D:/learn/gifti/cloudagent/crates/agent-app-server-client/src/local_node.rs:1) 直接通过 TCP 发送 `AppClientCommand`，接收 `AppServerMessage`。

2. `gatewayd` 对外暴露的是自定义 node surface，不是正式 app-server remote surface
   现在的 [apps/gatewayd/src/node/command_router.rs](/D:/learn/gifti/cloudagent/apps/gatewayd/src/node/command_router.rs:1) 在 node 内部自行解析和转发命令。

3. 缺少 Codex remote transport 的握手与请求语义
   当前没有对齐以下能力：
   - `initialize`
   - `initialized`
   - typed request/response
   - `resolve_server_request`
   - `reject_server_request`
   - transport-level disconnect/error semantics

4. `gatewayd` 目前更像“worker 路由代理”，而不是“remote app-server host”

## 参考基线

严格参考以下 Codex 源码：

- [D:\learn\AIbac\JiangFang\codex\codex-rs\app-server-client\src\lib.rs](/D:/learn/AIbac/JiangFang/codex/codex-rs/app-server-client/src/lib.rs:1)
- [D:\learn\AIbac\JiangFang\codex\codex-rs\app-server-client\src\remote.rs](/D:/learn/AIbac/JiangFang/codex/codex-rs/app-server-client/src/remote.rs:1)
- [D:\learn\AIbac\JiangFang\codex\codex-rs\tui\src\lib.rs](/D:/learn/AIbac/JiangFang/codex/codex-rs/tui/src/lib.rs:284)

Codex 的关键原则是：

1. TUI 只依赖统一的 `AppServerClient`
2. transport 只是 `InProcess` / `Remote` 的底层实现
3. remote transport 与 embedded transport 使用同一套上层事件面
4. remote transport 必须完成明确握手，不能裸连后直接发业务命令
5. workspace root 与 app data root 必须分离，不能让日志/会话数据隐式跟随当前启动目录漂移
6. typed request/response 与同名 state notification 必须职责分离

这里尤其要强调第 6 条：

- `conversation/list`
- `conversation/status`
- `conversation/history`
- `conversation/historyPage`

这些能力的**初始化读取 / 显式读取**必须走 typed request/response。

同名 notification 最多只能承担：

- 增量同步
- 重连后的状态投影刷新
- node 内部 registry / UI projection 的更新来源

不能再做的事情：

- typed request 成功后，再顺手补发一份同名 notification 给同一个 client 当初始化面
- CLI/UI 把同名 notification 当成首屏 bootstrap 的主来源
- 让 history / status / list 同时充当“读取面”和“事件面”，形成双源语义

## 必须收敛到的目标形态

长期目标：

- `cli -> AppServerClient::{InProcess|Remote}`
- `Remote` 连接到 `gatewayd` 暴露的正式 app-server remote endpoint
- `gatewayd` 内部再决定 `conversation -> worker`
- `worker` 继续承载 `agent-app-server`

补充的数据根目标：

- `workspace_root` 只负责工作区与工具执行
- `data_root_dir` 负责 conversations / logs / memory
- dev / release 都必须有明确 `data_root_dir` 语义
- `conversation_store_dir` 和 `memory.root_dir` 只是 `data_root_dir` 之下的高级覆盖项

也就是说：

- `cli` 不直接理解 `local node` 内部协议
- `gatewayd` 对外要像真正的 remote app-server
- `local-node` 只是部署形态，不该成为另一套客户端协议名词

## 整改阶段

### Phase A：收口当前错误基线

目标：先修掉当前会直接误导开发和使用者的问题。

已处理：

- `--version` 优先输出 git describe，而不是只输出 workspace version
- 首次 `RequestConversationHistory/Status/Page` 会先确保 conversation 已持久化
- `command handling failed` 现在会带上具体错误文本

### Phase B：定义正式 remote surface

目标：让 `gatewayd` 对外说“正式 app-server remote 协议”，而不是 node 私有命令协议。

必须完成：

1. 为 `agent-app-server-client` 增加正式 `Remote` transport
   - 复用统一 `AppServerEvent`
   - 复用统一 typed request API
   - 明确连接参数结构

2. 为 `gatewayd` 增加正式 remote server transport
   - 接收 `initialize`
   - 要求 `initialized`
   - 使用标准 request/notification envelope
   - 支持 request/response correlation

3. 将当前 `local_node.rs` 降级为过渡层或删除
   - 不再长期保留 node 私有业务协议

### Phase C：把 CLI 拉回 Codex 结构

目标：CLI 只认 target，不认 node 内部实现。

必须完成：

1. `AppServerTarget`
   - `Embedded`
   - `Remote { websocket_url / local_url / auth_token }`

2. `local-node`
   - 只是一个 CLI 便捷 target
   - 其行为是“确保本地 `gatewayd` 在跑，然后通过 `RemoteAppServerClient` 连接它”

3. 去掉现在 `LocalNode` 作为独立 client variant 的长期地位

### Phase D：node 内部职责收敛

目标：`gatewayd` 只对内做 worker 路由，对外做 remote app-server host。

必须完成：

1. 保留：
   - `conversation registry`
   - `worker manager`
   - `idle recycle`
   - `shared conversation list`

2. 删除或重构：
   - 对外裸露的 `AppClientCommandEnvelope` 直通
   - node 自己扮演的业务层 client contract

## 代码映射

### 当前应保留并复用

- [crates/agent-app-server](/D:/learn/gifti/cloudagent/crates/agent-app-server:1)
- [crates/agent-protocol](/D:/learn/gifti/cloudagent/crates/agent-protocol:1)
- [apps/gatewayd/src/node/worker_manager.rs](/D:/learn/gifti/cloudagent/apps/gatewayd/src/node/worker_manager.rs:1)
- [apps/gatewayd/src/node/conversation_registry.rs](/D:/learn/gifti/cloudagent/apps/gatewayd/src/node/conversation_registry.rs:1)

### 当前应重构的入口

- [crates/agent-app-server-client/src/local_node.rs](/D:/learn/gifti/cloudagent/crates/agent-app-server-client/src/local_node.rs:1)
- [apps/gatewayd/src/node/command_router.rs](/D:/learn/gifti/cloudagent/apps/gatewayd/src/node/command_router.rs:1)
- [apps/gatewayd/src/node/server.rs](/D:/learn/gifti/cloudagent/apps/gatewayd/src/node/server.rs:1)

## 推荐提交顺序

1. `docs(architecture): document codex transport alignment remediation`
2. `fix(cli): report build version from git describe`
3. `fix(app-server): hydrate missing conversations before history requests`
4. `refactor(app-server-client): rename local-node transport into transitional remote shim`
5. `feat(gatewayd): add codex-style initialize handshake for resident node transport`
6. `feat(app-server-client): add typed remote app-server client for local gateway`
7. `refactor(cli): connect local-node target through remote app-server client`
8. `refactor(gatewayd): move node-private routing behind remote app-server host boundary`
9. `test(cli): add startup smoke coverage for local-node remote transport`
10. `chore(transport): remove obsolete direct local-node command protocol`

## 验收标准

满足以下条件后，才算真正“对齐 Codex”：

1. `cli` 不再依赖 node 私有命令协议
2. `local-node` 通过统一 remote app-server client 连接 `gatewayd`
3. `gatewayd` 对外具备明确握手和请求/响应语义
4. `embedded` 与 `remote` 共享同一上层事件面
5. 启动、切会话、请求历史、审批请求、断连语义都能在统一 client surface 下成立
6. `conversation/list|status|history|historyPage` 的 typed read 不会再向同一 client 回放重复同名 notification
