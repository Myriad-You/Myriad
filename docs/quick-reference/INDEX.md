# 📚 Myriad Documentation Portal

Welcome to Myriad documentation! This is your central hub for all project documentation, scripts, and resources.

## 🎯 Quick Navigation

| I want to...              | Go to                                                     |
| ------------------------- | --------------------------------------------------------- |
| **Deploy Myriad now!**    | [Quick Start →](../QUICKSTART.md)                         |
| **Understand the system** | [Architecture →](../development/ARCHITECTURE.md)          |
| **Use the API**           | [API Docs →](../API.md)                                   |
| **Deploy with Docker**    | [Docker Deployment →](../deployment/DOCKER_DEPLOYMENT.md)  |
| **Build from source**     | [Build Guide →](../development/BUILD.md)                  |
| **Check ports**           | [Ports →](../deployment/PORTS.md)                         |

---

## 📖 Documentation Structure

```
docs/
├── API.md                    # Complete API reference with examples
├── QUICKSTART.md             # ⭐ Quick start guide (START HERE)
├── UPDATER_QUICKSTART.md     # Update, rollback, and rescue flow
├── updater-spec.md           # Updater protocol and failure-mode design
│
├── deployment/               # Current deployment documentation
│   ├── DOCKER_DEPLOYMENT.md # proxy + updater + docker-guard production stack
│   ├── MIGRATION_DOCKER_GUARD.md # sock → docker-guard topology migration
│   ├── UPDATER_SECURITY_BASELINE.md # done-state updater security + red lines
│   └── PORTS.md             # dev/prod port map
│
├── development/              # Development Documentation
│   ├── ARCHITECTURE.md      # System architecture & design
│   ├── BUILD.md             # Build from source instructions
│   ├── TAPP_DEVELOPMENT.md  # Tapp development guide
│   └── tapp/                # Tapp reference docs
│
├── features/                 # Feature Documentation
│   ├── LIBRARY.md           # Library feature guide
│   └── TAPP_FILE_FORMAT.md  # Tapp file format
│
├── guides/                   # User Guides
│   └── SECURITY_HEADERS.md  # Security configuration
│
└── quick-reference/          # THIS FILE - Documentation portal
    └── INDEX.md              # Main documentation index
```

---

## 🚀 Getting Started Paths

### Path 1: New User (5 minutes)

**Goal:** Get Myriad running quickly

```
1. Install Docker Desktop
2. Read: QUICKSTART.md
3. Run: bash scripts/docker/deploy.sh up
4. Visit: http://localhost
5. Complete setup wizard
```

### Path 2: Developer (30 minutes)

**Goal:** Understand codebase and contribute

```
1. Read: development/ARCHITECTURE.md
2. Read: development/BUILD.md
3. Clone repository
4. Run development scripts
5. Explore: backend/src/ and frontend/src/
```

### Path 3: DevOps Engineer (20 minutes)

**Goal:** Deploy and manage in production

```
1. Read: QUICKSTART.md
2. Read: deployment/DOCKER_DEPLOYMENT.md
3. Configure proxy/TLS at the host edge
4. Configure monitoring and backups
```

### Path 4: API Consumer (15 minutes)

**Goal:** Integrate with Myriad API

```
1. Read: API.md
2. Test endpoints with curl/Postman
3. Review JavaScript/Python examples
4. Implement client integration
5. Handle errors and rate limits
```

---

## 📂 Project Structure

### Root Directory

```
Myriad/
├── README.md                    # Project overview
├── LICENSE                      # GPL-3.0 license
├── Makefile                     # Quick commands (Linux/Mac)
├── docker-compose.yml           # Production proxy + updater stack
├── docker-compose.dev.yml       # Development PostgreSQL only
├── .env.production.example      # Production environment template
└── docker/                      # Component Dockerfiles
```

### Scripts Directory

```
scripts/
├── docker/                      # Docker Deployment Scripts
│   ├── deploy.ps1              # Unified deployment (Windows)
│   ├── deploy.sh               # Unified deployment (Linux/Mac)
│   ├── build-and-push.ps1      # Build & push images to registry (Windows)
│   └── build-and-push.sh       # Build & push images to registry (Linux/Mac)
│
└── dev/                         # Development Scripts
    ├── dev.ps1                 # Unified dev tool (Windows)
    ├── dev.sh                  # Unified dev tool (Linux/Mac)
    └── build.sh                # Build backend (Linux/Mac)
```

