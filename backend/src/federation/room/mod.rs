//! 联邦 Room 管理模块
//!
//! N:N 多方房间：群聊、协作、共享阅读室、联合分析等
//! 支持星型路由（Home Server fan-out）和成员治理

mod crud;
mod e2e;
pub(crate) mod game;
mod helpers;
mod inbox;
mod members;
mod messages;
mod stickers;
mod types;

pub use crud::*;
pub use e2e::{handle_key_exchange, initiate_e2e_key_exchange};
pub(crate) use helpers::{fanout_to_remote_members, get_member_role};
pub use inbox::*;
pub use members::*;
pub use messages::*;
pub use stickers::{add_room_sticker, remove_room_sticker};
pub use types::*;

#[cfg(test)]
mod tests;
