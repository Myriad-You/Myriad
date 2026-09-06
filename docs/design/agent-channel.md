# Agent 运输适配层

外部聊天软件走进站点已经存在的办事流水线，结果回到原来那条聊天。
不是另造一个 bot 大脑，也不是把 Telegram / QQ 的线程 id 漏进 Planner。

通道锁死 **办事（Work）**。聊天（Chat）仍是面板和实时语音的路径，不作为第一版外部 Channel。

代码入口：

- 请求形状：`backend/src/api/agent/types.rs`（`ProcessRequest` / `ProcessContext`）
- 开工函数：`backend/src/api/agent/process.rs`（`start_process_run`）
- 办事分流：`backend/src/services/agent/process_work.rs`
- 流式事件：`backend/src/services/agent/types.rs`（`AgentProgressEvent`）
- 车道：`backend/src/services/agent/queue.rs`（`user:{id}:session:{sid}`）
- 现有 Discord 路由是数据平台 OAuth，不是 Bot：`backend/src/api/discord.rs`

术语见 `CONTEXT.md` 的办事 / 聊天 / 设定。不要把 `PlannerStatus::Chat` 当成产品聊天档。

## 管道

```
平台事件
  → pairing（平台账号 → Myriad Claims）
  → session grammar（平台对话 → ses_…）
  → ProcessRequest { input, context.mode = work, session_id }
  → start_process_run
  → AgentProgressEvent / ApiResponse
  → 通道按 capabilities() 投递
```

`start_process_run` 负责：核对用户、Chat/Work 分叉、保证会话、车道排队、历史、额度、开 run。
通道只负责进出。浏览器和 Agora 语音已经共用这个函数，只换运输。
`backend/src/api/agent/process.rs` 注释：Browser and realtime voice enter the same run lifecycle… Only their event transports differ.

语音是 Chat-only 先例，云端不能自选用户、会话、模型或 Work。
`backend/src/api/speech_conversation.rs`

## 三层不要混

| 层 | 人看见什么 | 代码 | 寿命 |
| --- | --- | --- | --- |
| 会话 | 这一段办事对话 | `agent_sessions`，id 如 `ses_…` | 跨很多句 |
| 一轮 | 这一句正在被处理 | `start_process_run` 开出的 run，有 `runId` | 一句从进到出 |
| 车道 | 同一段对话不要两句同时抢 | `LaneQueue::make_lane_key` | 跟会话走 |

新开一个平台对话 = 新会话。同一对话里下一句 = 旧会话上的新一轮。
`ensure_session` 发现手里的 id 模式对不上，会另开一个，不混历史。
`backend/src/api/agent/sessions.rs`

Chat 插话可以替换上一句还没说完的聊天，**不能取消正在跑的办事**。
办事插话换题走 `POST /api/agent/session/interrupt`，会取消未完成任务。
`backend/src/services/agent/turn.rs`

pairing 和会话是两件事：这个 QQ 号是谁；这个人在这个窗口里对应哪一段 `ses_…`。

## 通道最小接口

```text
ChannelAdapter
  id / capabilities()
  ingest(platform event) -> ProcessRequest + Claims
  session_key(platform conversation) -> session_id
  deliver(AgentProgressEvent | ApiResponse) -> receipt
  ack_policy / typing / split
```

`capabilities()` 必须能表达：

- 入站：text、media、callback/button
- 出站：final text、markdown、image/document、edit / streaming draft
- 交互：clarification、confirmation、waiting_for_input
- 明确不支持：`frontendAction`、performance、outfit、page interact

未声明的能力，core 按不支持处理。碰到确认或 `frontendAction` 不能静默当成功。

