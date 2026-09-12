//! Steam 游戏数据库
//!
//! 从 `game_database.json` 加载（缺失则写空数组）；用于分类用户游戏库

#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::cmp::Reverse;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use tracing::{error, info};
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameEntry {
    pub name: String,
    pub genres: Vec<String>,
    pub tags: Vec<String>,
    pub rating: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameGenreAnalysis {
    pub genre: String,
    pub count: usize,
    pub percentage: f32,
    pub examples: Vec<String>,
    pub total_playtime: i64, // 分钟
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameAnalysis {
    pub total_games: usize,
    pub genre_analysis: Vec<GameGenreAnalysis>,
    pub summary: String,
    pub unknown_games: Vec<(String, i64)>, // (name, playtime)
}

pub struct GameDatabase {
    entries: HashMap<String, GameEntry>,
}

impl GameDatabase {
    pub fn new() -> Self {
        let mut entries = HashMap::new();

        // Load from JSON file
        let file_path = if Path::new("backend/data/game_database.json").exists() {
            Path::new("backend/data/game_database.json")
        } else if Path::new("data/game_database.json").exists() {
            Path::new("data/game_database.json")
        } else if Path::new("backend").exists() {
            Path::new("backend/data/game_database.json")
        } else {
            Path::new("data/game_database.json")
        };

        if file_path.exists() {
            match fs::read_to_string(file_path) {
                Ok(content) => match serde_json::from_str::<Vec<GameEntry>>(&content) {
                    Ok(entries_list) => {
                        for entry in entries_list {
                            entries.insert(entry.name.clone(), entry);
                        }
                        info!("Loaded {} entries from game database", entries.len());
                    }
                    Err(e) => {
                        error!("Failed to parse game database JSON as array: {}", e);
                    }
                },
                Err(e) => error!("Failed to read game database file: {}", e),
            }
        } else {
            info!(
                "Game database file not found at {:?}, creating new and requesting population.",
                file_path
            );
            if let Some(parent) = file_path.parent() {
                let _ = fs::create_dir_all(parent);
            }

            let empty_entries: Vec<GameEntry> = Vec::new();

            if let Ok(content) = serde_json::to_string_pretty(&empty_entries) {
                if let Err(e) = fs::write(file_path, content) {
                    error!("Failed to create game database file: {}", e);
                }
            }
        }

        Self { entries }
    }

    pub fn add_entry(&mut self, entry: GameEntry) {
        self.entries.insert(entry.name.clone(), entry);
    }

    pub fn save(&self) -> Result<(), std::io::Error> {
        let file_path = Path::new("backend/data/game_database.json");
        let file_path = if file_path.exists() || Path::new("backend").exists() {
            file_path
        } else {
            Path::new("data/game_database.json")
        };

        let entries_list: Vec<GameEntry> = self.entries.values().cloned().collect();
        let content = serde_json::to_string_pretty(&entries_list)?;
        fs::write(file_path, content)?;
        Ok(())
    }

    /// 查找游戏条目
    pub fn find(&self, name: &str) -> Option<&GameEntry> {
        // 精确匹配
        if let Some(entry) = self.entries.get(name) {
            return Some(entry);
        }

        // 模糊匹配
        for (key, entry) in &self.entries {
            if name.contains(key) || key.contains(name) {
                return Some(entry);
            }
        }

        None
    }

    /// 分析游戏库
    pub fn analyze(&self, game_list: Vec<(String, i64)>) -> GameAnalysis {
        // game_list: [(name, playtime_minutes), ...]
        let mut genre_map: HashMap<String, Vec<(String, i64)>> = HashMap::new();
        let mut genre_playtime: HashMap<String, i64> = HashMap::new();
        let mut unknown_games: Vec<(String, i64)> = Vec::new();

        let total_games = game_list.len();

        for (name, playtime) in game_list {
            if let Some(entry) = self.find(&name) {
                // 已知游戏 - 按类型分类
                for genre in &entry.genres {
                    genre_map
                        .entry(genre.clone())
                        .or_default()
                        .push((name.clone(), playtime));

                    *genre_playtime.entry(genre.clone()).or_insert(0) += playtime;
                }
            } else {
                // 未知游戏 - 保留原样并记录
                unknown_games.push((name.clone(), playtime));
            }
        }

        // 按类型聚合
        let mut genre_analysis: Vec<GameGenreAnalysis> = genre_map
            .into_iter()
            .map(|(genre, games)| {
                let count = games.len();
                let percentage = (count as f32 / total_games as f32) * 100.0;
                let total_playtime = genre_playtime.get(&genre).copied().unwrap_or(0);

                // 选择代表性例子（最多5个，按游玩时间排序）
                let mut sorted_games = games;
                sorted_games.sort_by_key(|b| Reverse(b.1));
                let examples: Vec<String> = sorted_games
                    .iter()
                    .take(5)
                    .map(|(name, _)| name.clone())
                    .collect();

                GameGenreAnalysis {
                    genre,
                    count,
                    percentage,
                    examples,
                    total_playtime,
                }
            })
            .collect();

        // 按游玩时间排序
        genre_analysis.sort_by_key(|b| Reverse(b.total_playtime));

        // 生成摘要
        let top_genres: Vec<String> = genre_analysis
            .iter()
            .take(3)
            .map(|g| g.genre.clone())
            .collect();

        let summary = if top_genres.is_empty() {
            format!("Owns {total_games} games")
        } else {
            format!(
                "Owns {} games, mostly likes {}",
                total_games,
                top_genres.join(", ")
            )
        };

        GameAnalysis {
            total_games,
            genre_analysis,
            summary,
            unknown_games,
        }
    }
}

impl Default for GameDatabase {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_database() -> GameDatabase {
        let mut db = GameDatabase {
            entries: HashMap::new(),
        };
        for (name, genre) in [
            ("Counter-Strike: Global Offensive", "FPS"),
            ("Dota 2", "MOBA"),
            ("The Witcher 3: Wild Hunt", "RPG"),
        ] {
            db.add_entry(GameEntry {
                name: name.to_string(),
                genres: vec![genre.to_string()],
                tags: Vec::new(),
                rating: 9.0,
            });
        }
        db
    }

    #[test]
    fn test_find() {
        let db = test_database();
        assert!(db.find("Counter-Strike: Global Offensive").is_some());
        assert!(db.find("不存在的游戏").is_none());
    }

    #[test]
    fn test_analyze() {
        let db = test_database();
        let game_list = vec![
            ("Counter-Strike: Global Offensive".to_string(), 5000),
            ("Dota 2".to_string(), 3000),
            ("The Witcher 3: Wild Hunt".to_string(), 2000),
        ];

        let analysis = db.analyze(game_list);
        assert_eq!(analysis.total_games, 3);
        assert!(analysis.summary.contains("FPS"));
    }
}
