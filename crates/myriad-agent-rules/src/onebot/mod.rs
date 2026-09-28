//! OneBot v11 协议支持（纯规则，无 I/O）。
//!
//! 第一刀只做私聊办事：正向 WebSocket 收事件、Action 出站。

pub mod decode;
pub mod encode;
pub mod rules;
pub mod wire;
