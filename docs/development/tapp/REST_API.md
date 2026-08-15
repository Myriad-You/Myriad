# Tapp REST API

本文记录当前宿主调用的 Tapp 后端路由。第三方 Tapp 代码通常应使用
[Tapp SDK](API_REFERENCE.md)，不要直接拼接这些 URL；SDK 会处理身份、CSRF、字段转换、
权限预检和错误解包。

架构与安全边界见 [Tapp 架构](ARCHITECTURE.md)。路由权威来源是
`backend/src/router/authenticated.rs`（及同目录 router 组装）、
`backend/src/api/tapp_store.rs` 和 `backend/src/api/tapp_scheduler.rs`。

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
| GET  | `/api/tapps/list-card-sizes`    | 列表页卡片尺寸/顺序（见下「列表布局」）      |
| GET  | `/api/tapps/{tappId}`           | Tapp 详情、Manifest、状态和最终授权          |
| GET  | `/api/tapps/{tappId}/code`      | 主代码文本                                   |
| GET  | `/api/tapps/{tappId}/resources` | 代码、CSS、HTML、i18n、Page 模块等资源对象   |
| GET  | `/api/tapps/{tappId}/asset?path=` | Manifest 声明的包内资源（base64）          |
| GET  | `/api/tapps/{tappId}/export`    | 导出 `.tapp` ZIP                             |

读取与运行时授权优先当前用户的私有安装；未安装私有副本时再使用站点公开（管理员）安装。
公开与私有副本可双向并存；安装冲突只检查操作者可修改的 owner 命名空间。

Storage 按当前 **subject**（持久用户或签名游客 session）命名空间隔离：打开公开安装时，
每个 subject 读写自己的 `user_id + tapp_id` 数据，不会读取站点 owner 的 storage。
`storage:read` / `platform:read` 可进入访客 Runtime Grant（见下「Widget 与存储」）。
安装级 Manifest 设置仍由安装 owner 或管理员写入；能打开该安装的运行者（含游客）可读已
保存的声明键。

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

### 可选认证的 subject 路由

以下路由挂在 `optional_auth` 上：始终注入真实用户 Claims 或稳定 guest Claims。

| 方法 | 路径                                 | 说明 |
| ---- | ------------------------------------ | ---- |
| GET  | `/api/tapps/recent?limit=10`         | 可选认证：按 **当前 subject**（`tapp_user_activities.user_id`）返回最近运行；`limit` 默认 10（1–50）。游客 subject 无 start 活动，结果为 `[]`。 |
| GET  | `/api/tapps/{tappId}/settings`       | **读**安装级 Manifest 设置（见下节「Settings 读/写」） |
| GET  | `/api/tapps/{tappId}/settings/{key}` | **读**单个声明键 |

### 需要登录的变更路由

| 方法   | 路径                                 | 说明                                    |
| ------ | ------------------------------------ | --------------------------------------- |
| POST   | `/api/tapps/install`                 | direct/store 统一安装                   |
| POST   | `/api/tapps/install-file`            | multipart 上传 `.tapp`，字段名 `file`   |
| POST   | `/api/tapps/cleanup-temporary`       | 私有安装清理（见下「私有安装清理」）    |
| PUT    | `/api/tapps/list-card-sizes`         | 写入**当前登录用户**个人列表布局        |
| POST   | `/api/tapps/{tappId}/update`         | direct/store 更新，保留用户数据         |
| POST   | `/api/tapps/{tappId}/start`          | 持久化 owner 自己的 running 状态        |
| POST   | `/api/tapps/{tappId}/stop`           | 停止 owner 安装并撤销对应 Runtime Grant |
| DELETE | `/api/tapps/{tappId}?keep_data=true` | 卸载；可选保留存储/设置                 |

### 列表布局 `/api/tapps/list-card-sizes`

宿主 Tapp **列表页**卡片尺寸（`1x1` | `2x1`）与拖拽顺序。存于
`users.tapp_list_card_sizes`；**不是**安装级 Manifest 设置。

| 方法 | 路径 | 身份 | 说明 |
| ---- | ---- | ---- | ---- |
| GET | `/api/tapps/list-card-sizes` | 可选认证 | 见响应语义 |
| PUT | `/api/tapps/list-card-sizes` | **登录**（持久用户；游客 403） | body：`{ sizes, order }` |

