//! 联邦文件传输模块
//!
//! 基于 federation_file_transfers 表实现：
//! 1. 文件元数据发送与接收
//! 2. 分块传输与进度追踪
//! 3. 基于 Channel 的文件传输 Activity

mod http;
mod inbox;
mod storage;
mod types;

pub use http::*;
pub use inbox::handle_file_transfer;
pub use types::*;
