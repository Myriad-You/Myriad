# Myriad 文档门户

## 快速导航

| 目标 | 文档 |
| --- | --- |
| 部署 / 本地跑起来 | [QUICKSTART.md](../QUICKSTART.md) |
| 生产 Docker 拓扑 | [DOCKER_DEPLOYMENT.md](../deployment/DOCKER_DEPLOYMENT.md) |
| 端口与暴露面 | [PORTS.md](../deployment/PORTS.md) |
| 系统架构 | [ARCHITECTURE.md](../development/ARCHITECTURE.md) |
| 从源码构建 | [BUILD.md](../development/BUILD.md) |
| HTTP API 入口 | [API.md](../API.md) |
| Updater 运维 | [UPDATER_QUICKSTART.md](../UPDATER_QUICKSTART.md) |
| Setup 安装暗号 | [SETUP_BOOTSTRAP.md](../deployment/SETUP_BOOTSTRAP.md) |
| 无 Docker 部署 | [NATIVE_DEPLOYMENT.md](../deployment/NATIVE_DEPLOYMENT.md) |
| Tapp 开发 | [TAPP_DEVELOPMENT.md](../development/TAPP_DEVELOPMENT.md) |

## 文档树

```
docs/
├── QUICKSTART.md
├── API.md
├── UPDATER_QUICKSTART.md
├── updater-spec.md
├── notification-catalog.md
├── deployment/
│   ├── DOCKER_DEPLOYMENT.md
│   ├── EXTERNAL_POSTGRES.md
│   ├── PORTS.md
│   ├── UPDATER_SECURITY_BASELINE.md
│   ├── SETUP_BOOTSTRAP.md           # 安装暗号
│   ├── NATIVE_DEPLOYMENT.md         # 无 Docker 生产部署
│   ├── MIGRATION_DOMAIN.md          # 非联邦换域名
│   ├── FEDERATION_DOMAIN_MOVE.md    # 联邦 Move
│   └── examples/
│       └── docker-compose.external-db.example.yml
├── development/
│   ├── ARCHITECTURE.md
│   ├── BUILD.md
│   ├── FEDERATION.md
│   ├── OAUTH.md
│   ├── TAPP_DEVELOPMENT.md
│   └── tapp/                        # Tapp 参考（SDK / Store / Playground …）
├── features/
│   ├── LIBRARY.md
│   └── TAPP_FILE_FORMAT.md
├── design/
│   ├── digital-life-3d-pipeline.md   # Tripo 3D 管线
│   ├── icon-inventory.md
│   └── theme-system.md
├── guides/
│   └── SECURITY_HEADERS.md
└── quick-reference/
    └── INDEX.md                     # 本页
```

## 仓库根（相关）

```
Myriad/
├── docker-compose.yml           # 生产：proxy + updater + guard
├── docker-compose.dev.yml       # 开发：仅 PostgreSQL
├── .env.production.example
├── backend/ · frontend/ · proxy/ · updater/
├── crates/ · shared/ · docker/ · scripts/ · release/ · tools/
└── README.md
```

## 分类索引

### 部署与运维

| 文档 | 说明 |
| --- | --- |
| [QUICKSTART.md](../QUICKSTART.md) | 生产与开发快速开始 |
| [DOCKER_DEPLOYMENT.md](../deployment/DOCKER_DEPLOYMENT.md) | 三网拓扑、env、健康检查 |
| [EXTERNAL_POSTGRES.md](../deployment/EXTERNAL_POSTGRES.md) | 外部数据库 |
| [PORTS.md](../deployment/PORTS.md) | 端口与代理路径 |
| [UPDATER_QUICKSTART.md](../UPDATER_QUICKSTART.md) | 更新 / 回滚 / 救援 |
| [updater-spec.md](../updater-spec.md) | Updater 协议与失败模式 |
| [UPDATER_SECURITY_BASELINE.md](../deployment/UPDATER_SECURITY_BASELINE.md) | Updater 安全基线 |
| [SETUP_BOOTSTRAP.md](../deployment/SETUP_BOOTSTRAP.md) | 编排安装时的安装暗号 |
| [NATIVE_DEPLOYMENT.md](../deployment/NATIVE_DEPLOYMENT.md) | 无 Docker 生产部署 |
| [MIGRATION_DOMAIN.md](../deployment/MIGRATION_DOMAIN.md) | 换域名（非联邦） |
| [FEDERATION_DOMAIN_MOVE.md](../deployment/FEDERATION_DOMAIN_MOVE.md) | 联邦域名 Move |

### 开发

| 文档 | 说明 |
| --- | --- |
| [ARCHITECTURE.md](../development/ARCHITECTURE.md) | 组件与拓扑 |
| [BUILD.md](../development/BUILD.md) | 工具链与构建 |
| [API.md](../API.md) | HTTP API 入口 |
| [OAUTH.md](../development/OAUTH.md) | 本地登录与 OAuth/OIDC |
| [FEDERATION.md](../development/FEDERATION.md) | 联邦行为笔记 |
| [TAPP_DEVELOPMENT.md](../development/TAPP_DEVELOPMENT.md) | Tapp 文档索引 |
| [tapp/](../development/tapp/) | Manifest / SDK / Store / Playground / 排错 |

### 功能与设计

| 文档 | 说明 |
| --- | --- |
| [LIBRARY.md](../features/LIBRARY.md) | 资料库 |
| [TAPP_FILE_FORMAT.md](../features/TAPP_FILE_FORMAT.md) | `.tapp` 包格式 |
| [digital-life-3d-pipeline.md](../design/digital-life-3d-pipeline.md) | Digital Life / Tripo 3D 管线 |
| [theme-system.md](../design/theme-system.md) | Surface / Glow |
| [icon-inventory.md](../design/icon-inventory.md) | 图标规范 |
| [notification-catalog.md](../notification-catalog.md) | 通知目录 |
| [SECURITY_HEADERS.md](../guides/SECURITY_HEADERS.md) | 安全响应头 |

## 常用命令

```bash
# 生产
bash scripts/docker/deploy.sh up
bash scripts/docker/deploy.sh status

# 开发（默认本机 PostgreSQL；`--docker` 改用 compose postgres）
./scripts/dev/dev.sh start
```

Windows：`.\scripts\docker\deploy.ps1 up`、`.\scripts\dev\dev.ps1 start`。