GET 响应字段名与 handler JSON **一致**（`sizes` / `order` 为短名；站点副本为
**snake_case** 的 `site_sizes` / `site_order`，**不是** camelCase）：

```json
{
  "success": true,
  "sizes": { "com.example.app": "2x1" },
  "order": ["com.example.app"],
  "site_sizes": { "com.myriad.notes": "1x1" },
  "site_order": ["com.myriad.notes"],
  "source": "viewer",
  "writable": true
}
```

游客示例：`source` 为 `"site_owner"`，`writable` 为 `false`；`sizes`/`order` 与
`site_sizes`/`site_order` 均为站点 owner 布局。

| 字段 | 语义 |
| ---- | ---- |
| `sizes` / `order` | **主视图布局**：游客 = 站点 owner 布局；已登录 = **纯个人**偏好（**不**用 owner 布局填洞） |
| `site_sizes` / `site_order` | 站点 owner（规范公开管理员）布局，供列表「站点」范围展示 |
| `source` | `viewer` \| `site_owner`（主视图数据来自哪一侧；**不是** `personal`） |
| `writable` | 是否允许 PUT（游客 `false`；持久登录用户 `true`） |

PUT body 只写调用者个人行：`{ "sizes": { "<tappId>": "1x1"|"2x1" }, "order": ["<tappId>", ...] }`。
非法尺寸键会被忽略；未知 Tapp id 可暂存，列表 hydrate 时按可见集合过滤。

前端：`TappListCardSizesApi` + `TappListPage`（mine / site 范围、拖拽排序、卸载后清理预设）。

### 私有安装清理 `POST /api/tapps/cleanup-temporary`

由站点配置 `tapp_private_install_cleanup` 驱动（管理端 OAuth/权限页）：

| 模式 | 本端点 | 每日后台 worker |
| ---- | ------ | --------------- |
| `logout`（可选） | **仅卸载当前调用者**的私有安装；失败记日志，不阻塞登出 | **no-op**（只在 mode=`inactivity` 时 prune） |
| `inactivity`（默认） | **no-op** | 全局按不活跃天数 prune 私有安装 |

相关配置：

- `tapp_private_install_cleanup`: `"logout"` \| `"inactivity"`
- `tapp_private_install_inactivity_days`: 正整数（worker 用 `COALESCE(last_login_at, last_seen_at, created_at)`）

**禁止**在用户登出路径上跑全局 prune（避免误删他人私有安装或拖慢 logout）。
`logout` 模式下 worker 也不会 prune。前端登出会调用本端点；卸载确认对话框与清理预设文案见
`UninstallConfirmDialog` / 安装流程 `InstallTappDialog`。

直接安装请求：

```json
{
  "source": "direct",
  "manifest": {
    "id": "com.example.app",
    "name": "App",
    "version": "1.0.0",
    "category": "utility",
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
  "permissions": ["storage:read"]
}
```

`permissions` 是用户同意的申请子集，不是当前有效授权；后端先与 Manifest 求交集并保存为
`approved_permissions`，再按当前实时角色和动态权限配置生成对调用者可见的有效权限。

安装/更新资源先进入 staging，校验后原子替换在线目录；数据库失败恢复旧目录。卸载把文件
移入隔离目录后，在一个事务中清理安装记录、Manifest/动态 Widget、调度任务及执行历史；
`keep_data=true` 只保留 storage，不保留任务或 Widget。相同公开 `tappId` 的最终冲突复核、
文件切换和数据库变更由 PostgreSQL advisory transaction lock 串行化，跨后端副本也不能并发覆盖。
激活目录包含与数据库 `updated_at` 对应的内部代际标记；启动时会恢复被进程退出打断的
rename/commit 窗口，该标记不会进入导出的 `.tapp` 包。
所有当前管理员都操作首次站点管理员对应的规范公开 owner；普通用户仍写入自己的临时 owner。
管理员身份不会让 Runtime 自动读取其他普通用户的私有安装。

### Runtime Grant

