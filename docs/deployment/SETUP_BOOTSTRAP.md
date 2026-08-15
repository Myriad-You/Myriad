# Setup 引导令牌与安装暗号

两把宿主凭证，用途不同：

| 凭证 | 何时要 | 放哪 |
| --- | --- | --- |
| **安装暗号** `MYRIAD_SETUP_SECRET` | 编排已经写好数据库、创建第一个所有者 | `.env` / compose 环境 |
| **引导令牌** `.bootstrap-token` | 已经配过、库又挂了，要改 setup | 与 `.env` 同级的文件 |

都不是给第三方调用的 API key，也不进高级设置。

相关代码：`backend/src/api/setup_bootstrap.rs`。

---

## 安装暗号（抢所有者）

只在**编排里已经写好数据库**时才要。这时栈一启动就是 FULL MODE，向导跳过「连库」直接建所有者，谁先点谁就是站长。

向导自己填数据库的路径不预置这枚值，也不显示这个框。过了连库那一步就算过了门槛。

- 进程环境里有非空 `MYRIAD_SETUP_SECRET` → 创建所有者必须对上（向导框 / `X-Setup-Secret` / JSON `setup_secret`）
- 没配这枚值 → 放行
- **编排生成器**（`com.myriad.config-generator`）和 `deploy.sh` 会写入并注入 backend
- 向导的 `init-env` / `database-config` **不会**再自动生成
- HTTP 响应和启动日志都不回传正文

```bash
# Docker / 编排产物
grep '^MYRIAD_SETUP_SECRET=' .env
```

配了但填错是 **401**。没配则不会出现这个框。

---

## 引导令牌（破窗恢复）

引导令牌只在**已经配过的实例**再次进入配置模式时出现，用来证明「你能读到宿主文件 / 预置 Secret」。

向导里对应字段：数据库那一步的「引导令牌」。

---

## 要解决什么

`CONFIG_MODE` 会在下面两种情况打开：

- 进程环境里没有 `DATABASE_URL`
- 有 `DATABASE_URL`，但连不上 PostgreSQL（重试失败后自动降级）

配置模式下，setup 路由可以改写 `.env` 里的 `DATABASE_URL` / `JWT_SECRET` / `CORS_ORIGINS`。  
若实例此前已经跑起来过，这次只是库暂时不可达，把这些端点重新对端口开放，等于把一次数据库故障升级成匿名控制面。

因此：

| 情形 | 判定 | 改配置 |
| --- | --- | --- |
| 首次安装 | 磁盘 `.env` 和进程环境里都没有真实 `DATABASE_URL` | 不需要令牌，向导直接开放 |
| 已配置实例进入 CONFIG_MODE | 进程环境或 `.env` 里已有真实 `DATABASE_URL` | 必须带 `X-Bootstrap-Token` |

「真实」指不是空值、不是 `postgres://` / `postgresql://` 这种半截串，也不含 `your_password` / `changeme` / `<...>` 这类模板占位。

生产 Docker 栈几乎总会通过 compose 注入真实 `DATABASE_URL`。所以 **postgres 挂了之后，backend 进 CONFIG_MODE 时默认就要令牌**。

---

## 令牌从哪来

进入 CONFIG_MODE 且判定为「已配置」时，backend 会：

1. 若环境变量 `MYRIAD_BOOTSTRAP_TOKEN` 非空，就用它；
2. 否则生成 48 位字母数字令牌；
3. 写到进程工作目录下、与 `.env` 同级的 **`.bootstrap-token`**，权限 `0600`（仅属主可读）。

**启动日志不会打印令牌正文**，只提示文件路径和请求头名。集中式日志 / 容器 stdout 不能当取令牌的地方。

| 部署 | 工作目录 | 默认文件 |
| --- | --- | --- |
| 原生（本文 `/opt/myriad`） | `WorkingDirectory=/opt/myriad` | `/opt/myriad/.bootstrap-token` |
| Docker 生产栈 | 容器内 `/app` | **`/app/.bootstrap-token`**（在容器可写层，不在 `backend_data` 卷上） |
| 本地 `cargo run` | 通常是 `backend/` | `backend/.bootstrap-token` |

Docker 注意：`/app/.bootstrap-token` **不会**跟 `backend_data` 一起持久化。容器被删掉重建后，若没有预置 `MYRIAD_BOOTSTRAP_TOKEN`，会生成**新**令牌。需要跨重建稳定下来时，把令牌写进宿主 Secret / `.env`，再注入 backend 环境（见下方）。

当前 `docker-compose.yml` **不会**自动注入 `MYRIAD_BOOTSTRAP_TOKEN`。要预置，自己加到 backend 的 `environment`。

---

## 怎么用

### 1. 读出令牌

原生：

```bash
sudo cat /opt/myriad/.bootstrap-token
```

Docker：

```bash
docker compose exec backend cat /app/.bootstrap-token
```

