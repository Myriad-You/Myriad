# Telegram Bot 通道调研

这是调研，不是规格。喂给 `agent-channel.md` 的第二平台决策（QQ 之后）；方案落 PR，不落这里。
术语见 `CONTEXT.md`：办事 / 聊天 / 配对 / 只写。外部文档抓取日期 2026-09-07；限频与大小上限数字会漂，以官方文档为准。

## 凭证：一个 token，没有换 token 环节

| | QQ | Telegram |
| --- | --- | --- |
| 凭证 | AppID + AppSecret 换 access token | BotFather 签发单个 bot token |
| 形态 | 一对字段 | `<id>:<hash>`，如 `123456:ABC-DEF…` |
| 刷新 | 3500s 定时换（`backend/src/services/qq_bot.rs:29-33`） | 无刷新环节；泄漏时 BotFather 撤销重发（第三方配置面） |

- 请求统一走 `https://api.telegram.org/bot<token>/METHOD_NAME`，GET/POST 均可；响应恒有 `ok`，失败给 `description` / `error_code`，限频错误附带 `parameters`（bots/api「Making requests」节）。
- **bot token 不是 MTProto `api_id`/`api_hash`。** 后者是用户账号客户端（TDLib 等）的凭证，bot 完全用不上。仓库已吃过把 Discord 数据平台 OAuth 当 bot 凭证的亏（`agent-channel.md`「不要做」第一条），这条别再犯。BotFather 页面给的 token 就是要落库的全部。
- 落库模式照抄 QQ：`telegram_bot_enabled: bool` + `telegram_bot_token` 只写（`backend/src/config.rs:381-384`）。`worker_intent(enabled, token 非空)` 可直接复用 `crates/myriad-agent-rules/src/channel.rs:336-342` 的形状，比 QQ 少一个「secret 是否存在」位。

## 入站传输：getUpdates 长轮询，不用 webhook

| | getUpdates | setWebhook |
| --- | --- | --- |
| 方向 | 纯出站，worker 主动拉 | Telegram 打进来，要公网 HTTPS + 443/80/88/8443 端口 |
| 运维 | 无 | 域名、证书、secret_token 请求头校验 |
| 先例 | 同 QQ Gateway worker：`qq_bot.rs:160` run_loop 出站长连接 | 仓库没有入站 web 服务先例 |

选长轮询。QQ 先例就是 worker 纯出站（`qq_bot.rs:160,244`），Telegram 长轮询同构：worker 循环 HTTP 请求，`timeout` 秒数由服务端挂住连接；官方建议 positive，短轮询仅测试用（bots/api getUpdates 节）。

核实过的规则（bots/api Update / getUpdates 节）：

- `update_id` 顺序递增；**超过一小时没有新 update，下一个 id 改为随机选取**，offset 逻辑不能假设连续。
- 确认语义：`offset` 传上一轮最大 `update_id` + 1，被越过即确认并弃存。服务端只保留未取 update 最多 24 小时。
- `allowed_updates` 白名单订类型；默认不含 `chat_member` 类。第一刀只订 `message`。
- 官方 Note：webhook 开着时 getUpdates 不可用（原话「This method will not work if an outgoing webhook is set up」）；重复拉取靠「每次响应后重算 offset」避免。**409 Conflict 这个具体状态码官方页面没有逐字写明**（uncertain，实践中是 webhook 或另一实例占用），它是可恢复错误，归 Transient。
- Update 字段核实：`update_id` / `message` / `edited_message` / `callback_query`；另有 `my_chat_member` 可感知被拉黑/解除。

## 会话与配对

- 会话键 `session_key("telegram", chat_id)`，冒号消毒已有（`channel.rs:108-118`），零改动。
- 绑定身份：`provider = "telegram"`，`provider_user_id = from.id`（数字用户 id，官方明文永不变化语义）。**私聊中 `chat.id` 与 `from.id` 数值相同——官方页面未逐字写明**（uncertain），实现以实际 Update 核对。
- `user_identities` / 配对码整套可直接换 provider 复用：`lookup_openid`（`backend/src/services/qq_pairing.rs:77-98`）、配对码 mint/consume/bind（`qq_pairing.rs:179-316`）、管理接口 GET/POST/DELETE `/api/agent/qq/pairing`（`backend/src/api/agent/qq_pairing.rs:7-71`）。
- 解析入站只需四个字段：`update_id`、`message.message_id`（chat 内唯一）、`message.from.id`、`message.text`。`from` 在私聊恒存在但类型上可空（频道消息为空），为空直接丢弃。
- 决策树复用 `InboundDecision` 四分支与 `ingest_c2c_text`（`channel.rs:138-159,180-214`），仅去重键语义从 QQ `msg_id` 换成 `update_id`。