以下路由使用可选认证，因此游客运行管理员共享 Tapp 时也能获得绑定稳定 guest subject 的
Grant；游客仍不能借此访问普通用户的临时安装。

| 方法   | 路径                                             | 说明                         |
| ------ | ------------------------------------------------ | ---------------------------- |
| POST   | `/api/tapps/{tappId}/runtime-grants`             | 为 Page/Widget/headless 签发 |
| POST   | `/api/tapps/{tappId}/runtime-grants/authorize`   | 宿主本地敏感能力实时复核     |
| DELETE | `/api/tapps/{tappId}/runtime-grants/{runtimeId}` | 销毁实例时撤销               |

签发 body 为 `{ "instanceId": "page_...", "kind": "page" }`；`kind` 还可以是
`widget` 或 `headless`。令牌只返回宿主一次，服务端仅存 SHA-256。停止、更新、卸载会撤销
相关 Grant。Grant 哈希与 TTL 租约存储在 PostgreSQL，可由任意副本校验；只有被撤销或到期的
Grant 才返回 `INVALID_RUNTIME_GRANT`。
每次使用 Grant 时还会重新核对当前可见安装 owner，并以当前角色、动态权限配置和安装授权
收缩权限；owner 改变的旧 Grant 会返回 `INVALID_RUNTIME_GRANT`，由宿主执行一次重签重试。

### Settings 读/写（安装级）

Settings 是 **installation owner** 命名空间上的 Manifest 声明配置，与 subject 私有
`Tapp.storage` 分离。宿主 `Tapp.settings` 走下列 REST（不接受用 Runtime Grant 顶替
会话身份，也不能用 storage key 读写 settings）。

| 方法 | 路径 | 认证层 | 说明 |
| ---- | ---- | ------ | ---- |
| GET | `/api/tapps/{tappId}/settings` | **optional_auth** | 一次读取全部已保存的声明键 |
| GET | `/api/tapps/{tappId}/settings/{key}` | **optional_auth** | 读取单个声明键 |
| POST | `/api/tapps/{tappId}/settings/{key}` | **auth（登录）** | 写入声明键 |

**读（GET）**

- 真实用户与稳定 **guest Claims** 均可调用（与 Runtime Grant / storage 同一 optional_auth
  层）。
- 解析与运行态一致：有私有安装优先私有；否则可读当前 viewer 可见的**公开安装**，返回
  **installation owner** 命名空间下已保存的值。
- 仅接受当前 Manifest 真实声明的 key；未保存的键由 SDK 侧回落到 Manifest `defaultValue`。
- 游客运行公开 Tapp（如后台 Aro）时，应能读到站主已写入的安装设置，而不是一律 401。
- 对 viewer 不可见的安装（例如 `visibility: admin` 对游客）返回 **403**，不是静默空对象。

**写（POST）**

- 仅已登录会话；游客 subject 拒绝。
- 仅安装 owner 或当前管理员可写；值须符合 type / select options / number min-max。
- 写入方法是 `POST`，不是旧文档中的 `PUT`。

不要把 secrets 放进 host settings：公开安装的 GET 对所有能打开该安装的 visitor（含游客）
可读。

### API 凭据（安装级，只写）

| 方法 | 路径 | 认证层 | 说明 |
| ---- | ---- | ------ | ---- |
| GET | `/api/tapps/{tappId}/credentials` | **auth + owner/admin** | 仅返回配置状态、重新授权状态和目标 origins |
| POST | `/api/tapps/{tappId}/credentials/{key}` | **auth + owner/admin** | body 为 `{ "value": "..." }`；加密写入，不回显 |
| DELETE | `/api/tapps/{tappId}/credentials/{key}` | **auth + owner/admin** | 删除凭据，不回显 |

这些路由不接受游客、普通 viewer 或 Runtime Grant 顶替管理身份。`key` 必须由当前 Manifest 的
`credentials` 声明并绑定到至少一个具名 HTTP API。值只在后端执行绑定 API 时按声明加入请求头、
query、form 或仅用于签名；状态还会列出每条绑定的 method、endpoint、`access` 和放置方式。
Manifest 绑定变化后状态会标记需重新授权，运行调用会拒绝使用旧值。
底层复用 installation owner 的 `tapp_storage` 行和现有唯一索引；`_credentials.` 是宿主保留
前缀，密文位于专用字段。通用 storage REST 在 SQL 层排除宿主记录且不查询密文字段，完整
storage entity 也不会序列化密文；数据库约束只允许 `_credentials.*` 行持有凭据密文和授权
指纹，因此通用 storage REST 无法读取、列举、覆盖或清除。

