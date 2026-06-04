# Node-Worker 架构总览

## 文档定位

这是 `node-worker` 改造的主文档。

阅读顺序建议：

1. 先看本文档
2. 再看 [`docs/node-worker-current-status.zh-CN.md`](D:/learn/gifti/cloudagent/docs/node-worker-current-status.zh-CN.md)
3. 再看 [`docs/node-worker-phase1-checklist.zh-CN.md`](D:/learn/gifti/cloudagent/docs/node-worker-phase1-checklist.zh-CN.md)
4. 最后看 [`docs/node-worker-protocol-draft.zh-CN.md`](D:/learn/gifti/cloudagent/docs/node-worker-protocol-draft.zh-CN.md)

本文负责：

- 架构目标
- 职责边界
- 长期稳定运行约束
- 关键抽象
- 可行性补充约束

不负责：

- 逐模块代码清单
- 具体 Rust 接口细节
- 当前代码已经落地到哪一步

## 目标

把当前偏“scoped worker”的实现，改造成“`node` 常驻调度 + `agentd` 作为 source worker controller”的模型。

本次改造解决的核心问题：

- 多目录启动 `cli` 时，旧 `cwd` 或旧 workspace 上下文串入新会话
- worker 身份和目录、权限、连接生命周期耦合过深
- 无法把 `cli`、`web`、`IM` 统一抽象为同一套 worker 管理模型

改造后的目标不是：

- 一会话一 worker
- 一目录一 worker
- 权限变化时立即切换到另一个 worker 接力

改造后的目标是：

- `node` 统一管理 `cli`、`web`、`IM` 的 source domain
- `agentd` 作为长驻控制进程，管理所属 source domain 下的多会话
- `workspace`、`session`、permission、`cwd` 都作为独立抽象建模
- 每个 turn 显式携带 execution context

## 现状判断

当前设计的主要问题不在 `agent-core` 的推理能力，而在 `node + agentd` 的作用域模型。

现状特征：

- `node` 负责拉起和复用 worker
- worker scope 仍然和连接、目录或会话语义混杂
- `node` 启动时会读取进程级 `current_dir`
- 本地 `cli` 连接本地 `node` 时，目录与 node 身份、data root、worker 复用策略之间存在隐式耦合

因此，“新目录会话拿到旧目录上下文”是架构问题，不是单点 bug。

当前已落地实现与测试结果，统一看：

- [`docs/node-worker-current-status.zh-CN.md`](D:/learn/gifti/cloudagent/docs/node-worker-current-status.zh-CN.md)

## 目标架构

### `node`

职责：

- 常驻 supervisor
- transport host
- source domain registry
- session registry
- workspace registry
- worker registry / worker pool
- worker health / restart / circuit breaker
- turn 调度与执行绑定
- 状态持久化与恢复

不负责：

- 直接持有具体 conversation 的运行时执行细节
- 把 `cwd` 当作 worker 身份

### `agentd`

职责：

- source worker controller
- 多 session / 多 conversation 管理
- conversation runtime 承载
- turn queue
- tool runner dispatcher
- event streaming
- 故障与资源状态上报

不负责：

- 自行决定全局 worker 拓扑
- 把 session 生命周期等同于进程生命周期

### `agent-core`

职责保持不变，但接口需要更明确：

- 接受显式 execution context
- 避免隐式依赖进程级目录状态
- 将 permission / workspace / cwd 作为输入，而不是环境猜测

## 关键抽象

### `source domain`

入口来源维度，例如：

- `cli`
- `web`
- `wecom`
- `weixin`

### `worker instance`

某个 source domain 下的一个长驻 `agentd` 实例。

建议保留两层概念：

- 逻辑层：`cli` 是一个 domain
- 物理层：`cli` 可对应一个或多个 worker instance

### `workspace`

定义逻辑工作区：

- `workspace_id`
- `root_path`
- repo 指纹
- config/data root

### `session`

定义一个入口连接会话：

- `session_id`
- `source_domain`
- 默认 `workspace_id`
- 默认 permission profile
- 会话元信息

