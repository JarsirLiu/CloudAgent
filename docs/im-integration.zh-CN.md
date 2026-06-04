# CloudAgent IM 接入架构

本文档描述当前仓库里 IM 平台接入的正式结构与运行边界。

注意：

- 当前 IM 仍然是“通过本地 node 做 remote relay”
- 本文不是 hub 设计文档
- 本文也不覆盖本地 `node-worker` 改造细节

相关文档：

- [`docs/node-worker-rebuild-plan.zh-CN.md`](D:/learn/gifti/cloudagent/docs/node-worker-rebuild-plan.zh-CN.md)
- [`docs/node-worker-current-status.zh-CN.md`](D:/learn/gifti/cloudagent/docs/node-worker-current-status.zh-CN.md)

## 当前角色

- `cloudagent`
  产品入口，负责 `start/status/stop/cli/platform/node`
- `cli`
  终端 surface，只负责和本地 `node` 交互
- `node`
  本机常驻进程，负责平台 runtime、source worker 生命周期、会话状态与远程 app-server host
- `agentd`
  source worker 进程，负责所属入口来源下的会话控制与执行编排

## 运行链路

当前 IM 接入统一走：

```text
IM Platform
  -> agent-gateway adapter
  -> AppServerClient::Remote
  -> node
  -> worker(agentd)
  -> core
```

也就是说：

- 平台适配层不直连 `agent-core`
- 平台适配层不自己发明 node/client 协议
- 所有平台都通过统一的 remote app-server surface 回到 `node`

## 代码位置

平台适配代码统一位于：

- `crates/agent-gateway/src/adapter/`

当前目录形态：

```text
crates/agent-gateway/src/adapter/
  feishu/
    mod.rs
    config.rs
    client.rs
    admission.rs
    normalize.rs
    outbound.rs
    render.rs
    formatter.rs
    reply_context.rs
    runtime.rs
    types.rs
  wecom/
    mod.rs
    config.rs
    client.rs
    inbound.rs
    outbound.rs
    runtime.rs
  weixin/
    mod.rs
    config.rs
    client.rs
    inbound.rs
    outbound.rs
    runtime.rs
```

## 职责边界

### `config.rs`

负责：

- 平台配置模型
- 必填字段校验
- 长连接接入参数

不负责：

- 建连
- 消息编排

### `client.rs`

负责：

- 平台 SDK / HTTP / WebSocket 初始化
- 鉴权
- 长连接收发
- 平台原始事件输入
- 平台消息发送

不负责：

- node 协议
- agent 会话逻辑

### `runtime.rs`

负责：

- 把平台 client 接到 `AppServerClient::Remote`
- 把平台入站事件翻译成提交给 node 的统一行为
- 处理平台侧审批、回调、控制流桥接

### `inbound.rs` / `admission.rs` / `normalize.rs`

负责：

- 平台原始消息解码
- 准入判定
- 会话 key 生成
- 平台消息到统一消息模型的转换

### `outbound.rs` / `render.rs` / `formatter.rs`

负责：

- 统一事件到平台出站模型的转换
- 平台富文本、卡片、文本模式渲染

## 会话 key

当前约定：

- 飞书私聊：`feishu:p2p:<open_id>`
- 飞书群聊：`feishu:chat:<chat_id>`
- 飞书线程：`feishu:chat:<chat_id>:thread:<root_message_id>`
- 企业微信私聊：`wecom:single:<user_id>`
- 企业微信群：`wecom:group:<chat_id>`

## 当前支持

- 飞书：WebSocket 常连接入
- 企业微信：WebSocket 长连接接入
- 个人微信：仍在适配链路中，但不作为当前正式主通道

## 平台管理

平台启停由 `node` 管理，不由 CLI 直接写状态文件。

常用命令：

```powershell
cloudagent platform list
cloudagent platform status feishu
cloudagent platform enable feishu
cloudagent platform disable feishu
cloudagent platform enable wecom
cloudagent platform disable wecom
```

## 当前边界

当前已经具备：

- 平台长连接入站
- 统一 remote app-server 回传
- `node` 内的平台启停与 source worker 协调
- 文本消息主链路

当前仍未完全覆盖：

- 图片 / 文件真实下载与回传闭环
- 更完整的平台卡片交互
- 更统一的多平台 media 能力抽象
