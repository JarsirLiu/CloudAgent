# 开发说明

## 当前目标

构建 `cloudagent`，把它做成一个模块化的 Rust workspace，用于承载一个本地运行或服务器常驻运行的 agent。

这个项目不是传统意义上的监控平台。服务器巡检、日志分析、服务检查、远程通知、定时唤醒，都只是 agent 的能力。

当前产品目标是：

- 在服务器或本地机器上运行一个常驻 agent
- 让 agent 通过工具检查和操作系统环境
- 让 agent 能为未来的自己创建计划任务
- 当计划任务到期时自动唤醒 agent 继续执行
- 后续支持通过手机消息通道进行远程对话
- 保持仓库结构强模块化、workspace 优先，并适合长期演进

## 架构方向

仓库采用 Rust workspace 结构，顶层只保留少量高层目录，核心代码放在 `crates/` 中。

顶层结构：

```text
apps/      可执行程序入口
crates/    可复用 Rust crate
web/       未来的 Web 管理端或前端
configs/   配置文件
docs/      架构和设计文档
tests/     集成测试与工作区级测试
data/      本地开发运行数据
```

## 核心设计思想

系统的中心是 agent 本身。

这意味着：

- 监控不是整个项目的主架构
- 调度不是整个项目的主架构
- 消息通道也不是整个项目的主架构

真正的主线是：

- agent 是核心
- tools 是 agent 可以调用的能力
- scheduler 负责在未来某个时间重新唤醒 agent
- gateway 负责 agent 与远程用户之间的消息连接

## 各个 crate 的职责边界

### `agent-core`

负责 agent 的核心概念和编排契约。

典型职责：

- conversation 模型
- message / turn 模型
- task / plan 模型
- context 拼装
- tool call 抽象
- agent 核心编排接口

它应该描述 agent 如何思考、如何推进任务，但不应该直接持有过多具体基础设施实现。

### `agent-runtime`

负责 agent 的执行生命周期。

典型职责：

- 运行 agent conversation
- 驱动执行循环
- 处理中断、取消、超时
- 把 scheduler 的唤醒事件转成一次真正的 agent 执行

### `agent-tools`

负责 agent 的工具系统。

典型职责：

- 工具定义
- 工具注册表
- shell / file / http / system / service / log 等工具
- 创建计划任务的工具
- 通知发送工具

### `agent-memory`

负责面向 agent 的记忆抽象与记忆逻辑。

典型职责：

- 会话记忆
- 任务记忆
- 唤醒上下文快照
- 用户和环境相关记忆

### `agent-gateway`

负责远程交互入口和消息路由抽象。

典型职责：

- 远程消息的输入与输出
- 会话路由
- 远程用户与本地 agent 执行之间的映射关系

### `agent-model-provider`

负责模型 provider 协议适配。

典型职责：

- `ChatModel` 的具体实现
- OpenAI-compatible / Responses API / Realtime 等模型协议适配
- provider 配置映射
- 模型流式事件到 core 抽象的转换

### `agent-scheduler`

负责延迟任务与周期任务。

典型职责：

- 任务调度
- 周期计划
- 唤醒触发
- 调度任务的重试策略

### `storage`

负责业务级持久化。

典型职责：

- schedule 的 repository
- 执行历史的 repository
- 记忆与状态对象的 repository

### `config`

负责应用和工作区配置。

### `infra-*`

负责具体基础设施适配。

当前拆分为：

- `infra-shell`
- `infra-http`
- `infra-ssh`
- `infra-store`

这些 crate 应只提供底层接入能力，而不是承载业务编排。

### `shared`

负责跨模块共享的轻量类型与工具。

## 当前仓库结构

当前 `crates/` 目录：

```text
crates/
├─ agent-core/
├─ agent-model-provider/
├─ agent-runtime/
├─ agent-tools/
├─ agent-memory/
├─ agent-gateway/
├─ agent-scheduler/
├─ storage/
├─ config/
├─ infra-http/
├─ infra-shell/
├─ infra-ssh/
├─ infra-store/
└─ shared/
```

## 设计规则

1. 根目录保持干净。
2. 重要子系统优先用 crate 边界隔离。
3. 不要让基础设施细节污染 `agent-core`。
4. 把服务器检查视为 tool，不要把整个项目做成监控平台。
5. 调度和远程消息通道都要作为一等能力，但保持独立边界。
6. 优先先定义接口，再逐步补具体实现。

## 当前阶段的下一步

1. 在 `agent-core` 中定义最小核心模型和 trait。
2. 明确 `agent-core` 与 `agent-runtime` 之间的执行契约。
3. 在 `agent-tools` 中定义工具注册和调用抽象。
4. 明确 `agent-memory` 与 `storage` 之间的 repository 抽象。
5. 定义 `agent-scheduler` 与 `agent-runtime` 之间的唤醒载荷。
6. 为未来的手机/Web 接入定义 `agent-gateway` 的消息抽象。
