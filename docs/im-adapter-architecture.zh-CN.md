# CloudAgent IM 平台适配骨架与接入方案

本文档约束 CloudAgent 在 `IM / App / Web` 平台适配层的目录与职责边界。

## 目标

平台适配层必须满足：

- 不直连 `agent-core`
- 不发明独立 node/client 协议
- 统一通过 `agent-gateway -> AppServerClient::Remote -> gatewayd`
- 平台差异只停留在 adapter / platform client / message mapping 层

## 目录建议

当前建议把 IM 适配代码放在：

- `crates/agent-gateway/src/adapter/`

推荐结构：

```text
crates/agent-gateway/src/adapter/
  mod.rs
  feishu/
    mod.rs
    config.rs
    client.rs
    inbound.rs
    outbound.rs
  wecom/
    mod.rs
    config.rs
    client.rs
    inbound.rs
    outbound.rs
```

后续若平台代码继续增长，可再增加：

- `cards.rs`
- `media.rs`
- `stream.rs`

## 文件职责

### `config.rs`

负责：

- 平台配置模型
- 必填字段校验
- 长连接接入参数

不负责：

- 真正建立连接
- 消息解析

### `client.rs`

负责：

- SDK 初始化
- 平台鉴权
- 长连接启动
- 心跳、重连、错误恢复
- 平台原始事件输入
- 平台消息发送

不负责：

- `GatewayMessage` / `GatewayOutbound` 映射
- 业务会话逻辑

### `inbound.rs`

负责：

- 平台原始消息抽象
- 平台事件到 `GatewayMessage` 的转换
- 会话 key 生成规则

### `outbound.rs`

负责：

- `GatewayOutbound` 到平台发送模型的转换
- 文本、卡片、审批、文件等出站类型分流

## 会话 key 约定

推荐：

- 飞书私聊：`feishu:p2p:<open_id>`
- 飞书群聊：`feishu:chat:<chat_id>`
- 飞书 thread：`feishu:chat:<chat_id>:thread:<root_message_id>`
- 企业微信私聊：`wecom:single:<user_id>`
- 企业微信群：`wecom:group:<chat_id>`

这些 key 由 `inbound.rs` 统一生成。

## 首批平台顺序

1. `feishu`
2. `wecom`
3. `weixin`

说明：

- 飞书最适合作为第一条正式 IM 通道
- 企业微信次之
- 个人微信应单独作为更高风险适配放在后续阶段

## 当前实现状态

本轮已完成：

- `feishu/` 与 `wecom/` 目录骨架
- 平台 `config / client / inbound / outbound` 分责
- `DirectGatewaySession::run_until_closed()` 统一运行时桥接
- 飞书长连接入站、消息回发 HTTP client
- 企业微信 WebSocket 入站、消息回发
- 平台入站消息统一转换到 `GatewayMessage`
- `GatewayOutbound` 统一转换到平台出站模型

本轮仍未完成：

- 飞书图片 / 文件真实下载
- 平台卡片按钮审批闭环
- 更完整的平台交互控件