### Widget 与存储

| 方法   | 路径                                     | 说明                                        |
| ------ | ---------------------------------------- | ------------------------------------------- |
| POST   | `/api/tapps/{tappId}/widgets`            | 管理员以 Runtime Grant 注册/更新动态 Widget |
| DELETE | `/api/tapps/{tappId}/widgets/{widgetId}` | 管理员以 Runtime Grant 注销动态 Widget      |
| GET    | `/api/tapps/{tappId}/storage`            | 列出 key                                    |
| DELETE | `/api/tapps/{tappId}/storage`            | 清空存储                                    |
| GET    | `/api/tapps/{tappId}/storage/entries`    | 一次返回全部键值                            |
| GET    | `/api/tapps/{tappId}/storage/usage`      | 一次统计使用字节                            |
| GET    | `/api/tapps/{tappId}/storage/{key}`      | 读取值                                      |
| POST   | `/api/tapps/{tappId}/storage/{key}`      | 写入值                                      |
| DELETE | `/api/tapps/{tappId}/storage/{key}`      | 删除值                                      |

storage 路由要求 optional_auth + Runtime Grant。读取需要 `storage:read`；写入、删除和清空
需要 `storage:write`。`storage:read` 与 `platform:read` 均为 **guest-safe basic**（见 [MANIFEST · 权限](MANIFEST.md) 与
`permission_service::requires_authenticated_subject`）：签名游客 session 可作为 subject，
私有 storage 落在负 id 命名空间下，平台 **读** 走 optional_auth 的公开站点缓存。
通用 storage 使用当前 subject 命名空间，并拒绝访问 `_settings.`、`_component:`、
`_shortcut:`、`_report:` 等宿主保留键。

下列能力的真实后端路由仍要求**持久登录**主体，不会被签入访客 Grant：`report:read`、
`ui:notification`、组件/快捷键注册、scheduler、语音服务、Brew 写入/评论，以及
**privileged / admin-gated** `platform:write`（`POST …/platform/items*`；非仅「已登录」）。
动态 Widget 注册与注销进一步限制为当前管理员。
Manifest Widget 由安装/更新自动对账；动态 Widget 路由要求 `widget:register` 同时存在于
Runtime Grant、安装授权和当前管理员角色，并拒绝覆盖/删除 Manifest 来源的注册。动态行记录
Runtime Grant 的安装 owner，只返回给注册主体；公共安装卸载时会清理绑定该 owner 的动态
Widget 和所有主体为该公共安装创建的 scheduler 任务。
普通用户安装带 Widget 声明的 Tapp 时不会被拒绝；权限过滤只移除其管理员专属的
`widget:register` 能力，应用其余部分继续安装并运行。

Widget 注册 body 除 `id`、`name`、`default_size`、`sizes` 等元数据外，还可包含
`settings` 与 `refresh_policy`；后端会执行与 Manifest 相同的字段、类型、范围和刷新间隔
校验，并将其保存到 Widget 注册记录。

### 商店源管理

| 方法   | 路径                                  | 说明                         |
| ------ | ------------------------------------- | ---------------------------- |
| GET    | `/api/tapps/store/sources`            | 列出全部源（公开；见可选认证读取表） |
| POST   | `/api/tapps/store/sources`            | 添加源；handler 内检查管理员 |
| POST   | `/api/tapps/store/sources/{sourceId}` | 更新源；handler 内检查管理员 |
| DELETE | `/api/tapps/store/sources/{sourceId}` | 删除源；handler 内检查管理员 |

不存在 `/api/tapp-store/...` 路由。

**商店安装语义**（完整协议见 [Tapp 商店](STORE.md)）：

