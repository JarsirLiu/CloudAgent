# CloudAgent

CloudAgent 现在以干净的 IM 网关架构重建，目标不是把飞书写死，而是先做一套可以持续扩平台的 Hermes 风格骨架。

当前落地的是第一版最小链路：

- `PlatformAdapter`：平台协议层，只负责接入 IM
- `GatewayRuntime`：统一消息编排层
- `SessionKey`：跨平台统一会话映射
- `OpenAiResponder`：模型调用层

飞书当前通过 `websocket` 常连模式接入，不依赖公网 webhook。

## 架构

```text
Feishu WebSocket
  -> FeishuAdapter
  -> InboundMessage
  -> GatewayRuntime
  -> SessionKey
  -> LLM Responder
  -> OutboundMessage
  -> FeishuAdapter
```

这套分层的关键约束是：

- 平台适配器不直接碰模型推理逻辑
- 网关运行时不直接碰飞书 SDK
- 会话键生成与平台协议解耦
- 后续新增钉钉、企业微信、Telegram、Slack 时，只新增各自的 adapter

## 目录

- [apps/gatewayd/src/main.rs](/D:/learn/gifti/cloudagent/apps/gatewayd/src/main.rs)
  只有启动与配置加载
- [crates/agent-gateway/src/platform.rs](/D:/learn/gifti/cloudagent/crates/agent-gateway/src/platform.rs)
  平台适配器抽象
- [crates/agent-gateway/src/runtime.rs](/D:/learn/gifti/cloudagent/crates/agent-gateway/src/runtime.rs)
  统一消息运行时
- [crates/agent-gateway/src/platforms/feishu/adapter.rs](/D:/learn/gifti/cloudagent/crates/agent-gateway/src/platforms/feishu/adapter.rs)
  飞书 websocket 适配器主入口
- [crates/agent-gateway/src/platforms/feishu/admission.rs](/D:/learn/gifti/cloudagent/crates/agent-gateway/src/platforms/feishu/admission.rs)
  飞书消息准入
- [crates/agent-gateway/src/platforms/feishu/normalize.rs](/D:/learn/gifti/cloudagent/crates/agent-gateway/src/platforms/feishu/normalize.rs)
  飞书事件归一化
- [crates/agent-gateway/src/platforms/feishu/outbound.rs](/D:/learn/gifti/cloudagent/crates/agent-gateway/src/platforms/feishu/outbound.rs)
  飞书回复与线程路由
- [crates/agent-gateway/src/message.rs](/D:/learn/gifti/cloudagent/crates/agent-gateway/src/message.rs)
  标准化消息模型
- [crates/agent-gateway/src/session.rs](/D:/learn/gifti/cloudagent/crates/agent-gateway/src/session.rs)
  会话键映射
- [crates/agent-gateway/src/openai.rs](/D:/learn/gifti/cloudagent/crates/agent-gateway/src/openai.rs)
  OpenAI 兼容模型调用

## 配置

示例配置见 [configs/config.toml.example](/D:/learn/gifti/cloudagent/configs/config.toml.example)。

最少需要：

- `feishu.app_id`
- `feishu.app_secret`
- `llm.api_key`

可选：

- `feishu.verification_token`
- `feishu.encrypt_key`
- `feishu.group_only_mentioned`
- `llm.base_url`
- `llm.model`

配置读取顺序：

1. `CLOUDAGENT_CONFIG` 指向的配置文件
2. `~/.cloudagent/config.toml`
3. `./.cloudagent/config.toml`
4. `./configs/config.toml`
5. 环境变量覆盖文件配置

平台凭据文件路径和 CLI `/gateway` 保持一致：

- 开发模式默认走工作区 `data_root_dir = <workspace>/data`，平台配置写到 `<workspace>/platform/<name>.json`
- 如果 `data_root_dir` 改成 `<workspace>/.cloudagent-dev` 这类目录，则平台配置写到 `<workspace>/.cloudagent-dev/platform/<name>.json`
- 发行模式默认走 `~/.cloudagent/data`，平台配置写到 `~/.cloudagent/platform/<name>.json`

## 启动

```bash
cargo run -p gatewayd
```

或者显式指定配置文件：

```bash
cargo run -p gatewayd -- D:/learn/gifti/cloudagent/configs/config.toml.example
```

## 飞书应用设置

建议使用以下模式：

- 事件订阅：`WebSocket Client`
- 订阅事件：`im.message.receive_v1`
- 机器人权限：发送消息、接收消息

如果群聊里不想被每条消息触发，保持：

```toml
[feishu]
group_only_mentioned = true
```

## 下一步扩平台

后续新增一个 IM 平台时，原则上只需要：

1. 新建一个 `XxxAdapter` 并实现 `PlatformAdapter`
2. 把平台原始事件转换成 `InboundMessage`
3. 实现 `send_message`
4. 在启动层注册该 adapter

这样核心 runtime、session 和 llm 层都不用重写。
