//! Myriad Federation Protocol (MFP) — 核心模块
//!
//! Discovery（WebFinger, NodeInfo）；Actor / Inbox / Outbox / HTTP Signatures；
//! Channel / Room / Ring；内容发布 + Timeline。

// 基础设施
pub mod audience;
pub mod errors;
pub mod http_cache;
pub mod keys;
pub mod limits;
pub mod signature;
pub mod types;

// AP 兼容层
pub mod actor;
pub mod delivery;
pub mod discovery;
pub mod follow;
pub mod inbox;
pub mod move_actor;
pub mod outbox;

// 内容发布
pub mod content;
pub mod interactions;

// Channel 实时通信
pub mod channel;
pub mod notify;
pub mod ws_gateway;

// Room 多方通信
pub mod room;
pub mod room_peers;

// Ring 去中心化环网
pub mod ring;

// 文件传输 + E2E 加密
pub mod e2e;
pub mod file_transfer;
pub mod trust;

pub mod worker;

#[cfg(test)]
pub(crate) mod test_db;
