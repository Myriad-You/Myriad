# Tapp REST API

本文记录当前宿主调用的 Tapp 后端路由。第三方 Tapp 代码通常应使用
[Tapp SDK](API_REFERENCE.md)，不要直接拼接这些 URL；SDK 会处理身份、CSRF、字段转换、
权限预检和错误解包。

架构与安全边界见 [Tapp 架构](ARCHITECTURE.md)。路由权威来源是
`backend/src/main.rs`、`backend/src/api/tapp_store.rs` 和
`backend/src/api/tapp_scheduler.rs`。

## 请求约定

### 基础 URL

浏览器同源调用 `/api/...`。开发环境由 Astro 把 `/api/*` 转发到后端；生产环境由
Myriad proxy 转发，不要在 Tapp 中硬编码后端端口。

### 身份与 CSRF

- 登录态使用 HttpOnly 会话 Cookie，前端请求设置 `credentials: "include"`。
- GET/HEAD/OPTIONS 以外的变更请求需要 `X-CSRF-Token`。
- CSRF token 从 `GET /api/csrf-token` 获取。
- 标为“可选认证”的路由仍会根据当前身份、Tapp owner 和最终授权返回不同结果。
- Tapp 沙箱运行时请求还必须携带宿主持有的 `X-Tapp-Runtime-Grant`。该值不能交给 iframe；
  它绑定 subject、owner、Tapp、具体 runtime 和最终权限，5 分钟过期。
- 不要把旧文档中的 Bearer token 示例用于当前浏览器宿主。

```javascript
const csrf = await fetch("/api/csrf-token", {
  credentials: "include",
}).then((response) => response.json());

await fetch("/api/tapps/com.example.app/start", {
  method: "POST",
  credentials: "include",
  headers: { "X-CSRF-Token": csrf.csrf_token },
});
```

### 响应格式

Tapp 管理接口大多返回：

```json
{ "success": true, "data": {} }
```

错误通常是：

```json
{ "success": false, "error": "message" }
```

并非所有历史接口都使用同一个 envelope：资源接口返回资源对象，scheduler 返回
`{success, task/tasks/...}`，部分运行时接口也直接返回业务字段。调用方必须同时检查 HTTP
状态和该端点的具体响应；不要假设存在统一的 `error.code/details` 结构。

## 应用管理 `/api/tapps`

### 可选认证的读取路由

| 方法 | 路径                            | 说明                                         |
| ---- | ------------------------------- | -------------------------------------------- |
| GET  | `/api/tapps`                    | 可见 Tapp 摘要；管理员 Tapp + 当前用户 Tapp  |
| GET  | `/api/tapps/details`            | 批量详情、Manifest、状态和当前角色的最终授权 |
| GET  | `/api/tapps/widgets`            | 所有可见的已注册 Widget                      |
| GET  | `/api/tapps/store/sources`      | 已启用的商店源                               |
| GET  | `/api/tapps/{tappId}`           | Tapp 详情、Manifest、状态和最终授权          |
| GET  | `/api/tapps/{tappId}/code`      | 主代码文本                                   |
| GET  | `/api/tapps/{tappId}/resources` | 代码、CSS、HTML、i18n、Page 模块等资源对象   |
| GET  | `/api/tapps/{tappId}/export`    | 导出 `.tapp` ZIP                             |

读取与运行时授权优先当前用户的私有安装；未安装私有副本时再使用站点公开（管理员）安装。
用户可与公开安装并存私有副本；管理员公开安装仍拒绝覆盖任何已有同 ID 安装。

Storage（Option A）按安装 owner 命名空间隔离：打开公开安装时读写站点 owner 数据，viewer
只读；写/删/清空仅 owner。个人数据需用户安装自己的副本。

`/details` 是 `TappRuntime` 的启动同步接口。它固定执行管理员集合与当前用户集合查询，
同 ID 时保留用户私有版本，并对每项应用与单项 `/api/tapps/{tappId}` 相同的动态角色权限过滤，
用于避免列表后逐项读取详情的 N+1 请求。列表项使用 camelCase；详情沿用历史 snake_case。