## 出站：没有被动窗口

Telegram 没有被动回复窗口。`DeliveryContext` 的 `inbound_msg_id` / `passive_window_open` / `remaining_passive_replies`（`channel.rs:233-237`）在 Telegram 无对应物。

- `can_reply_passively` 恒 false（`channel.rs:257-261`），`plan_delivery` 自动全走 `ActiveText`（`channel.rs:275-301`）——**规则层不用改，语义自然退化**，但前提是 bot 对该用户有发言权：官方明文「Bots can't start conversations with users. A user must either add them to a group or send them a message first」（bots 页「How Are Bots Different from Users?」）。即主动出站只对配对过（发过消息）的用户成立；被拉黑返回 403。
- 幂等：QQ 的 `msg_seq` / `next_passive_seq`（`channel.rs:304-306`）不需要；`outbound_idempotency_key`（`channel.rs:310-326`）的 msg_id 槽填出站后拿到的 `message_id` 或空走 active 槽。429 重试可能双发，第一刀接受。
- `sendMessage`：`chat_id`（Integer 或 String）+ `text`，**1-4096 字符，按 entities 解析后计**；`parse_mode` 支持 HTML / MarkdownV2 / 旧 Markdown（官方只推荐前两者）；`link_preview_options` 可关预览（bots/api sendMessage 节）。办事终态直接投 `Answer`/`Error` 文本。
- typing：`sendChatAction`，action `typing`，**状态维持 5 秒或更短**，bot 的消息到达时客户端自动清除；官方仅建议在响应需要可感知等待时用。可映射 `Progress` 事件。
- 编辑：`editMessageText` 可改 bot 自己发的文本消息；「Updating messages」节原话：目前只能编辑无 `reply_markup` 或带 inline keyboard 的消息。**bot 自发消息的编辑时间窗页面没有写**（仅对非 bot 发的 business 消息明文 48 小时）——别拍脑袋承诺。
- 按钮：`InlineKeyboardButton.callback_data` 1-64 字节；用户点击后必须 `answerCallbackQuery`，否则客户端一直转进度条（bots/api InlineKeyboardMarkup / CallbackQuery 节）。第一刀不开。
- 文件：`sendPhoto` multipart 上传上限 10 MB（HTTP URL 方式 5 MB），`sendDocument` 上限 50 MB（官方注明「may be changed in the future」）；下发方向 `getFile` 只能下载 20 MB 以内，链接 `https://api.telegram.org/file/bot<token>/<file_path>` 至少 1 小时有效，不保证保留原文件名与 MIME（bots/api 各节 + FAQ）。`file_id` 复用是官方推荐姿势。

## 限频与错误分类

限频（bots/faq「My bot is hitting limits」节原话口径，数字会漂，抓取 2026-09-07）：单聊约 1 msg/s（允许短促突发，超了开始收 429）；群约 20 msg/min；广播默认约 30 msg/s。超限返回 429 + `parameters.retry_after` 秒数（bots/api ResponseParameters 节）。

| HTTP | 场景 | 分类 | 对应 `classify_connect_failure`（`channel.rs:470-502`） |
| --- | --- | --- | --- |
| 401 | token 被撤销/错误 | Permanent | HttpStatus 401 同款 |
| 403 | 用户拉黑 bot / 隐私限制 | Permanent | HttpStatus 403 同款 |
| 400 `chat not found` | 没配对就主动发 | Permanent | 「其他 4xx → Permanent」同款 |
| 429 | 限频，读 `retry_after` 退避 | Transient | 同 429 分支 |
| 409 | webhook / 另一实例占用 getUpdates | **Transient** | 不是 token 问题，别当 Permanent |
| 5xx / 网络 | 服务端或本地网络抖动 | Transient | Transport 同款 |

Phase 机、指纹轮询、退避（`QqBotPhase`、`CredentialFingerprint`、`transient_backoff`，`qq_bot.rs:37-43,109-136,496-503`）全部照搬，只是失败源从 Gateway 关闭码换成 HTTP 状态码。

## 与 QQ 差异一览

