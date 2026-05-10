# CloudAgent 飞书 / 企业微信接入说明

本文档说明当前 `CloudAgent` 如何通过长连接把本地 `gatewayd` 接到飞书和企业微信平台。

## 当前结论

- 飞书：主路径已切到长连接
- 企业微信：主路径已切到 WebSocket 长连接
- webhook 不再是正式接入路径

也就是说，现在要实现“本机常驻、手机端正常对话”，前提是：

- 本机能主动访问飞书 / 企业微信平台云
- 不需要把本机暴露到公网

## 当前代码位置

平台适配代码位于：

- `D:/learn/gifti/cloudagent/crates/agent-gateway/src/adapter/feishu/`
- `D:/learn/gifti/cloudagent/crates/agent-gateway/src/adapter/wecom/`

node 侧平台管理位于：

- `D:/learn/gifti/cloudagent/apps/gatewayd/src/node/platform_manager.rs`

统一运行时桥接位于：

- `D:/learn/gifti/cloudagent/crates/agent-gateway/src/direct/session.rs`

## 运行模型

当前接法是：

1. 本地电脑运行 `gatewayd`
2. `gatewayd` 内部按平台配置启动对应长连接 runtime
3. 平台云通过已建立的长连接把事件推给本地 runtime
4. `agent-gateway` 通过统一 `AppServerClient::Remote` 把事件送回 `gatewayd`
5. `gatewayd` 再驱动 shared worker 执行

也就是：

- 飞书 / 企业微信手机端 -> 平台云
- 平台云 -> 本地 `gatewayd` 长连接 runtime
- `agent-gateway` -> `gatewayd` remote app-server host
- `gatewayd` -> shared worker

## CLI 管理命令

CLI 通过 node 管理命令控制平台期望状态：

```powershell
cloudagent platform list
cloudagent platform status feishu
cloudagent platform enable feishu
cloudagent platform disable feishu
cloudagent platform enable wecom
cloudagent platform disable wecom
```

这些命令会走传输层请求 `gatewayd`，不会由 CLI 直接写平台状态文件。

## 当前环境变量

### 飞书

- `CLOUDAGENT_FEISHU_APP_ID`
- `CLOUDAGENT_FEISHU_APP_SECRET`
- `CLOUDAGENT_FEISHU_DOMAIN`，可选，默认 `https://open.feishu.cn`

### 企业微信

- `CLOUDAGENT_WECOM_BOT_ID`
- `CLOUDAGENT_WECOM_BOT_SECRET`

## `gatewayd` 当前实际负责的事情

- 读取平台配置
- 维护平台 enable / disable 持久化状态
- 建立 `AppServerClient::Remote`
- 调用 `spawn_runtime(...)`
- 在 node 进程内统一管理平台连接
- 在 node 进程内统一管理 worker 拉起和空闲回收

## 飞书接入步骤

### 1. 平台准备

在飞书开放平台准备：

- 自建应用
- `app_id`
- `app_secret`
- 长连接事件订阅权限
- 消息发送权限

### 2. 本地运行

先设置环境变量，再启动 `gatewayd`，然后执行：

```powershell
cloudagent platform enable feishu
```

如果缺少：

- `CLOUDAGENT_FEISHU_APP_ID`
- `CLOUDAGENT_FEISHU_APP_SECRET`

启用会直接报错，不会静默成功。

### 3. 会话语义

飞书会自动映射成：

- 私聊：`feishu:p2p:<open_id>`
- 群聊：`feishu:chat:<chat_id>`
- 线程：`feishu:chat:<chat_id>:thread:<root_message_id>`

## 企业微信接入步骤

### 1. 平台准备

在企业微信准备：

- 智能机器人
- `bot_id`
- `bot_secret`

### 2. 本地运行

先设置环境变量，再启动 `gatewayd`，然后执行：

```powershell
cloudagent platform enable wecom
```

如果缺少：

- `CLOUDAGENT_WECOM_BOT_ID`
- `CLOUDAGENT_WECOM_BOT_SECRET`

启用会直接报错，不会静默成功。

### 3. 会话语义

企业微信会自动映射成：

- 私聊：`wecom:single:<user_id>`
- 群聊：`wecom:group:<chat_id>`

## 当前能力边界

现在已经具备：

- 飞书长连接入站
- 企业微信 WebSocket 入站
- 文本消息闭环
- 图片 / 文件消息模型入站映射
- `gatewayd` 内平台启停管理

当前还没做：

- 图片 / 文件真实下载
- 飞书卡片按钮闭环
- 更完整的平台交互控件

## 当前推进顺序

1. 先用 `gatewayd + feishu websocket` 跑通无公网闭环
2. 再用 `gatewayd + wecom websocket` 跑通无公网闭环
3. 图片 / 文件真实下载放在下一轮
4. 审批卡片和按钮回调最后补闭环

## 个人微信为什么还不在这一轮

个人微信当前没有像飞书 / 企业微信这样干净的官方长连接 bot 路径。你参考到的项目更像“先连第三方网关，再连个人微信体系”，这会把系统重新带回外部中转依赖，所以现在不建议把它作为第一批正式通道。
