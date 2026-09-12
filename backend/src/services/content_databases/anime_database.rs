//! 番剧/电视剧/电影数据库
//!
//! JSON loader (`anime_database.json`; bootstraps `entries: []` if missing) plus category analysis.

#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use tracing::{error, info};
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnimeEntry {
    pub title: String,
    pub category: ContentCategory,
    pub genre: Vec<String>,
    pub rating: f32,
    #[serde(default)]
    pub aliases: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ContentCategory {
    Anime,    // 番剧
    TvSeries, // 电视剧
    Movie,    // 电影
}

impl std::str::FromStr for ContentCategory {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Anime" => Ok(ContentCategory::Anime),
            "TvSeries" => Ok(ContentCategory::TvSeries),
            "Movie" => Ok(ContentCategory::Movie),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct DatabaseFile {
    version: String,
    entries: Vec<AnimeEntryJson>,
}

#[derive(Debug, Serialize, Deserialize)]
struct AnimeEntryJson {
    title: String,
    category: String,
    genres: Vec<String>,
    rating: f32,
    #[serde(default)]
    aliases: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategoryAnalysis {
    pub category: ContentCategory,
    pub count: usize,
    pub percentage: f32,
    pub genres: HashMap<String, usize>,
    pub examples: Vec<String>, // up to 5 (`take(5)`); may be empty
    pub summary: String,       // 判断摘要
}

pub struct AnimeDatabase {
    entries: HashMap<String, AnimeEntry>,
}

impl AnimeDatabase {
    pub fn new() -> Self {
        let mut entries = HashMap::new();

        // Load from JSON file
        let file_path = if Path::new("backend/data/anime_database.json").exists() {
            Path::new("backend/data/anime_database.json")
        } else if Path::new("data/anime_database.json").exists() {
            Path::new("data/anime_database.json")
        } else if Path::new("backend").exists() {
            Path::new("backend/data/anime_database.json")
        } else {
            Path::new("data/anime_database.json")
        };

        if file_path.exists() {
            match fs::read_to_string(file_path) {
                Ok(content) => {
                    match serde_json::from_str::<DatabaseFile>(&content) {
                        Ok(db_file) => {
                            for json_entry in db_file.entries {
                                if let Ok(category) = json_entry.category.parse::<ContentCategory>()
                                {
                                    let entry = AnimeEntry {
                                        title: json_entry.title.clone(),
                                        category,
                                        genre: json_entry.genres,
                                        rating: json_entry.rating,
                                        aliases: json_entry.aliases.clone(),
                                    };
                                    entries.insert(json_entry.title.clone(), entry.clone());
                                    // 主 map 只按 title 插入；别名由 find() 扫描 aliases。
                                } else {
                                    error!("Invalid category for entry: {}", json_entry.title);
                                }
                            }
                            info!("Loaded {} entries from anime database", entries.len());
                        }
                        Err(e) => error!("Failed to parse anime database JSON: {}", e),
                    }
                }
                Err(e) => error!("Failed to read anime database file: {}", e),
            }
        } else {
            info!(
                "Anime database file not found at {:?}, creating new and requesting population.",
                file_path
            );
            if let Some(parent) = file_path.parent() {
                let _ = fs::create_dir_all(parent);
            }

            let db_file = DatabaseFile {
                version: "1.0".to_string(),
                entries: Vec::new(),
            };

            if let Ok(content) = serde_json::to_string_pretty(&db_file) {
                if let Err(e) = fs::write(file_path, content) {
                    error!("Failed to create anime database file: {}", e);
                }
            }
        }

        Self { entries }
    }

    pub fn add_entry(&mut self, entry: AnimeEntry) {
        self.entries.insert(entry.title.clone(), entry);
    }

    pub fn save(&self) -> Result<(), std::io::Error> {
        let file_path = Path::new("backend/data/anime_database.json");
        let file_path = if file_path.exists() || Path::new("backend").exists() {
            file_path
        } else {
            Path::new("data/anime_database.json")
        };

        let json_entries: Vec<AnimeEntryJson> = self
            .entries
            .values()
            .map(|e| AnimeEntryJson {
                title: e.title.clone(),
                category: match e.category {
                    ContentCategory::Anime => "Anime".to_string(),
                    ContentCategory::TvSeries => "TvSeries".to_string(),
                    ContentCategory::Movie => "Movie".to_string(),
                },
                genres: e.genre.clone(),
                rating: e.rating,
                aliases: e.aliases.clone(),
            })
            .collect();

        let db_file = DatabaseFile {
            version: "1.0".to_string(),
            entries: json_entries,
        };

        let content = serde_json::to_string_pretty(&db_file)?;
        fs::write(file_path, content)?;
        Ok(())
    }

    /// 查找内容条目
    pub fn find(&self, title: &str) -> Option<&AnimeEntry> {
        // 精确匹配
        if let Some(entry) = self.entries.get(title) {
            return Some(entry);
        }

        // 模糊匹配（包含关系）
        for (key, entry) in &self.entries {
            if title.contains(key) || key.contains(title) {
                return Some(entry);
            }
            // Check aliases
            for alias in &entry.aliases {
                if title.contains(alias) || alias.contains(title) {
                    return Some(entry);
                }
            }
        }

        None
    }

    /// 分析用户观看列表
    pub fn analyze(&self, watch_list: Vec<(String, String)>) -> Vec<CategoryAnalysis> {
        // watch_list: [(title, author), ...]
        let mut category_map: HashMap<ContentCategory, Vec<String>> = HashMap::new();
        let mut genre_count: HashMap<ContentCategory, HashMap<String, usize>> = HashMap::new();
        let mut unknown_items: Vec<String> = Vec::new();

        let total_count = watch_list.len();

        for (title, _author) in watch_list {
            if let Some(entry) = self.find(&title) {
                // 已知内容 - 分类统计
                category_map
                    .entry(entry.category.clone())
                    .or_default()
                    .push(title.clone());

                // 统计类型
                let genre_map = genre_count.entry(entry.category.clone()).or_default();
                for genre in &entry.genre {
                    *genre_map.entry(genre.clone()).or_insert(0) += 1;
                }
            } else {
                // Unknown titles are pushed then discarded (`analyze` returns known categories only).
                unknown_items.push(title.clone());
            }
        }

        let mut analyses = Vec::new();

        // 生成各分类的分析
        for (category, items) in category_map {
            let count = items.len();
            let percentage = (count as f32 / total_count as f32) * 100.0;

            // 获取该分类的类型统计
            let genres = genre_count.get(&category).cloned().unwrap_or_default();

            // 找出最常见的类型
            let top_genres: Vec<String> = {
                let mut genre_vec: Vec<_> = genres.iter().collect();
                genre_vec.sort_by(|a, b| b.1.cmp(a.1));
                genre_vec
                    .iter()
                    .take(3)
                    .map(|(k, _)| k.to_string())
                    .collect()
            };

            // 选择代表性例子（最多5个）
            let examples: Vec<String> = items.iter().take(5).cloned().collect();

            // 生成摘要
            let category_name = match category {
                ContentCategory::Anime => "anime",
                ContentCategory::TvSeries => "TV series",
                ContentCategory::Movie => "movies",
            };

            let summary = if top_genres.is_empty() {
                format!("Watched {count} {category_name}")
            } else {
                format!(
                    "Watched {} {}, likely interested in {}",
                    count,
                    category_name,
                    top_genres.join(", ")
                )
            };

            analyses.push(CategoryAnalysis {
                category,
                count,
                percentage,
                genres,
                examples,
                summary,
            });
        }

        analyses
    }
}

impl Default for AnimeDatabase {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_database() -> AnimeDatabase {
        let mut db = AnimeDatabase {
            entries: HashMap::new(),
        };
        for title in ["鬼灭之刃", "进击的巨人", "咒术回战"] {
            db.add_entry(AnimeEntry {
                title: title.to_string(),
                category: ContentCategory::Anime,
                genre: vec!["热血".to_string()],
                rating: 9.0,
                aliases: Vec::new(),
            });
        }
        db
    }

    #[test]
    fn test_find() {
        let db = test_database();
        assert!(db.find("鬼灭之刃").is_some());
        assert!(db.find("不存在的动画").is_none());
    }

    #[test]
    fn test_analyze() {
        let db = test_database();
        let watch_list = vec![
            ("鬼灭之刃".to_string(), "某作者".to_string()),
            ("进击的巨人".to_string(), "某作者".to_string()),
            ("咒术回战".to_string(), "某作者".to_string()),
        ];

        let analyses = db.analyze(watch_list);
        assert_eq!(analyses.len(), 1); // 只有番剧分类
        assert_eq!(analyses[0].count, 3);
        assert!(analyses[0].summary.contains("热血"));
    }
}