`/resources` 当前返回 snake_case 字段，`TappApiService.getTappResources` 会转换为：

```typescript
interface TappResources {
  code: string;
  styles?: string;
  widgetStyles?: string;
  pageStyles?: string;
  widgetCSS?: string;
  pageCSS?: string;
  widgetTemplates?: Record<string, Record<string, string>>;
  pageTemplate?: string;
  cssMode?: "unified" | "separated";
  i18n?: Record<string, unknown>;
  pageModules?: Record<string, string>;
  pageModuleOrder?: string[];
}
```

### 需要登录的变更路由

| 方法   | 路径                                 | 说明                                    |
| ------ | ------------------------------------ | --------------------------------------- |
| POST   | `/api/tapps/install`                 | direct/store 统一安装                   |
| POST   | `/api/tapps/install-file`            | multipart 上传 `.tapp`，字段名 `file`   |
| POST   | `/api/tapps/cleanup-temporary`       | 清理当前用户临时 Tapp                   |
| GET    | `/api/tapps/recent?limit=10`         | 当前用户最近运行的 Tapp                 |
| POST   | `/api/tapps/{tappId}/update`         | direct/store 更新，保留用户数据         |
| POST   | `/api/tapps/{tappId}/start`          | 持久化 owner 自己的 running 状态        |
| POST   | `/api/tapps/{tappId}/stop`           | 停止 owner 安装并撤销对应 Runtime Grant |
| DELETE | `/api/tapps/{tappId}?keep_data=true` | 卸载；可选保留存储/设置                 |
| POST   | `/api/tapps/{tappId}/separated-css`  | 写入生成后的 Widget/Page CSS            |

直接安装请求：

```json
{
  "source": "direct",
  "manifest": {
    "id": "com.example.app",
    "name": "App",
    "version": "1.0.0",
    "main": "main.js",
    "permissions": []
  },
  "code": "console.log('hello')",
  "styles": "...",
  "pageTemplate": "...",
  "widgetTemplates": { "clock": { "2x2": "..." } },
  "widgetCss": "...",
  "pageCss": "...",
  "i18n": { "zh-CN": {} },
  "pageModules": { "index.js": "..." },
  "permissions": []
}
```

商店安装请求：

```json
{
  "source": "store",
  "storeSource": "1",
  "tappId": "com.example.app",
  "permissions": ["storage"]
}
```

`permissions` 是用户同意的申请子集，不是可信授权；后端会按 Manifest、当前实时角色和
动态权限配置再次过滤。

安装/更新资源先进入 staging，校验后原子替换在线目录；数据库失败恢复旧目录。卸载把文件
移入隔离目录后，在一个事务中清理安装记录、Manifest/动态 Widget、调度任务及执行历史；
`keep_data=true` 只保留 storage，不保留任务或 Widget。相同公开 `tappId` 的最终冲突复核、
文件切换和数据库变更由 PostgreSQL advisory transaction lock 串行化，跨后端副本也不能并发覆盖。
所有当前管理员都操作首次站点管理员对应的规范公开 owner；普通用户仍写入自己的临时 owner。
管理员身份不会让 Runtime 自动读取其他普通用户的私有安装。

### Runtime Grant

以下路由使用可选认证，因此游客运行管理员共享 Tapp 时也能获得绑定稳定 guest subject 的
Grant；游客仍不能借此访问普通用户的临时安装。

| 方法   | 路径                                             | 说明                         |
| ------ | ------------------------------------------------ | ---------------------------- |
| POST   | `/api/tapps/{tappId}/runtime-grants`             | 为 Page/Widget/headless 签发 |
| DELETE | `/api/tapps/{tappId}/runtime-grants/{runtimeId}` | 销毁实例时撤销               |