### `execution context`

定义某个 turn 的运行时快照：

- `session_id`
- `conversation_id`
- `workspace_id`
- `cwd`
- permission profile
- env overlay
- model profile
- tool availability

## 调度原则

默认规则：

- `node` 按 source domain 维护 worker pool
- session 默认绑定到某个 worker instance
- 同一 session 内多个 turn 默认落到同一 worker instance
- conversation 级别保持串行
- 不同 conversation 可以并行

worker 选择建议参考：

- 当前活跃 turn 数
- 当前资源占用
- 是否已有相同 workspace 的热状态
- 是否需要更强隔离

### 长期运行补充约束

第一阶段可以先采用“每个 source domain 一个共享 worker instance”，但这不能作为长期最终形态的全部规则。

必须补充以下晋升策略：

- 长任务超过阈值时，session 可以提升到专属 worker instance
- 单实例会话数超过阈值时，node 必须允许扩成 pool
- 单实例内存或事件积压超过阈值时，node 必须进入限流、拒绝新绑定或隔离重建

最低要求：

- 为 `cli` 域定义并发上限
- 为单 worker instance 定义软上限和硬上限
- 为“共享实例 -> 专属实例”的升级条件建模

否则第一阶段方案可落地，但不够支撑长期稳定运行。

## 权限策略

permission 不作为 worker 身份的一部分。

规则：

- permission profile 挂在 session 上
- turn 开始时快照成 execution context
- 本 turn 内权限固定
- 用户调整权限后，从下一个 turn 生效
- 不做运行中的 worker 热迁移

如果未来需要更强隔离：

- 在 turn 边界把该 turn 调度到隔离 execution runner
- 必要时调度到专属 worker instance
- conversation identity 保持不变

## 关键补充约束

下面五项不是可选优化，而是把方案做成长期可运行架构的必要条件。

### 1. Session 所有权与重连规则

如果 `session_id` 允许由 surface 传入，node 必须定义重连所有权规则。

至少要明确：

- 同一 `session_id` 再次连接时，是接管还是拒绝
- 断线重连是否需要 resume token
- 原连接仍存活时，新连接如何处理
- IM / web / cli 三类来源是否允许共享同名 `session_id`

推荐规则：

- `session_id` 只在同一 `source domain` 内可重连
- node 为每个 session 分配 resume token
- 没有有效 resume token 的重连不能直接接管已有 session

### 2. Workspace Identity 与 Execution Profile 分离

`workspace` 只表示逻辑工作区，不应该同时承载权限、环境和运行配置身份。

必须区分：

- `workspace identity`
  - `workspace_id`
  - `root_path`
  - repo 指纹
- `execution profile`
  - permission mode
  - env overlay
  - model profile
  - data root / config root

原因：

- 同一路径下可能存在多个不同执行配置
- 如果只按路径复用 runtime，容易把错误权限或错误配置复用到错误会话

### 3. Conversation Runtime 生命周期

`agentd` 内的 conversation runtime 不能只定义“创建和复用”，还必须定义失效与重建规则。

至少要覆盖：

- 首次创建
- 闲置淘汰
- worker 重启后重建
- workspace 配置变化后失效
- execution profile 变化后重建
- 技能目录或工具暴露变化后的刷新策略

如果没有这套生命周期规则，长驻 worker 最终会退化成脏缓存容器。

### 4. Command Context 合法性校验

命令级 context 可以覆盖 session 默认值，但 node 不能无条件接受。

必须校验：

- `workspace_root` 是否属于允许的 workspace 集合
- `workspace_id` 与 `workspace_root` 是否匹配
- `cwd` 是否在允许边界内
- permission mode 是否允许被当前来源覆盖
- 当前 command 是否允许切换 execution profile

推荐规则：

- session 默认只能在自身 workspace 内改变 `cwd`
- 切换到新 workspace 必须显式创建或重绑定 session
- 更高权限 profile 只能在 turn 边界生效

### 5. Worker 隔离与退避策略

共享 worker instance 发生故障时，不能只做“进程重启然后继续”。