| | QQ 单聊 | Telegram DM |
| --- | --- | --- |
| 被动窗口 | 60 分钟 / 4 次 | 无，总能主动发（前提：用户开过聊） |
| 传输 | Gateway WS 在线才能发 | getUpdates 纯出站 HTTP |
| 编辑 | 无（仅撤回） | 有，bot 自发消息可 `editMessageText` |
| typing | 无 | `sendChatAction`，5 秒 |
| 按钮 | Markdown 挂按钮，需自定义语法 | 原生 inline keyboard + callback |
| Markdown | 自定义语法，频道要内邀 | HTML / MarkdownV2 全量 |
| 幂等 | `msg_id` + `msg_seq` | 入站 `update_id`；出站无 seq |
| 限频 | 约 5/qps、30/qpm、日 1000 条 | 单聊约 1/s、广播约 30/s，429 带 retry_after |
| 凭证 | AppID+AppSecret 换 token | 单 bot token，无刷新 |
| 群入站 | 群 @ 消息 | 默认 privacy mode 只收命令/@/回复；开关在 BotFather（第三方配置面） |
| 大陆可达性 | 国内服务 | `api.telegram.org` 在中国大陆的可达性不在平台控制内（uncertain）；自托管部署需自行确认出站可达，可用代理或本地 Bot API server 缓解 |

本地 Bot API server（[tdlib/telegram-bot-api](https://github.com/tdlib/telegram-bot-api)）自托管后放宽：上传 2000 MB、下载无上限、webhook 任意端口。第一刀用不上，是大陆部署/大文件的备选路径。

## 生态

- [frankenstein](https://github.com/ayrat555/frankenstein)：types-only 客户端，跟到 Bot API 10.3，不带框架。
- teloxide：全套框架（命令分发、状态机），依赖面大。
- QQ 第一刀的先例是手写传输：worker 直接 reqwest + serde_json 挖字段（`qq_bot.rs:418`）。**判断：第一刀不引依赖**，需要的结构体就六个（Update/Message/Chat/User + 两个响应壳）；要类型层时 frankenstein 也只是锦上添花。

## 第一刀（奥卡姆）

只保证一条链：Telegram 私聊进字 → 站点办事跑完 → 同一聊天出字。

1. **工人能 getUpdates 收发私聊文本。** 设置里只写 token，开关打开后长轮询；`allowed_updates` 只订 `message`，只认 `chat.type == "private"`。401/403 停工人，429 读 `retry_after`，409 退避。
2. **from.id 能变成站点用户。** 复用 `InboundDecision` 与配对码链路，provider=`telegram`。第一刀可以沿用 QQ 的减配：第一句绑站长或设置里指定。
3. **办事跑完回一句终态文本。** `sendMessage`，超 4096 按 QQ 同款策略截断/分段；过程事件吞掉；确认与 `frontendAction` 失败可见，禁止默认同意。

会话键 `telegram:{chat_id}`。群、频道、按钮、图片、编辑、typing 全部忽略——都在能力位里关着。

## 不要做

- 把 MTProto `api_id`/`api_hash` 当 bot 凭证，或把 bot token 与用户账号客户端混为一谈。
- 群 / 频道作为第一版办事表面（privacy mode 语义又是一层坑，第一刀不碰）。
- 因为「能编辑」就把 `outbound_streaming_draft` 或 `outbound_edit` 声明成 true——先核实编辑窗口与 Bot API 新的草稿接口稳定性再说；未声明能力按不支持处理是 `agent-channel.md` 定的铁则。
- 把 409 Conflict（webhook/实例占用）当成 Permanent 停机；那是配置竞争，退避即可。

## uncertain 清单

- `api.telegram.org` 中国大陆可达性：不在平台控制内，不做断言；自托管需自行验证。
- 私聊 `chat.id == from.id`：社区共识，官方页未逐字写明。
- bot 自发消息的编辑时间窗：官方页未写，仅有 business 消息 48 小时明文。
- 409 Conflict 具体状态码：官方只写「will not work」，状态码来自实践观察。
- BotFather 撤销/重发 token、群 privacy mode 开关：均在 Telegram 侧配置面，行为不受本仓库控制。

## 来源

抓取日期均为 2026-09-07；bot api 页对应 Bot API 10.3。

- https://core.telegram.org/bots/api （Update / getUpdates / setWebhook / Message / sendMessage / editMessageText / sendChatAction / sendPhoto / sendDocument / getFile / InlineKeyboardMarkup / CallbackQuery / ResponseParameters / Paid Broadcasts / 本地 Bot API server 各节）
- https://core.telegram.org/bots/faq （限频原话、privacy mode、20 MB 下载 / 50 MB 上传）
- https://core.telegram.org/bots/features （privacy mode）
- https://core.telegram.org/bots/webhooks （webhook 指南、端口、secret_token）
- https://core.telegram.org/bots （「Bots can't start conversations with users」原话）
- https://github.com/tdlib/telegram-bot-api （自托管 server 能力）
- https://github.com/ayrat555/frankenstein （生态判断）