- `source: "store"` 时 `storeSource` 必须是已配置源的 **数字 id**、完整 `index.json` URL，或可规范化匹配到已配置源的 base URL；禁止传字面量 `"store"` / `"direct"`。
- 后端从该源拉 `index.json` 与 `download` / `manifest.assets` 文件，校验索引与 Manifest 的 `category` 一致后进入与 direct 相同的 staging 安装。
- 浏览器侧列表经 `RemoteStoreService` 直连远程索引；后端 502 / 大包（索引 `size` ≥ 1 MiB）时宿主回退为浏览器下载 + `source: "direct"`。
- 官方源由迁移预置：`https://raw.githubusercontent.com/Myriad-You/tapp-store/main/index.json`（`official=true`，URL 不可改）。

## 运行时 `/api/tapp`

这些端点主要由 Bridge handler 经 `TappApiService` 调用。除了路由中间件，handler 还应
校验 Tapp ID、owner、安装批准集与当前动态有效权限。

### 平台、AI 与数据

| 方法   | 路径                                                     | 身份 | SDK 能力                        |
| ------ | -------------------------------------------------------- | ---- | ------------------------------- |
| GET    | `/api/tapp/platform/{platform}/data`                     | 可选认证（`platform:read` + Grant；游客可读站点缓存） | `Tapp.platform.getData`         |
| GET    | `/api/tapp/platform/{platform}/stats`                    | 可选认证（同上） | `Tapp.platform.getStats`        |
| GET    | `/api/tapp/platform/{platform}/distribution/{dimension}` | 可选认证（同上） | `Tapp.platform.getDistribution` |
| POST   | `/api/tapp/platform/items`                               | **登录** | `Tapp.platform.addItem`         |
| POST   | `/api/tapp/platform/items/batch`                         | **登录** | `Tapp.platform.addItems`        |
| POST   | `/api/tapp/ai/v2/tasks`                                  | 可选认证 | 创建 AI 任务                    |
| GET    | `/api/tapp/ai/v2/tasks/{taskId}`                         | 可选认证 | 读取任务快照                    |
| DELETE | `/api/tapp/ai/v2/tasks/{taskId}`                         | 可选认证 | 取消非终态任务                  |
| GET    | `/api/tapp/ai/v2/tasks/{taskId}/events`                  | 可选认证 | SSE token/progress/state        |
| GET    | `/api/tapp/ai/v2/usage`                                  | 可选认证 | 权威 calls/tokens/cooldown      |
| GET    | `/api/tapp/ai/v2/ledger`                                 | 登录     | 宿主 UI 专用，无 SDK 暴露       |
| POST   | `/api/tapp/data/transform`                               | 登录     | `Tapp.data.transform`           |
| GET    | `/api/tapp/analytics/summary`                            | 可选认证 + Grant | `Tapp.analytics.getSummary`（`analytics:read`；见下双 scope） |
| GET    | `/api/tapp/analytics/visitor`                            | 可选认证 + Grant | `Tapp.analytics.getVisitorCard`（`analytics:read`） |

#### 访问统计（Runtime Grant · 双 scope）

实现：`backend/src/api/tapp_runtime/analytics.rs`。均需 `X-Tapp-Runtime-Grant` 且 Grant 含
**`analytics:read`**（basic，guest-safe）。仅返回**聚合**数据，永不含访客哈希、序位
（`your_ordinal_today` / `counted`）等身份字段。

| 方法 | 路径 | 说明 |
| ---- | ---- | ---- |
| GET | `/api/tapp/analytics/summary` | **Admin**：完整汇总（`scope: "admin"`）— `today` / `range` / `daily` / `pages` / `events` / `referrers` / `countries`；`days`（默认 7、上限 365）或 `from`/`to`（`YYYY-MM-DD`）**生效**。**非 admin**（user/guest）：仅访客卡片聚合（`scope: "visitor"`）— `today` / `all_time` / 短 `daily`；**忽略** range query |
| GET | `/api/tapp/analytics/visitor` | 访客卡片精简（始终 `scope: "visitor"`）：今日 / 累计 / 短趋势；无窗口 query |

对非 admin，`GET …/summary` 与 `GET …/visitor` 实质同为 visitor-card 形状；`…/visitor`
仍是专用访客端点。Admin 若只要卡片也可调 `…/visitor`（不会给 pages 等 breakdown）。

