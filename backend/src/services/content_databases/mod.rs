//! 预置内容数据库模块
//!
//! 用于对平台内容进行初步分类和判断，减少需要发送给 AI 的 token 数量
//! 策略：识别常见内容 → 分类统计 → 合并为判断 + 代表性例子。省 token 依赖下方静态库。

pub mod anime_database;
pub mod artist_database;
pub mod game_database;

pub use anime_database::AnimeDatabase;
pub use artist_database::ArtistDatabase;
pub use game_database::GameDatabase;