**Core Scripts Listed:** 7 (4 Docker + 3 Development). Updater smoke/e2e test scripts
also live directly under `scripts/`.

### Backend Structure

```
backend/
├── src/
│   ├── main.rs                 # Entry point
│   ├── config.rs               # Configuration management
│   ├── api/                    # API endpoints
│   │   ├── auth.rs            # Authentication
│   │   ├── profile.rs         # User profiles
│   │   ├── platforms.rs       # Platform integrations
│   │   ├── analysis.rs        # AI analysis
│   │   └── ...
│   ├── db/                     # Database layer
│   ├── models/                 # Data models
│   └── services/               # Business logic
│       ├── analyzer.rs        # AI service
│       ├── fetcher.rs         # Data fetching
│       └── ...
├── migrations/                  # Database migrations
├── cache/                       # Runtime cache
└── Cargo.toml                   # Rust dependencies
```

### Frontend Structure

```
frontend/
├── src/
│   ├── pages/                  # Astro pages
│   │   ├── index.astro        # Homepage
│   │   ├── setup.astro        # Setup wizard
│   │   ├── dashboard.astro    # Dashboard
│   │   └── ...
│   ├── components/             # React/Astro components
│   │   ├── LoginForm.tsx
│   │   ├── ConfigForm.tsx
│   │   ├── PlatformStats.astro
│   │   └── ...
│   ├── layouts/                # Page layouts
│   ├── lib/                    # Utilities & API client
│   └── styles/                 # CSS styles
├── public/                      # Static assets
└── package.json                 # Node.js dependencies
```

### Documentation Structure

```
docs/
├── API.md                      # 📡 Complete API reference
├── QUICKSTART.md               # ⭐ Quick start guide
├── UPDATER_QUICKSTART.md       # 🔁 Update operations
├── updater-spec.md             # 🔧 Updater design
│
├── deployment/                 # 🚀 Current deployment guides
│   ├── DOCKER_DEPLOYMENT.md   # proxy + updater production stack
│   ├── UPDATER_SECURITY_BASELINE.md # updater security done-state
│   └── PORTS.md               # dev/prod port map
│
├── development/                # 💻 Developer Guides
│   ├── ARCHITECTURE.md        # System design
│   ├── BUILD.md               # Build instructions
│   ├── TAPP_DEVELOPMENT.md    # Tapp guide
│   └── tapp/                  # Tapp reference docs
│
├── features/                   # 📖 Feature Docs
│   ├── LIBRARY.md             # Library feature
│   └── TAPP_FILE_FORMAT.md    # Tapp file format
│
├── guides/                     # 📚 User Guides
│   └── SECURITY_HEADERS.md    # Security setup
│
└── quick-reference/            # 📖 Quick Reference
    └── INDEX.md               # This file
```

---

## 🎮 Scripts Quick Reference

### Docker Deployment Scripts

| Task                         | Windows                                                    | Linux/Mac                                           |
| ---------------------------- | ---------------------------------------------------------- | --------------------------------------------------- |
| **Bootstrap / Start**        | `.\scripts\docker\deploy.ps1 up`                           | `bash scripts/docker/deploy.sh up`                  |
| **Manual Tag Upgrade**       | `.\scripts\docker\deploy.ps1 upgrade`                      | `bash scripts/docker/deploy.sh upgrade`             |
| **View Status**              | `.\scripts\docker\deploy.ps1 status`                       | `bash scripts/docker/deploy.sh status`              |
| **View Logs**                | `.\scripts\docker\deploy.ps1 logs`                         | `bash scripts/docker/deploy.sh logs`                |
| **Restart Stack**            | `.\scripts\docker\deploy.ps1 restart`                      | `bash scripts/docker/deploy.sh restart`             |
| **Stop Services**            | `.\scripts\docker\deploy.ps1 down`                         | `bash scripts/docker/deploy.sh down`                |

### Development Scripts