**`analytics_enabled` 短路**：站点关闭采集时两路由均返回
`{ "success": true, "enabled": false, "source": "site_analytics" }`，不查表、无 `scope`。
开启时响应含 `enabled: true`、`source: "site_analytics"` 与上表 `scope`。

SDK 与示例见 [API_REFERENCE · 访问统计](API_REFERENCE.md#访问统计-api)。

AI 权限、每分钟速率、每日 calls/tokens 与 cooldown 全由后端执行。配额在模型调用前事务预留、
完成后按实际估算结算、失败/取消释放未消耗 token；calls 仍记录一次尝试。AI Task 还校验
Manifest operation/model tier/context/output 声明，并将任务绑定 subject、安装 owner 和 Tapp。
访客除浏览器 session 配额外，还受匿名 IP 指纹日预算、匿名分钟限流和全站访客日预算约束；
服务端只保存单向摘要，不保存原始 IP。
最终任务注册在 subject advisory-lock 事务中原子检查并发数、保留数和幂等键；未成功注册的
请求完整回滚 calls/token 预留，不会留下只计费但未执行的任务。

除按日聚合的配额计数外，每次受治理的 AI 调用（完成/失败/取消）都会追加一条
`tapp_ai_cost_ledger` 流水：subject、安装 owner、Tapp、任务、来源（runtime 或
internal 宿主适配器）、operation、provider/model、输入/输出 token 估算与终态。账本
append-only、不随每日重置，`GET /api/tapp/ai/v2/ledger` 供登录用户读取本人逐次流水与
按 Tapp 汇总；该端点是宿主 UI 能力，不进入沙箱 SDK。`cost_micro_usd` 列预留给后续
接入定价源，当前为 NULL。

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

### Federation

| 方法 | 路径                         | 身份 | 说明 |
| ---- | ---------------------------- | ---- | ---- |
| GET  | `/api/tapp/federation/feed`  | 可选认证 + Runtime Grant | 需 Grant 含 `federation:read`。游客只返回公开活动（`audience: "public"`）；已登录用户返回公开 Feed 与个人时间线的合并结果（`audience: "public+personal"`，同 `activity_id` 时个人条目优先，整体按时间新到旧，条数有上限）。响应形如 `{ items, total, audience }`。 |
| GET  | `/api/federation/public/rooms/{room_id}` | **无认证** | 仅 `is_public` 群卡片（name、owner、home_server、member_count 等）。**不**走 Runtime Grant / `host_attribution`（与 WebFinger 同类公开发现）。跨实例 `joinRoom` 用此端点物化本地行。 |

联邦写操作、消息、私有 Room 与文件等仍走各自 SDK/宿主路径，且对游客不可用；见
[ARCHITECTURE 所有权与可见性](ARCHITECTURE.md#所有权与可见性) 与
[API_REFERENCE Channel/Room](API_REFERENCE.md#channel--room--ring--trust--传输摘要)。
Brew / 语音 / 联邦 REST 宿主代理路径已统一按 Grant 归因：带 `X-Tapp-Runtime-Grant` 的请求在服务端按路由强制 Tapp
权限并记录归因日志（共享 `host_attribution` 中间件；路由→权限表见
`docs/development/tapp/fixtures/host_route_permissions.json`，**先改 fixture 再改映射**）。
`GET /api/federation/public/rooms/*` **不**列入该表。联邦 E2E 密钥交换与 Channel/Room
WebSocket 升级不能携带 Grant 头，因此 Tapp Bridge 先调用
`POST /api/federation/channels/{channelId}/ws-ticket` 或
`POST /api/federation/rooms/{roomId}/ws-ticket`（要求 `federation:message`），再把一次性
`tapp_ws_ticket` 放入对应升级 URL。票据过期、复用、subject 或目标不匹配都会失败关闭，宿主 UI
不带票据的 Claims-only WebSocket 语义保持不变。独立 AI 费用账本见 `/api/tapp/ai/v2/ledger`。

Room 消息 POST body 上限与 `MESSAGE_PAYLOAD_LIMIT` / `MAX_ROOM_MESSAGE_PAYLOAD`
（**36 MiB**）及联邦 inbox DefaultBodyLimit（**64 MiB**）对齐；`join` 可接受 path 中的
`rm_…@home[:port]`（URL 编码）或 body `{ "home_server": "…" }`。

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
AI backendAction 还要求 Manifest AI `generate` 声明；每次执行前重验安装授权，并进入统一
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

Scheduler WS presence 与 frontend 消息使用 PostgreSQL TTL registry/mailbox，而不是进程内广播。
连接每 20 秒续租，服务端按 connection 原子领取消息；发送中断会重新入队。这样任务 worker 与
WebSocket 位于不同后端副本时仍可投递，每个标签页都能按自己的 Tapp 回调决定是否处理，执行
终态由数据库 CAS 去重。管理员
`GET /api/tapp/metrics` 可查看在线 subject、mailbox 深度以及投递/失败/回退/完成/超时计数。
其中在线 subject 与 mailbox 深度来自共享数据库，`processCounters` 明确只表示当前后端副本，
部署侧应按实例采集后聚合。

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
`protected` 只允许登录主体；所有 HTTP API 都需要实时 `network:fetch`。后端负责共享限流、模板注入、出站安全、SSRF 防护、builtin
路由和上下文隔离缓存。

旧的 `POST /api/tapp/{tappId}/proxy` 不存在，也不应重新引入。

## 入站声明路由

给其他程序用，不走 Runtime Grant，不种游客 Cookie，也**不读取**站点登录 Cookie。
始终解析 `visibility = all` 的公开安装；私有安装和 `visibility = admin` 对 `/tapi` 一律按未安装处理（401 `ROUTE_VERIFY_INVALID`）。

| 方法 | 路径 | 说明 |
| ---- | ---- | ---- |
| GET/POST | `/tapi/{tappId}/{path}` | 执行对应 `apis.*.route`。`HEAD` 不按 GET 处理 |

没有未签名的目录接口。调用方必须事先知道 Manifest 里的 `path` 和验签头，不能靠探测本机安装列表。
未安装、无此 route、方法不对、缺头或 HMAC 错误一律 401 `ROUTE_VERIFY_INVALID`，不区分「有没有这条路由」。

`{path}` 是 Manifest 里去掉前导 `/` 的单段。必须带声明的时间戳、nonce 和 HMAC 头。
缺密钥或绑定指纹过期在 `/tapi` 上也回 401 `ROUTE_VERIFY_INVALID`（不把「这条路由已声明但未配密钥」暴露给探测方；owner 在 Tapp 详情页看凭据状态）。
时间窗外 401 `ROUTE_VERIFY_EXPIRED`；nonce 重放 401 `ROUTE_VERIFY_REPLAY`（这两码只在 HMAC 已经通过之后出现）。
限流：匿名 IP 每分钟 60 次；HMAC 通过后同一凭据每分钟 60 次、每小时 180 次。
拉黑：10 分钟内对同一 Tapp 验签失败 25 次会自动封该调用方指纹 1 小时；全站 10 分钟失败 80 次会封全站入站 1 小时。不保存原始 IP。
owner 可在 Tapp 详情里暂停**该安装**的 `/tapi`，或解除该安装下已列出的指纹。暂停/拉黑按安装 owner 隔离，私有副本不能冻结或解封公开安装。解除只清本安装拉黑，不清全站自动封禁。暂停返回 403 `ROUTE_PAUSED`，拉黑返回 403 `ROUTE_BLOCKED`。
`/tapi` 验签替代 CSRF；带站点 Cookie 的 POST 也不要求 `X-CSRF-Token`。
请求体超过 1 MiB 返回 413 `ROUTE_BODY_TOO_LARGE`。入站密钥泄露后应立刻在详情页轮换。

签名规则见 [Manifest · 入站路由](MANIFEST.md#入站路由-apisroute)。

## 修改路由时的同步项

新增或修改端点时同时检查：

1. 后端路由注册、中间件和 handler 内授权；
2. `frontend/src/tapp/services/TappApiService.ts` 的 Cookie、CSRF 和字段转换；
3. sandbox handler 与 `permissionConfig.ts`；
4. `sdkGenerator.ts` 的公开方法；
5. 本文和 [SDK API](API_REFERENCE.md)；
6. Page、Widget、headless 三种宿主是否都应暴露该能力。
