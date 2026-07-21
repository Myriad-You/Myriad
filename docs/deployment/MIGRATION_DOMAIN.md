# 更换站点公网域名（非联邦）

本文说明**站点访问身份**迁移：把对外的 HTTPS origin 从旧域名换到新域名时，Myriad 会改写哪些配置、哪些必须手工完成。

> **这不是联邦 Move。**  
> ActivityPub / MFP 身份与 `federation_*` 表的迁移由单独的 federation domain-move 流程负责（见下文指针）。  
> 本流程只处理站点访问：`BASE_URL`、`FRONTEND_URL`、`CORS_ORIGINS`、OAuth 回调基址等。

英文对照名：**site public identity migration**（CORS / env / OAuth consoles），**not** federation Move.

## 适用场景

- 自托管站点从 `https://old.example` 迁到 `https://new.example`
- 仅改公网域名 / 证书 / 反代，不涉及联邦 actor 重写
- Docker Compose 或裸机部署均可；`.env` 与配置库中的 `base_url` 需一致

## 自动完成（Admin UI / API）

管理员可通过：

1. **设置 → 基础配置 → 站点公网地址 / BASE_URL**  
   普通保存配置时，若 `base_url` 为合法 origin，后端会同步：
   - 配置库 `configurations.base_url`
   - `.env`：`BASE_URL`、`FRONTEND_URL`（与新 origin 相同）
   - `.env`：`CORS_ORIGINS`（见下方合并规则）
2. **设置 → 更换域名** 面板  
   或直接调用：

```http
POST /api/admin/site/domain
Content-Type: application/json

{
  "new_origin": "https://new.example",
  "previous_origin": "https://old.example"
}
```

成功时返回：

- `applied.base_url` / `applied.frontend_url` / `applied.cors_origins`
- `checklist`：DNS / TLS / 反代 301 / OAuth 回调 / 后端重启（CORS）/ 联邦另做

### Origin 校验

- 必须是 origin 形态：`https://host[:port]`，**不要**带路径、查询或 fragment
- 生产使用 `https`；`http` **仅**允许 `localhost` / `127.0.0.1` / `::1`
- 禁止 `*` 通配

### CORS_ORIGINS 合并规则

| 当前 CORS 列表 | previous | new | 结果 |
| --- | --- | --- | --- |
| 含 previous | 有 | 新 origin | 将该项替换为 new，保留其它 origin |
| 不含 previous | 有或无 | 新 origin | 追加 new（若尚未存在） |
| 空或仅 `*` | — | 新 origin | 变为单独的 new（**从不**保留 `*`） |

空 `base_url` **不会**清空 `CORS_ORIGINS`（生产环境缺少该变量会 panic）。

### 热更新边界

- 保存后会 `dotenvy` 重载并设置 `CONFIG_RELOAD_REQUESTED`（与既有配置保存一致）
- 动态配置缓存中的 `base_url` 会更新（OAuth URL 构建等）
- **HTTP `CorsLayer` 在进程启动时构建**：要让浏览器侧 CORS 真正生效，仍需**重启 backend**（或整栈 recreate）。清单项 `backend_restart_for_cors` 即指此

## 手工完成

| 步骤 | 说明 |
| --- | --- |
| DNS | 新主机名 A/AAAA 或 CNAME 指向本机 / LB |
| TLS | 为新主机名签发或挂载证书（反代 / ACME） |
| 反代 301/308 | 旧 origin → 新 origin，避免书签与爬虫死链 |
| OAuth 控制台 | GitHub / Google / Microsoft 等应用的 **Authorization callback URL** 改为新 origin 下路径 |
| 后端重启 | 使 `CORS_ORIGINS` 进入 CorsLayer |
| 联邦（可选） | 若该站参与联邦，另走 federation domain-move；**不要**指望本 API 改写联邦表 |

### 建议顺序

1. 先配好 DNS + TLS，确认新域名已能打到反代  
2. Admin 调用更换域名 API（或保存 BASE_URL）  
3. 更新各 OAuth 应用回调  
4. 重启 backend / `docker compose up -d`  
5. 旧域名加 301  
6. 若需要联邦身份迁移，再执行 federation Move

## Updater / 运维侧

后端已能写的 env 键与 Admin API 相同。可选 reconcile：在 updater 或运维脚本中仅重写：

- `BASE_URL`
- `FRONTEND_URL`
- `CORS_ORIGINS`

规则与 `backend/src/api/site_domain.rs` 中的 `merge_cors_origins` / `validate_and_normalize_origin` 一致即可。  
**不要**在 updater 路径里改 federation 表。

## 与联邦迁移的关系

| | 本流程（站点） | 联邦 domain-move |
| --- | --- | --- |
| 目标 | 浏览器 / CORS / OAuth / 公网 URL | Actor ID、inbox、签名身份 |
| 写 `.env` 三键 | 是 | 可能另有约定，不属本 PR |
| `federation_*` 表 | **否** | 是 |
| ActivityPub Move | **否** | 是 |

文档指针（实现到位后）：

- [FEDERATION_DOMAIN_MOVE.md](./FEDERATION_DOMAIN_MOVE.md)（若尚未合入，以联邦 Move PR / 设计为准）

## 相关代码

- `backend/src/api/site_domain.rs` — 校验、CORS 合并、Admin API
- `backend/src/api/config.rs` — 通用保存路径在 `base_url` 变更时适配 FRONTEND_URL / CORS
- `frontend/src/components/config/UiConfigSection.tsx` — 站点 URL + 更换域名面板
- 生产启动：`CORS_ORIGINS` 为空时 panic（见 `backend/src/main.rs`）— 本流程刻意避免写出空 CORS

## 快速检查清单

- [ ] 新域名 DNS / TLS 已就绪  
- [ ] Admin 已应用新 origin，`.env` 中三键一致  
- [ ] OAuth 回调已改  
- [ ] 已重启 backend  
- [ ] 旧域名 301  
- [ ] （可选）联邦 Move 另开任务  