签发 body 为 `{ "instanceId": "page_...", "kind": "page" }`；`kind` 还可以是
`widget` 或 `headless`。令牌只返回宿主一次，服务端仅存 SHA-256。停止、更新、卸载会撤销
相关 Grant。Grant 哈希与 TTL 租约存储在 PostgreSQL，可由任意副本校验；只有被撤销或到期的
Grant 才返回 `INVALID_RUNTIME_GRANT`。
每次使用 Grant 时还会重新核对当前可见安装 owner，并以当前角色、动态权限配置和安装授权
收缩权限；owner 改变的旧 Grant 会返回 `INVALID_RUNTIME_GRANT`，由宿主执行一次重签重试。

### 设置、Widget 与存储

| 方法   | 路径                                     | 说明                               |
| ------ | ---------------------------------------- | ---------------------------------- |
| GET    | `/api/tapps/{tappId}/settings/{key}`     | 登录宿主读取 Manifest 声明的设置   |
| POST   | `/api/tapps/{tappId}/settings/{key}`     | 登录宿主写入 Manifest 声明的设置   |
| POST   | `/api/tapps/{tappId}/widgets`            | Runtime Grant 注册/更新动态 Widget |
| DELETE | `/api/tapps/{tappId}/widgets/{widgetId}` | Runtime Grant 注销动态 Widget      |
| GET    | `/api/tapps/{tappId}/storage`            | 列出 key                           |
| DELETE | `/api/tapps/{tappId}/storage`            | 清空存储                           |
| GET    | `/api/tapps/{tappId}/storage/entries`    | 一次返回全部键值                   |
| GET    | `/api/tapps/{tappId}/storage/usage`      | 一次统计使用字节                   |
| GET    | `/api/tapps/{tappId}/storage/{key}`      | 读取值                             |
| POST   | `/api/tapps/{tappId}/storage/{key}`      | 写入值                             |
| DELETE | `/api/tapps/{tappId}/storage/{key}`      | 删除值                             |

settings 路由是详情页宿主控制面，只接受已登录会话与当前 Manifest 的真实 key，不接受
`X-Tapp-Runtime-Grant` 代替登录，也不能访问任意 storage key。写入值必须符合声明的
type、select options 与 number min/max；游客只使用 Manifest 默认值。
`storage` 路由同样要求登录身份和 Runtime Grant；访客 Grant 不包含 `storage`，因此沙箱内
的 `Tapp.storage`/`Tapp.settings` 也不能为访客创建持久数据。
Manifest Widget 由安装/更新自动对账；动态 Widget 路由要求 `widget:register` 同时存在于
Runtime Grant、安装授权和当前角色，并拒绝覆盖/删除 Manifest 来源的注册。

注意写入方法是 `POST`，不是旧文档中的 `PUT`。
Widget 注册 body 除 `id`、`name`、`default_size`、`sizes` 等元数据外，还可包含
`settings` 与 `refresh_policy`；后端会执行与 Manifest 相同的字段、类型、范围和刷新间隔
校验，并将其保存到 Widget 注册记录。

### 商店源管理

| 方法   | 路径                                  | 说明                         |
| ------ | ------------------------------------- | ---------------------------- |
| POST   | `/api/tapps/store/sources`            | 添加源；handler 内检查管理员 |
| POST   | `/api/tapps/store/sources/{sourceId}` | 更新源；handler 内检查管理员 |
| DELETE | `/api/tapps/store/sources/{sourceId}` | 删除源；handler 内检查管理员 |

不存在 `/api/tapp-store/...` 路由。商店索引和应用资源也可能由浏览器通过
`RemoteStoreService` 读取；后端商店下载失败时，安装器会回退到浏览器下载 + direct 安装。

## 运行时 `/api/tapp`

这些端点主要由 Bridge handler 经 `TappApiService` 调用。除了路由中间件，handler 还应
校验 Tapp ID、owner 与 `granted_permissions`。

### 平台、AI 与数据