必须定义：

- 连续故障次数阈值
- 熔断窗口
- 故障 session 是否需要拆离到专属实例
- 是否允许对单个污染 session 做隔离摘除

否则一个坏会话会长期污染整个 `cli` 域共享 worker。

## 代码组织约束

node-worker 改造必须同时改代码组织方式。

原则：

- `main.rs` 只做启动组装
- manager 只做协调，不做万能对象
- registry、dispatcher、process host、queue 分层
- 单文件持续超过约 500 行默认触发拆分检查
- 优先按业务对象拆模块，不按杂项工具函数堆文件

推荐拆分主线：

- `node`
  - `transport / routing / session / workspace / source / worker / runtime`
- `agentd`
  - `controller / session / workspace / execution / conversation / transport`

## 模块改造范围

### 第一优先级：`apps/agentd`

要做的事：

- 从单 worker host 转成 source worker controller
- 增加 session registry
- 增加 workspace registry
- 增加 conversation runtime table
- 增加 turn queue / interrupt / resume 管理
- 拆分 execution runner 与主控制器

这是本次改造主战场。

### 第二优先级：`apps/node`

要做的事：

- 用 source domain / worker instance 替代当前 `worker_scope_key` 语义
- 建立 worker registry 和 pool 调度
- 建立 session -> worker instance 绑定表
- 建立 workspace registry
- 在请求路由时显式传递 execution context
- 新增 worker 健康与重启策略

### 第三优先级：`agent-app-server` / `agent-app-server-client` / 协议层

要做的事：

- 扩充请求和事件中的上下文字段
- 让 surface、node、worker 之间传递 `session_id`、`workspace_id`、`cwd`、permission profile
- 避免通过连接状态猜测上下文

### 第四优先级：`agent-core`

要做的事：

- 明确从外部接收 execution context
- 清理隐式读取目录或权限的路径
- 保持核心推理与工具执行语义稳定

## 分阶段文档说明

本文不再承载逐阶段细清单。

对应关系：

- 第一阶段实施清单见 [`docs/node-worker-phase1-checklist.zh-CN.md`](D:/learn/gifti/cloudagent/docs/node-worker-phase1-checklist.zh-CN.md)
- 协议与 Rust 骨架见 [`docs/node-worker-protocol-draft.zh-CN.md`](D:/learn/gifti/cloudagent/docs/node-worker-protocol-draft.zh-CN.md)

## 兼容与风险

主要风险：

- 当前 `/session` 和 conversation store 逻辑可能依赖旧的 scope 假设
- CLI 启动时的本地 node 发现逻辑可能仍混入 workspace 级推导
- worker 内已有状态对象可能默认按单会话设计
- 平台 runtime 与 worker 事件流可能假设“一连接一运行时”

建议控制风险的方法：

- 先保留旧协议兼容层
- 增量引入新字段
- 优先修正上下文显式传递
- 在 `cli` 场景跑通后再推广到 IM / hub

## 验收标准

至少满足以下行为：

- 在不同目录分别启动多个 `cli`，每个新会话都拿到正确 `workspace_root` 和 `cwd`
- 同一目录多个会话可以并行，不互串上下文
- 同一 worker instance 可以承载多个目录下的多个会话
- 用户调整权限后，只影响后续 turn，不打断当前 turn
- worker 崩溃后，node 能重建绑定并恢复可用状态

## 结论

这次改造的主战场是 `apps/agentd`，第二战场是 `apps/node`，`agent-core` 只做配合性接口收敛。

如果目标是让 `cli worker` 拥有接近 Codex CLI 的多会话、多目录、多并发管理能力，那么必须把 `agentd` 从“单次执行 worker”升级为“长驻 source worker controller”。

方向上，这套方案是可行的。

但要达到“长期稳定运行、代码干净、职责解耦”的标准，必须同时满足：

- session 重连所有权规则明确
- workspace identity 与 execution profile 分离
- conversation runtime 生命周期建模
- command context 合法性校验
- 共享 worker 的隔离与退避策略