| Task                   | Windows                         | Linux/Mac                      |
| ---------------------- | ------------------------------- | ------------------------------ |
| **Start Dev Services** | `.\scripts\dev\dev.ps1 start`   | `./scripts/dev/dev.sh start`   |
| **Stop Services**      | `.\scripts\dev\dev.ps1 stop`    | `./scripts/dev/dev.sh stop`    |
| **Restart Services**   | `.\scripts\dev\dev.ps1 restart` | `./scripts/dev/dev.sh restart` |
| **Clean Everything**   | `.\scripts\dev\dev.ps1 clean`   | `./scripts/dev/dev.sh clean`   |
| **View Status**        | `.\scripts\dev\dev.ps1 status`  | `./scripts/dev/dev.sh status`  |
| **Build Backend**      | N/A                             | `./scripts/dev/build.sh`       |

**Advanced Options:**

```powershell
# Windows - Manage specific services
.\scripts\dev\dev.ps1 start -Service backend
.\scripts\dev\dev.ps1 stop -Service frontend
.\scripts\dev\dev.ps1 clean -Force  # Skip confirmation
```

```bash
# Linux/Mac - Manage specific services
./scripts/dev/dev.sh start backend
./scripts/dev/dev.sh stop frontend
```

### Makefile Commands (Linux/Mac)

```bash
make deploy      # Bootstrap and start through scripts/docker/deploy.sh
make start       # Start through scripts/docker/deploy.sh
make stop        # Stop containers, preserving volumes
make restart     # Restart the stack
make logs        # View stack logs
make status      # View status and image versions
make build       # Build all component images locally
make clean       # Full cleanup, including pgdata/state/backups
make backup      # Backup PostgreSQL into ./backups
```

---

## 📊 Documentation by Category

### 🚀 Deployment

| Document                                                   | Description                | Audience              |
| ---------------------------------------------------------- | -------------------------- | --------------------- |
| [QUICKSTART.md](../QUICKSTART.md)                          | Quick start guide          | Everyone              |
| [DOCKER_DEPLOYMENT.md](../deployment/DOCKER_DEPLOYMENT.md) | Current proxy + updater Docker stack | Operators |
| [UPDATER_SECURITY_BASELINE.md](../deployment/UPDATER_SECURITY_BASELINE.md) | Updater security done-state + operator red lines | Operators |
| [PORTS.md](../deployment/PORTS.md)                          | Development and production port map | Operators, Developers |
| [UPDATER_QUICKSTART.md](../UPDATER_QUICKSTART.md)          | Update, rollback, rescue flow | Operators |

**Start with:** QUICKSTART.md

### 💻 Development

| Document                                          | Description                  | Audience   |
| ------------------------------------------------- | ---------------------------- | ---------- |
| [ARCHITECTURE.md](../development/ARCHITECTURE.md) | System architecture & design | Developers |
| [BUILD.md](../development/BUILD.md)               | Build from source            | Developers |
| [TAPP_DEVELOPMENT.md](../development/TAPP_DEVELOPMENT.md) | Tapp development guide | Developers |
| [development/tapp/](../development/tapp/QUICKSTART.md) | Tapp reference docs | Developers |

**Start with:** ARCHITECTURE.md

### 📡 API

| Document            | Description                          | Audience                |
| ------------------- | ------------------------------------ | ----------------------- |
| [API.md](../API.md) | Complete API reference with examples | Developers, Integrators |

**Includes:** Endpoints, request/response formats, code examples in JavaScript, Python, cURL

### 📚 Guides

| Document                                             | Description                | Audience      |
| ---------------------------------------------------- | -------------------------- | ------------- |
| [SECURITY_HEADERS.md](../guides/SECURITY_HEADERS.md) | Security configuration     | System Admins |

### 📝 Reference

| Document                        | Description                     | Audience |
| ------------------------------- | ------------------------------- | -------- |
| [updater-spec.md](../updater-spec.md) | Updater design reference | Operators, Developers |
| [oauth-refactor-plan.md](../oauth-refactor-plan.md) | OAuth refactor plan | Developers |
| [LICENSE](../../LICENSE)        | GPL-3.0 license terms           | Everyone |

---

## 🔍 Find What You Need

### "How do I deploy Myriad?"