| 方法   | 路径                                                     | 身份     | SDK 能力                        |
| ------ | -------------------------------------------------------- | -------- | ------------------------------- |
| GET    | `/api/tapp/platform/{platform}/data`                     | 登录     | `Tapp.platform.getData`         |
| GET    | `/api/tapp/platform/{platform}/stats`                    | 登录     | `Tapp.platform.getStats`        |
| GET    | `/api/tapp/platform/{platform}/distribution/{dimension}` | 登录     | `Tapp.platform.getDistribution` |
| POST   | `/api/tapp/platform/items`                               | 登录     | `Tapp.platform.addItem`         |
| POST   | `/api/tapp/platform/items/batch`                         | 登录     | `Tapp.platform.addItems`        |
| POST   | `/api/tapp/ai/v2/tasks`                                  | 可选认证 | 创建 AI 任务                    |
| GET    | `/api/tapp/ai/v2/tasks/{taskId}`                         | 可选认证 | 读取任务快照                    |
| DELETE | `/api/tapp/ai/v2/tasks/{taskId}`                         | 可选认证 | 取消非终态任务                  |
| GET    | `/api/tapp/ai/v2/tasks/{taskId}/events`                  | 可选认证 | SSE token/progress/state        |
| GET    | `/api/tapp/ai/v2/usage`                                  | 可选认证 | 权威 calls/tokens/cooldown      |
| POST   | `/api/tapp/data/transform`                               | 登录     | `Tapp.data.transform`           |

AI 权限、每分钟速率、每日 calls/tokens 与 cooldown 全由后端执行。配额在模型调用前事务预留、
完成后按实际估算结算、失败/取消释放未消耗 token；calls 仍记录一次尝试。AI Task 还校验
Manifest operation/model tier/context/output 声明，并将任务绑定 subject、安装 owner 和 Tapp。
最终任务注册在 subject advisory-lock 事务中原子检查并发数、保留数和幂等键；未成功注册的
请求完整回滚 calls/token 预留，不会留下只计费但未执行的任务。

### One-shot Data Exchange

四个端点都接受真实用户或稳定 guest Claims，并强制校验 `X-Tapp-Runtime-Grant`：

| 方法   | 路径                                                     | 说明                         |
| ------ | -------------------------------------------------------- | ---------------------------- |
| POST   | `/api/tapp/data-exchange/requests`                       | 校验双方 Manifest 并准备请求 |
| POST   | `/api/tapp/data-exchange/requests/{requestId}/authorize` | 宿主确认后签发一次性 Grant   |
| DELETE | `/api/tapp/data-exchange/requests/{requestId}`           | 拒绝/关闭并撤销请求或 Grant  |
| POST   | `/api/tapp/data-exchange/consume`                        | 提供方提交并原子消费 Grant   |

