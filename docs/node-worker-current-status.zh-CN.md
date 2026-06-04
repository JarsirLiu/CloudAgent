# Node-Worker 当前落地状态

## 文档定位

本文档只回答两件事：

- 当前本地 `node-worker` 改造已经实际做到哪里
- 目前哪些结论已经由代码和测试验证

本文不再重复长期架构设计和协议草案，相关内容统一看：

- [`docs/node-worker-rebuild-plan.zh-CN.md`](D:/learn/gifti/cloudagent/docs/node-worker-rebuild-plan.zh-CN.md)
- [`docs/node-worker-protocol-draft.zh-CN.md`](D:/learn/gifti/cloudagent/docs/node-worker-protocol-draft.zh-CN.md)
- [`docs/node-worker-phase1-checklist.zh-CN.md`](D:/learn/gifti/cloudagent/docs/node-worker-phase1-checklist.zh-CN.md)

## 当前范围

当前只覆盖本地 `node-worker` 架构收敛：

- 本地常驻 `node`
- `cli -> node -> worker(agentd)` 链路
- IM 仍然走当前 remote relay 模式

当前明确不包含：

- hub 设计
- 多设备互连
- 跨 node 调度

## 已落地的核心结论

### 1. CLI 会显式发送会话上下文

本地 `cli` 连接 `node` 时，会在 transport initialize 里发送：

- `source_domain`
- `workspace_root`
- `cwd`
- `permission_mode`
- `data_root_dir`

代码位置：

- [`cli/src/transport/client.rs`](D:/learn/gifti/cloudagent/cli/src/transport/client.rs)
- [`crates/agent-app-server-client/src/remote.rs`](D:/learn/gifti/cloudagent/crates/agent-app-server-client/src/remote.rs)

这一步解决的是：

- `node` 不再只能靠 client name 或 node 自己启动目录去猜本次 CLI 会话属于哪个工作区

### 2. node 会把 session context 写入 session state

`node` 在 initialize 阶段会：

- 根据 `session_context.source_domain` 解析来源
- 把 `workspace_root/cwd/permission/data_root_dir` 写入 `NodeSessionState`
- 基于来源和 workspace 重新计算 worker scope

代码位置：

- [`apps/node/src/node/server.rs`](D:/learn/gifti/cloudagent/apps/node/src/node/server.rs)
- [`apps/node/src/node/session_state.rs`](D:/learn/gifti/cloudagent/apps/node/src/node/session_state.rs)

### 3. worker scope 已改成“来源策略 + workspace 输入”

当前规则已经明确：

- `local:*` 来源按 workspace scope 派生 worker key
- `im:*` / `remote:*` 来源按 domain scope 共享 worker key

代码位置：

- [`apps/node/src/node/source.rs`](D:/learn/gifti/cloudagent/apps/node/src/node/source.rs)

这一步解决的是：

- 不再把“连接字符串”或“node 启动目录”直接当成 worker 身份

补充说明：

- 代码和状态输出里暂时仍保留 `worker_scope_key` 这个兼容字段名
- 现在更准确的理解应该是“派生出来的 worker instance key”
- 它已经不是“目录本身”或“连接本身”

### 4. 后续命令和 typed request 也会带默认执行上下文

当前不仅 initialize 会发上下文，后续：

- command
- typed request

也会自动带默认 `CommandExecutionContext`。

代码位置：

- [`crates/agent-app-server-client/src/remote.rs`](D:/learn/gifti/cloudagent/crates/agent-app-server-client/src/remote.rs)
- [`apps/node/src/node/command_router.rs`](D:/learn/gifti/cloudagent/apps/node/src/node/command_router.rs)

这一步解决的是：

- 不再只靠“初始化时曾经连接到哪个目录”维持状态
- 共享 worker 也能按请求上下文切回正确 runtime

### 5. agentd 已按 workspace/data root 选 runtime

`agentd` 当前已经支持按上下文选择 runtime，关键输入是：

- `workspace_root`
- `data_root_dir`

代码位置：

- [`apps/agentd/src/runtime_manager.rs`](D:/learn/gifti/cloudagent/apps/agentd/src/runtime_manager.rs)
- [`crates/agent-app-server/src/lib.rs`](D:/learn/gifti/cloudagent/crates/agent-app-server/src/lib.rs)

## 已验证的自动测试

目前和本次问题最相关的自动测试包括：

- CLI 层：
  - 不同 workspace 的本地连接会发送不同 `session_context`
  - `create_local_node_client(...)` 会同时校验 workspace context 和 data root
- node 层：
  - initialize 会把 session context 写入 session state
  - 两个不同 workspace 的本地 CLI session 会得到不同 worker scope
  - 共享 worker 会按 request context 切换 runtime
- remote client 层：
  - initialize 会发送 session context
  - 后续 command 会发送默认 command context
  - typed request 会自动注入 `_context`
- app server / agentd 层：
  - typed request runtime selection 会优先使用 request context
  - in-process runtime 也会按 command context 切换

## 最近一次联合回归结果

最近一次与本地 `node-worker` 改造直接相关的联合回归：

```text
cargo test -p cli -p node -p agent-app-server-client -p agent-app-server -- --nocapture
```

结果：

- `agent-app-server`: 36 passed
- `agent-app-server-client`: 16 passed
- `cli`: 263 passed, 1 ignored
- `node`: 61 passed

其中 `ignored` 的是手工 smoke test，不属于自动回归失败。

## 当前仍保留的边界

### 1. 仍然保留手工 smoke test

当前有一条手工 smoke test：

- [`cli/src/transport/client.rs`](D:/learn/gifti/cloudagent/cli/src/transport/client.rs)

它用于：

- 真实启动预构建 `node/agentd`
- 走真实 TCP 连接
- 做一次启动期 typed read 验证

它被保留为 `ignored`，原因是：

- 依赖本机二进制和运行环境
- 不适合作为默认 CI 回归

### 2. hub 仍未开始

当前文档和实现都应继续遵守这个边界：

- 不提前设计跨设备 hub 行为
- 不把当前 IM relay 误写成 hub

### 3. 本地 node 发现策略还没彻底重做

当前 `workspace_scoped_node_port()` 仍然存在，含义是：

- 本地开发阶段的 node 发现策略

它不再应该被理解为：

- worker scope 策略
- execution context 身份

## 下一步建议

后续如果继续推进本地 `node-worker` 架构，建议优先顺序是：

1. 继续保持文档和代码边界一致
2. 逐步把“开发期发现策略”和“长期常驻 node 拓扑”分开
3. 等本地模型完全稳定后，再单独进入 hub 方案设计