→ [QUICKSTART.md](../QUICKSTART.md)

### "I want to use versioned Docker images"

→ [QUICKSTART.md](../QUICKSTART.md) - Production quick start

### "How do I build Docker images?"

→ [UPDATER_QUICKSTART.md](../UPDATER_QUICKSTART.md) - Versioned image and updater flow

### "What configuration options are available?"

→ [deployment/DOCKER_DEPLOYMENT.md](../deployment/DOCKER_DEPLOYMENT.md) - Current production configuration

### "How do I use the API?"

→ [API.md](../API.md)

### "What's the system architecture?"

→ [development/ARCHITECTURE.md](../development/ARCHITECTURE.md)

### "How do I build from source?"

→ [development/BUILD.md](../development/BUILD.md)

### "What ports does each service use?"

→ [deployment/PORTS.md](../deployment/PORTS.md)

### "How do I contribute?"

→ [development/BUILD.md](../development/BUILD.md) + [development/ARCHITECTURE.md](../development/ARCHITECTURE.md)

### "How do I secure my deployment?"

→ [guides/SECURITY_HEADERS.md](../guides/SECURITY_HEADERS.md)

### "What is the updater security baseline?"

→ [deployment/UPDATER_SECURITY_BASELINE.md](../deployment/UPDATER_SECURITY_BASELINE.md)

### "Where are all the scripts?"

→ See [Scripts Quick Reference](#-scripts-quick-reference) above

---

## 💡 Documentation Tips

### For New Users

1. **Start simple:** Read [QUICKSTART.md](../QUICKSTART.md)
2. **Try it:** Run deployment script
3. **Explore:** Use the application
4. **Deep dive:** Read other docs as needed

### For Developers

1. **Understand design:** [ARCHITECTURE.md](../development/ARCHITECTURE.md)
2. **Set up environment:** [BUILD.md](../development/BUILD.md)
3. **Learn API:** [API.md](../API.md)
4. **Start coding:** Explore `backend/src/` and `frontend/src/`

### For DevOps

1. **Quick deploy:** [QUICKSTART.md](../QUICKSTART.md)
2. **Production setup:** [DOCKER_DEPLOYMENT.md](../deployment/DOCKER_DEPLOYMENT.md)
3. **Ports:** [PORTS.md](../deployment/PORTS.md)
4. **Security:** [SECURITY_HEADERS.md](../guides/SECURITY_HEADERS.md)

---

## 🆘 Getting Help

### Documentation Issues

- **Outdated info?** Check the deployment and updater docs for the current production layout
- **Missing details?** Search within documents (Ctrl+F)
- **Need examples?** See [API.md](../API.md) for code samples

### Technical Issues

1. Check relevant troubleshooting section in docs
2. View logs: `bash scripts/docker/deploy.sh logs`
3. Search [GitHub Issues](https://github.com/myriad-you/Myriad/issues)
4. Open new issue with details

### Contributing to Docs

Found an error or want to improve documentation?

1. All docs are in Markdown format
2. Follow existing style and structure
3. Test all commands before documenting
4. Include examples for both Windows and Linux
5. Submit pull request on GitHub

---

## 📈 Documentation Statistics

| Category        | Files | Total Lines     |
| --------------- | ----- | --------------- |
| **Deployment**  | 4     | ~610            |
| **Development** | 12    | ~5870           |
| **API**         | 1     | ~900            |
| **Features**    | 2     | ~310            |
| **Guides**      | 1     | ~200            |
| **Reference**   | 3     | ~1375           |
| **Total**       | **23** | **~9270 lines** |

---

## 🌟 Quick Start Checklist

- [ ] Docker Desktop installed
- [ ] Repository cloned
- [ ] Read [QUICKSTART.md](../QUICKSTART.md)
- [ ] Run deployment script
- [ ] Access http://localhost
- [ ] Complete setup wizard
- [ ] Configure platform integrations
- [ ] Explore dashboard
- [ ] Read [API.md](../API.md) for integrations

---

**Documentation Version:** 1.1  
**Last Updated:** 2026-07-03
**Maintainer:** Myriad Team

**Have questions?** Open an issue on [GitHub](https://github.com/myriad-you/Myriad/issues)