启动日志只提示路径和 header 名，不会带令牌正文：

```bash
docker compose logs backend | rg -n "REQUIRE a bootstrap token|\\.bootstrap-token"
```

### 2. 填进向导

打开站点（CONFIG_MODE 下会进初始化向导）→ 数据库步骤 →「引导令牌」→ 粘贴 → 保存并连接。

未填或填错时，保存会返回 **401**，文案提示去读 `.bootstrap-token`。

### 3. 直接打 setup API

受令牌保护的改写端点：

- `POST /api/setup/database-config`
- `POST /api/setup/init-env`
- `POST /api/setup/update-env`

```bash
curl -X POST "https://yourdomain.com/api/setup/database-config" \
  -H "Content-Type: application/json" \
  -H "X-Bootstrap-Token: $(sudo cat /opt/myriad/.bootstrap-token | tr -d '\n')" \
  -d '{"host":"postgres","port":5432,"database":"myriad","username":"myriad","password":"..."}'
```

Docker 里把 `sudo cat ...` 换成 `docker compose exec -T backend cat /app/.bootstrap-token`。

只读的 `/api/setup/status`、`/api/setup/config`、`/health` **不**需要这枚令牌。

---

## 预置固定令牌

适合 k8s Secret、1Panel 环境变量、不想每次重建容器都换令牌的场景：

```env
MYRIAD_BOOTSTRAP_TOKEN=<用 openssl rand -base64 36 生成，去掉换行>
```

Docker 需要同时写进宿主 `.env`，并在 backend 服务里显式注入，例如：

```yaml
# docker-compose.yml 的 backend.environment
MYRIAD_BOOTSTRAP_TOKEN: ${MYRIAD_BOOTSTRAP_TOKEN}
```

然后 `bash scripts/docker/deploy.sh up`（或等价的 compose up）。空值会被忽略，仍会生成文件令牌。

---

## 不要做什么

- **不要**把安装暗号或引导令牌做到高级设置、诊断 JSON、`/health` 或任何已登录 API 里。能进后台 ≠ 能上机器。
- **不要**当长期自动化凭证。实例正常时改配置走管理员登录；引导令牌只用于破窗。
- **不要**贴进 issue、聊天、CI 日志。暗号泄露后改 `.env` 里的 `MYRIAD_SETUP_SECRET` 并重启；令牌泄露后改 `MYRIAD_BOOTSTRAP_TOKEN` 或删掉 `.bootstrap-token` 后重启。
- **不要**和 `UPDATE_TOKEN` / `UPDATER_GATEWAY_SECRET` / `JWT_SECRET` 混用。它们各管各的面。

---

## 排错

| 现象 | 原因 / 处理 |
| --- | --- |
| 向导创建所有者返回 401 | 编排预置了 `MYRIAD_SETUP_SECRET` 但对不上。从宿主 `.env` 复制。向导自己填库时不该出现这个框。 |
| 向导保存数据库返回 401 | 已配置实例缺引导令牌或令牌不对。读 `.bootstrap-token` 再贴一次。 |
| 日志里找不到令牌 | 正常。只看路径提示，去文件或 `MYRIAD_BOOTSTRAP_TOKEN` 取。 |
| `docker compose exec` 没有这个文件 | 当前进程可能仍被判定为首次安装（`DATABASE_URL` 是占位符），或不在 CONFIG_MODE。先 `docker compose logs backend` 确认是否出现 “already configured” / “REQUIRE a bootstrap token”。 |
| 重建容器后旧令牌失效 | 未预置时每次新进程会重新生成。改用 `MYRIAD_BOOTSTRAP_TOKEN`，或读新文件。 |
| 首次安装也被要令牌 | 进程环境里已经有真实 `DATABASE_URL`（compose 常见）。这是按「已配置」处理的。要么带令牌改库，要么先修 postgres 让它回到 FULL MODE。 |
| 填了令牌仍 403 | 那是 `CONFIG_MODE=false` 的另一道锁（实例已在 FULL MODE）。引导令牌过不了这道；正常运行时不要用 setup 改库。 |

---

## 和其它密钥的关系

| 凭证 | 用途 | 出现位置 |
| --- | --- | --- |
| `MYRIAD_SETUP_SECRET` | 编排预置库时，创建第一个所有者对暗号 | `.env` / compose，**值不进前端** |
| `.bootstrap-token` / `MYRIAD_BOOTSTRAP_TOKEN` | 已配置实例在 CONFIG_MODE 下改 setup | 宿主文件 / 你注入的 env，**不进前端** |
| `JWT_SECRET` | 登录会话签名 | `.env` |
| `UPDATE_TOKEN` | updater / gateway / docker-guard | 不进 backend |
| `UPDATER_GATEWAY_SECRET` | backend → updater-gateway | backend + gateway |

日常更新仍走 `/config → 关于 → 更新管理`。库挂了进向导，才轮到引导令牌。