不要引入 chat-system / botkit 当依赖。Easybot 是 GPL-3 IM 网关，产品边界不是 Agent，但 QQ 运输实现可以写入本仓库：能力声明、`platform:chat[:thread]` 会话键、被动 `msg_id`/`msg_seq`、文件上传、401/403 永久失败与网络抖动的 Transient/Permanent、出站幂等。组合作品仍是 AGPL-3；从 Easybot 搬来的文件保留 GPL-3 声明与来源。不要把 Easybot 的 REST/WebSocket 网关、计费、插件加载器整仓引进 `Cargo.toml`。
[Easybot](https://github.com/EasyIndie/Easybot)

## 办事出站格式

浏览器订 `POST /api/agent/process/stream`。外部通道订同一条 `AgentProgressEvent` 流。
回程在任务还活着时打专用接口，不是再发一条 `ProcessRequest`。
路由：`backend/src/api/agent/routes.rs`

### 终态 `responseType`

Planner 先分流。`backend/src/services/agent/process_work.rs`

| `responseType` | 含义 | 通道至少要能 | 做不到 |
| --- | --- | --- | --- |
| `answer` | 一句话结束。内部 `PlannerStatus::Chat` 也走这里 | 发文本 | 不能接办事 |
| `clarification` | 规划前问清楚，带 `suggestions` | 按钮或让人回复 | 任务不会开始 |
| `confirmation_required` | 敏感步骤等点头，有过期 | 明确是/否 | **必须失败**，不能默认同意 |
| `task_created` / `task_progress` | 任务还在跑 | 过程事件 | 人会以为卡住 |
| `task_completed` | 配方跑完，内嵌完整 `ApiResponse` | 按字段渲染 | 只说「好了」会丢表/图/动作 |
| `error` | 失败文案 + `code` | 文本 | — |

`ApiResponse` 字段：`backend/src/api/agent/types.rs`

| 字段 | 内容 | 外部通道 |
| --- | --- | --- |
| `message` | 主文案 | 必投 |
| `dataDisplay` | 见下表 | 降级成文本或文件，不能丢 |
| `suggestions` | 后续建议 | 按钮或折进文本 |
| `task` | 任务 id、状态、进度 | 一条进度 |
| `confirmation` | id、风险、过期、待确认步骤 | 没有交互就不要跑会触发确认的任务 |
| `frontendAction` | 导航、开窗、点页面、播歌单 | **浏览器专属**。假装执行等于谎报成功 |
| `performance` | 表情/动作 | 丢弃 |
| `sessionId` | 办事会话 | 自己记，下一句打回同一 lane |

`dataDisplay`：`table` / `chart` / `card_list` / `markdown` / `key_value` / `timeline` / `raw`。
`backend/src/services/agent/types.rs`

### 过程事件

必须处理：`run_started`、`session_created`、`task_created`、`step_started`、`step_completed`（可能带 `imageUrl`、`frontendActions`）、`progress`、`waiting_for_input`、`task_completed`、`error`。

应当处理：`step_retrying`、`summary_token`。

默认丢弃：`thinking_token`、`planner_decision`、`step_debug`、`task_assigned`、`performance_plan`、`merope_state_changed`、`outfit_overlay`（Chat-only）、`music_control`、`session_title_updated`。

`step_completed.frontendActions` 在步骤结束时就要在浏览器执行。通道没有页面，必须当不支持，并让该步失败可见。

### 回程接口

| 人做了什么 | 接口 |
| --- | --- |
| 规划前澄清 | `POST /api/agent/clarify` |
| 敏感确认 | `POST /api/agent/confirm/stream` |
| 执行中答题 | `POST /api/agent/tasks/{id}/answer` 或 `.../answer/stream` |
| 浏览器做完一步 | `POST /api/agent/tasks/{id}/frontend-ack`（IM 没有这个表面） |
| 取消 | `POST /api/agent/tasks/{id}/cancel` |
| 办事换题 | `POST /api/agent/session/interrupt` |

`waiting_for_input.questionType`：`confirmation`、`single_choice`、`multiple_choice`、`free_text`、`numeric`、`date`。
`backend/src/services/agent/types.rs`

没有按钮的通道，确认只能靠回复是/否。解析失败必须再问。确认有 `expiresInSeconds`，过期当失败。

### 能力映射

| 能力 | 对应格式 | 缺了 |
| --- | --- | --- |
| `text` | `answer` / `error` / `message` | 不能接 |
| `interactive` | 澄清、确认、单选 | 一遇到确认就失败停住 |
| `free_text_reply` | 自由文本 / 数值 / 日期 | 执行中提问接不住 |
| `typing` | `progress` / `step_*` | 长任务像死了 |
| `message_edit` 或 `streaming_draft` | `summary_token` | 等 `done` 再整段发 |
| `image` / `document` | `imageUrl`、表格降级 | 只剩一句「好了」 |
| `frontend_action` | `frontendAction` | IM 声明 false；碰到就明确失败 |
| `performance` / `outfit` | 形象事件 | 声明 false，丢弃 |

## 第一平台：QQ 机器人

官方 API v2。[消息收发概述](https://bot.q.qq.com/wiki/develop/api-v2/server-inter/message/overview.html)

QQ 是三种场景三套接口。第一版只认 **单聊（C2C）**。
群被动窗口 5 分钟、默认只收 @；频道 Markdown/按钮要内邀。长任务撑不住。

| | 单聊 | 群 | 频道 |
| --- | --- | --- | --- |
| 入站 | `C2C_MESSAGE_CREATE` | `GROUP_AT_MESSAGE_CREATE` 或全量 `GROUP_MESSAGE_CREATE` | `AT_MESSAGE_CREATE` |
| 出站 | `POST /v2/users/{openid}/messages` | `POST /v2/groups/{openid}/messages` | 子频道消息 |
| 流式 | 有 `.../stream_messages` | 无 | 无 |
| 被动窗口 | 60 分钟 / 同一条最多回 4 次 | 5 分钟 / 5 次 | 5 分钟 |
| 编辑 | 无，只有撤回，发出超 2 分钟不能撤 | 同左 | 有编辑 |

基址：`https://api.bot.qq.com`（业务）、`https://bots.qq.com`（鉴权）。
凭证：AppID + AppSecret 换 access token；旧 Token 已弃用。
传输：Gateway WebSocket 收事件，HTTPS 发消息；发消息要求 WS 在线。
配对：`user_openid` → Myriad 用户。这不是用户 QQ 登录 OAuth。

`msg_type`：

| 值 | 内容 | 办事用处 |
| --- | --- | --- |
| 0 | 文本 `content` | `answer` / `error` / 进度摘要 |
| 2 | Markdown | `dataDisplay` 降级。单聊/群自定义 Markdown 已开放；频道要内邀 |
| 7 | 富媒体，先上传拿 `file_info` | `imageUrl`、文件。单聊和群上传接口不互通，`file_info` 有 TTL |

按钮挂在 Markdown 底下，最多 5×5。单聊/群自定义按钮已开放。
[消息按钮](https://bot.q.qq.com/wiki/develop/api-v2/server-inter/message/trans/msg-btn.html)

- 动作 0：跳转
- 动作 1：回调后台（确认、单选）
- 动作 2：往输入框塞指令

没有 typing。被动回复必须带用户那条 `msg_id`，否则算主动消息。用户可在客户端关掉主动接收。
相同 `msg_id` 可能重复推，用 `msg_seq` 去重；同一条被动回复要递增 `msg_seq`。
流式只在单聊：[流式发送单聊消息](https://bot.q.qq.com/wiki/develop/api-v2/autogen/api/v2_users_user_openid_stream_messages.post.html)

主动消息频控（未认证单聊）：约 5/qps 且 30/qpm，每用户每天 1000 条。数字以官方文档为准，会变。

实现对照可读 Easybot 的 `easybot-adapter-qq`（手写 Gateway + reqwest）。QQ 运输可按该 crate 写入；Gateway/鉴权优先用 MIT 的 [`qq-bot-rs`](https://github.com/yenharvey/qq-bot-rs)（无流式发送，第一刀本来就不做流式草稿）。
已知坑：`msg_type: 7` 带空 `content` 会多空行；C2C 不走旧图文混合；群/C2C 不能 edit。

### QQ 单聊能力相对办事格式

| 办事需要的 | QQ 单聊 |
| --- | --- |
| `text` | 有 |
| `markdown` | 有 |
| `interactive` | 有，按钮挂在 Markdown 上；`permission` 绑这个 openid |
| `free_text_reply` | 有，下一句 `C2C_MESSAGE_CREATE` |
| `image` / `document` | 有，上传 + `msg_type: 7` |
| `streaming_draft` | 仅单聊 |
| `message_edit` | 无 |
| `typing` | 无 |
| `frontend_action` | 无，声明 false |

### 第一刀（奥卡姆）

只保证四件事：QQ 单聊能收纯文本、已配对用户走办事、终态文本能回、没有入站时也能主动发一条。

1. 只接 C2C 文本。session key：`qq:{user_openid}` → 配对后的用户 → 一个 Work 会话。群、频道、附件、按钮第一刀忽略。
2. 入站只处理 `C2C_MESSAGE_CREATE`。留下 `msg_id`，被动回复带它，`msg_seq` 递增。同一 `msg_id` 不双开任务。
3. 过程事件全部吞掉。只投终态 `message`：`answer` / `error` / 「请到面板确认或完成页面动作」。
4. 有被动窗口就被动回；没有就主动发。设置里可对已配对 QQ 测一条主动消息。
5. 碰到确认或 `frontendAction`：回「请到站点面板完成这一步」，取消或让该步失败，不要默认同意。
6. 流式草稿、按钮回程、图片、Markdown 表格不在第一刀。

## 不要做

- 把 `backend/src/api/discord.rs` 的数据平台 OAuth 复用成 bot token
- 为通道伪造 `rig_state` / `current_route`
- 群或频道作为第一版办事表面
- 在通道里默认同意确认
- 把 Easybot 的网关进程、REST/WebSocket API、计费或 chat-system 引进 Cargo.toml
