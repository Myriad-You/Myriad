//! Myriad Federation Protocol (MFP) — 核心模块
//!
//! 分层架构：
//! - Layer 1: Discovery（WebFinger, NodeInfo）
//! - Layer 2: Instance Core（Actor, Inbox/Outbox, HTTP Signatures）
//! - Layer 3: Channel(1↔1) / Room(N↔N) / Ring(去中心化)
//! - Layer 4: Content Federation（内容发布 + Timeline）
//! - Layer 5: Application Tapps（联邦应用）

// Phase 0: 基础设施
pub mod audience;
pub mod errors;
pub mod http_cache;
pub mod keys;
pub mod limits;
pub mod signature;
pub mod types;

// Phase 1: AP 兼容层
pub mod actor;
pub mod delivery;
pub mod discovery;
pub mod follow;
pub mod inbox;
pub mod move_actor;
pub mod outbox;

// Phase 2: 内容发布
pub mod content;
pub mod interactions;

// Phase 3: Channel 实时通信
pub mod channel;
pub mod notify;
pub mod ws_gateway;

// Phase 4: Room 多方通信
pub mod room;

// Phase 5: Ring 去中心化环网
pub mod ring;

// Phase 5 补全: 安全增强 + 文件传输 + E2E 加密
pub mod e2e;
pub mod file_transfer;
pub mod trust;
