# Myriad 文档

## 从这里开始

| | |
| --- | --- |
| [快速开始](QUICKSTART.md) | 生产部署与本地开发 |
| [架构](development/ARCHITECTURE.md) | 组件与拓扑 |
| [API](API.md) | HTTP 入口 |
| [Tapp](development/TAPP_DEVELOPMENT.md) | 扩展应用 |

## 目录

```
docs/
  INDEX.md                 # 本页
  QUICKSTART.md
  API.md
  updater-spec.md          # Updater 协议
  deployment/              # 生产编排、端口、更新、安全
  development/             # 架构、构建、联邦、OAuth、Tapp
  features/                # Library、.tapp 包格式
  design/                  # 主题、图标、通知、Agent Channel、3D 管线
```

## 部署

| 文档 | 说明 |
| --- | --- |
| [QUICKSTART.md](QUICKSTART.md) | 生产与开发快速开始 |
| [DOCKER_DEPLOYMENT.md](deployment/DOCKER_DEPLOYMENT.md) | 三网拓扑、env、健康检查 |
| [NATIVE_DEPLOYMENT.md](deployment/NATIVE_DEPLOYMENT.md) | 无 Docker 生产部署 |
| [EXTERNAL_POSTGRES.md](deployment/EXTERNAL_POSTGRES.md) | 外部数据库 |
| [PORTS.md](deployment/PORTS.md) | 端口与代理路径 |
| [SETUP_BOOTSTRAP.md](deployment/SETUP_BOOTSTRAP.md) | 编排安装时的安装暗号 |
| [UPDATER_QUICKSTART.md](deployment/UPDATER_QUICKSTART.md) | 更新 / 回滚 / 救援 |
| [updater-spec.md](updater-spec.md) | Updater 协议与失败模式 |
| [UPDATER_SECURITY_BASELINE.md](deployment/UPDATER_SECURITY_BASELINE.md) | Updater 安全基线 |
| [UPDATER_GATEWAY_THREAT_MODEL.md](deployment/UPDATER_GATEWAY_THREAT_MODEL.md) | updater-gateway 能力边界 |
| [SECURITY_HEADERS.md](deployment/SECURITY_HEADERS.md) | 安全响应头 |
| [MIGRATION_DOMAIN.md](deployment/MIGRATION_DOMAIN.md) | 换域名（非联邦） |
| [FEDERATION_DOMAIN_MOVE.md](deployment/FEDERATION_DOMAIN_MOVE.md) | 联邦域名 Move |

## 开发

| 文档 | 说明 |
| --- | --- |
| [ARCHITECTURE.md](development/ARCHITECTURE.md) | 组件与拓扑 |
| [BUILD.md](development/BUILD.md) | 工具链与构建 |
| [API.md](API.md) | HTTP API 入口 |
| [OAUTH.md](development/OAUTH.md) | 本地登录与 OAuth/OIDC |
| [FEDERATION.md](development/FEDERATION.md) | 联邦行为笔记 |
| [UPGRADE_NOTES.md](development/UPGRADE_NOTES.md) | 升级说明 |
| [TAPP_DEVELOPMENT.md](development/TAPP_DEVELOPMENT.md) | Tapp 文档索引 |
| [tapp/](development/tapp/) | Manifest / SDK / Store / Playground / 排错 |

## 功能与设计

| 文档 | 说明 |
| --- | --- |
| [LIBRARY.md](features/LIBRARY.md) | 资料库 |
| [TAPP_FILE_FORMAT.md](features/TAPP_FILE_FORMAT.md) | `.tapp` 包格式 |
| [theme-system.md](design/theme-system.md) | Surface / Glow |
| [icon-inventory.md](design/icon-inventory.md) | 图标规范 |
| [notification-catalog.md](design/notification-catalog.md) | 通知目录 |
| [agent-channel.md](design/agent-channel.md) | Agent 运输适配：办事 Channel 与 QQ 单聊第一平台 |
| [merope-25d-pipeline.md](design/merope-25d-pipeline.md) | Merope / See-through / Anime2.5D 形象管线 |
| [merope-3d-pipeline.md](design/merope-3d-pipeline.md) | Merope / Tripo 3D 管线 |
| [brew-tile-grid.md](design/brew-tile-grid.md) | Brew 磁贴网格改造实施手册 |