准备请求 body 为 `{targetTappId, exportId, params, purpose}`。只有调用方声明 import、提供方
声明同名 export、双方可由同一 subject 访问时才返回包含 `params` 的弹窗元数据。授权端点只
允许原 requester runtime 调用；返回的一次性 token 60 秒过期且留在宿主。DELETE 在授权前
删除 prepared request，授权后按 requestId 删除活动 Grant。runtime 撤销、Tapp 停止、更新和
卸载也会清理 requester 状态；提供方 Tapp 停止或卸载会清理指向它的请求与 Grant。consume
必须携带匹配 Tapp ID、subject 与安装 owner 的 provider Runtime Grant，服务端先删除一次性 token，再验证响应 schema、
`maxBytes` 和 `maxRecords`，因此失败
也不能重放。详细 SDK 契约见 [Data Exchange API](API_REFERENCE.md#跨-tapp-data-exchange-api)。

### Event Broker 与 Agent Interaction

| 方法 | 路径                                           | 说明                       |
| ---- | ---------------------------------------------- | -------------------------- |
| POST | `/api/tapp/events/publish`                     | 发布 Manifest 声明 topic   |
| GET  | `/api/tapp/events/stream`                      | 当前 runtime 在线 SSE      |
| GET  | `/api/tapp/agent/v2/interactions/stream`       | 接收待处理 Interaction     |
| GET  | `/api/tapp/agent/v2/interactions/{id}`         | 读取状态快照               |
| POST | `/api/tapp/agent/v2/interactions/{id}/accept`  | 当前 runtime 接受          |
| POST | `/api/tapp/agent/v2/interactions/{id}/result`  | schema 校验后提交结果      |
| POST | `/api/tapp/agent/v2/interactions/{id}/reject`  | 拒绝并终止                 |
| POST | `/api/tapp/agent/v2/interactions/{id}/intents` | 宿主确认后记录 intent 授权 |

Event Broker 仅在线 at-most-once，不做积压；SSE 在 Runtime Grant 到期时关闭，由宿主刷新 Grant 后
重连。Agent Interaction 由可信 Agent 执行代码在后端创建，Tapp 不能伪造 source/task；只有实际
accept 的 runtime 可提交结果，结果会恢复持久化的原 Agent 任务；intent 经授权后由受支持的
宿主 adapter 执行。Runtime Grant、Data Exchange、Event、Agent interaction 和 AI task 使用
PostgreSQL TTL registry/mailbox；`pg_notify` 只作唤醒提示，消费者可从 mailbox 补读。
Interaction 的动作截止时间独立于终态保留时间；所有副本都可运行过期扫描，但数据库 CAS 只
允许一个副本写入 `expired` 并恢复原任务。

### 上下文与媒体

| 方法 | 路径                           | 身份                     |
| ---- | ------------------------------ | ------------------------ |
| GET  | `/api/tapp/context/app`        | 可选认证                 |
| GET  | `/api/tapp/context/user`       | 可选认证                 |
| GET  | `/api/tapp/context/player`     | 可选认证                 |
| GET  | `/api/tapp/context/navigation` | 可选认证                 |
| GET  | `/api/tapp/context/system`     | 可选认证                 |
| GET  | `/api/tapp/context/geo`        | 公开                     |
| POST | `/api/tapp/media/control`      | 可选认证 + 权限          |
| GET  | `/api/tapp/media/status`       | 可选认证 + 权限          |
| POST | `/api/tapp/notifications`      | 登录 + `ui:notification` |

Tapp 通知进入 Myriad 的统一通知流，不存在独立的 Tapp-only toast 通道。

### 报告、组件、快捷键和事件

| 方法           | 路径                                                          |
| -------------- | ------------------------------------------------------------- |
| POST           | `/api/tapp/reports`                                           |
| GET            | `/api/tapp/report-catalog`                                    |
| GET            | `/api/tapp/report-catalog/{reportId}`                         |
| GET            | `/api/tapp/report-catalog/platform/{platform}`                |
| GET            | `/api/tapp/reports/tapp/{tappId}`                             |
| GET/PUT/DELETE | `/api/tapp/reports/{tappId}/{reportId}`                       |
| POST           | `/api/tapp/components/register`                               |
| DELETE         | `/api/tapp/components/{tappId}/{componentType}/{componentId}` |
| GET            | `/api/tapp/components/{tappId}`                               |
| GET            | `/api/tapp/components/all/{componentType}`                    |
| POST           | `/api/tapp/shortcuts/register`                                |
| DELETE         | `/api/tapp/shortcuts/{tappId}/{shortcutId}`                   |
| GET            | `/api/tapp/shortcuts`                                         |

这些路由都需要登录；具体能力还受 `report:*`、`component:*` 与 `shortcut:register` 等最终授权约束。

### 指标与限流

| 方法 | 路径                            |
| ---- | ------------------------------- |
| GET  | `/api/tapp/metrics`             |
| GET  | `/api/tapp/rate-limit/{tappId}` |

不要依赖旧文档中的固定“每分钟 N 次”和 `X-RateLimit-*` 表格；实际限制由当前后端配置、
用户角色和具体 handler 决定。窗口计数位于 PostgreSQL 共享 registry，以
`subject + Tapp + operation` 隔离并原子递增；所有后端副本共用同一额度。registry 不可用时
受限操作返回 `503 RATE_LIMITER_UNAVAILABLE`，不会绕过限制。

## 调度器

全部需要登录；注册还检查 `scheduler:register`、Tapp 所有权、scope 和 backendActions。
AI backendAction 还要求 Manifest AI V2 `generate` 声明；每次执行前重验安装授权，并进入统一
AI Task registry 与配额账本。
`fetch` 使用公网 DNS 钉扎、禁止重定向和 2 MiB 流式响应上限。

| 方法     | 路径                                                  | 说明                        |
| -------- | ----------------------------------------------------- | --------------------------- |
| GET      | `/api/tapp/scheduler/tasks?tapp_id=...`               | 当前用户任务列表            |
| POST     | `/api/tapp/scheduler/tasks`                           | 注册任务                    |
| GET      | `/api/tapp/scheduler/{tappId}/tasks`                  | 指定 Tapp 任务              |
| GET      | `/api/tapp/scheduler/{tappId}/tasks/{taskId}`         | 单个任务                    |
| DELETE   | `/api/tapp/scheduler/{tappId}/tasks/{taskId}`         | 注销任务                    |
| POST     | `/api/tapp/scheduler/{tappId}/tasks/{taskId}/enable`  | 启用                        |
| POST     | `/api/tapp/scheduler/{tappId}/tasks/{taskId}/disable` | 禁用                        |
| POST     | `/api/tapp/scheduler/{tappId}/tasks/{taskId}/trigger` | 手动触发                    |
| GET (WS) | `/api/tapp/scheduler/ws`                              | frontend 任务推送与完成回执 |

注册请求使用 snake_case（SDK 会转换）：

```json
{
  "tapp_id": "com.example.app",
  "task_id": "daily-sync",
  "name": "每日同步",
  "schedule_type": "daily",
  "schedule": { "time": "09:00" },
  "execution_target": "frontend",
  "missed_policy": "run-once",
  "scope": "user",
  "retry": { "maxRetries": 2, "retryDelay": 5000 }
}
```

`execution_target` 为 `backend` 或 `both` 时必须提供非空 `backend_actions`；global scope 仅
当前仍为管理员的用户可注册。frontend 执行通过 WS 下发，直到沙箱回调上报完成前，执行
记录保持 running。`maxRetries` 范围为 0–2，`retryDelay` 范围为 0–60000 ms（0 按 1 秒执行）；
单个任务最多 8 个串行 `backend_actions`。领取租约还会计入最多 5 次补偿执行，按这些边界
动态计算为 15–360 分钟。

## Manifest 声明 API

| 方法 | 路径                               | 说明                          |
| ---- | ---------------------------------- | ----------------------------- |
| GET  | `/api/tapp/{tappId}/apis`          | 列出当前上下文可用的命名 API  |
| POST | `/api/tapp/{tappId}/api/{apiName}` | 执行 Manifest `apis[apiName]` |

请求体：

```json
{ "params": { "city": "Tokyo" } }
```

这不是任意 URL 代理。API 名必须存在于 Manifest：`public` 可在允许的游客上下文使用，
`protected` 默认需要 `network:fetch`。后端负责模板注入、出站安全、SSRF 防护、builtin
路由和上下文隔离缓存。

旧的 `POST /api/tapp/{tappId}/proxy` 不存在，也不应重新引入。

## 修改路由时的同步项

新增或修改端点时同时检查：

1. 后端路由注册、中间件和 handler 内授权；
2. `frontend/src/tapp/services/TappApiService.ts` 的 Cookie、CSRF 和字段转换；
3. sandbox handler 与 `permissionConfig.ts`；
4. `sdkGenerator.ts` 的公开方法；
5. 本文和 [SDK API](API_REFERENCE.md)；
6. Page、Widget、headless 三种宿主是否都应暴露该能力。
