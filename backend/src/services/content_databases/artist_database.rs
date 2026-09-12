//! 网易云音乐歌手数据库
//!
//! JSON loader (`data/artist_database.json`, default `[]`) plus region/genre analysis.

#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::cmp::Reverse;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use tracing::{error, info};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtistEntry {
    pub name: String,
    pub genres: Vec<String>,
    pub region: String, // detect_region: 韩国 / 日本 / 华语 / 欧美
    pub style: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtistGenreAnalysis {
    pub genre: String,
    pub count: usize,
    pub percentage: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MusicAnalysis {
    pub total_songs: usize,
    pub artist_count: usize,
    pub genre_analysis: Vec<ArtistGenreAnalysis>,
    pub region_distribution: HashMap<String, usize>,
    pub summary: String,
    pub favorite_artists: Vec<String>, // 最喜欢的歌手（按歌曲数排序，最多10个）
    pub unknown_songs: Vec<(String, String)>, // (title, artist)
}

pub struct ArtistDatabase {
    entries: HashMap<String, ArtistEntry>,
}

impl ArtistDatabase {
    pub fn new() -> Self {
        let mut entries = HashMap::new();

        // Load from JSON file
        let file_path = if Path::new("backend/data/artist_database.json").exists() {
            Path::new("backend/data/artist_database.json")
        } else if Path::new("data/artist_database.json").exists() {
            Path::new("data/artist_database.json")
        } else if Path::new("backend").exists() {
            Path::new("backend/data/artist_database.json")
        } else {
            Path::new("data/artist_database.json")
        };

        if file_path.exists() {
            if let Ok(content) = fs::read_to_string(file_path) {
                if let Ok(entries_list) = serde_json::from_str::<Vec<ArtistEntry>>(&content) {
                    for entry in entries_list {
                        entries.insert(entry.name.clone(), entry);
                    }
                    info!("Loaded {} entries from artist database", entries.len());
                } else {
                    error!("Failed to parse artist database JSON");
                }
            }
        } else {
            info!(
                "Artist database file not found at {:?}, creating new and requesting population.",
                file_path
            );
            if let Some(parent) = file_path.parent() {
                let _ = fs::create_dir_all(parent);
            }

            let empty_entries: Vec<ArtistEntry> = Vec::new();

            if let Ok(content) = serde_json::to_string_pretty(&empty_entries) {
                if let Err(e) = fs::write(file_path, content) {
                    error!("Failed to create artist database file: {}", e);
                }
            }
        }

        Self { entries }
    }

    pub fn add_entry(&mut self, entry: ArtistEntry) {
        self.entries.insert(entry.name.clone(), entry);
    }

    pub fn save(&self) -> Result<(), std::io::Error> {
        let file_path = Path::new("backend/data/artist_database.json");
        let file_path = if file_path.exists() || Path::new("backend").exists() {
            file_path
        } else {
            Path::new("data/artist_database.json")
        };

        let entries_list: Vec<ArtistEntry> = self.entries.values().cloned().collect();
        let content = serde_json::to_string_pretty(&entries_list)?;
        fs::write(file_path, content)?;
        Ok(())
    }

    /// 查找歌手条目
    pub fn find(&self, name: &str) -> Option<&ArtistEntry> {
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

    /// 简单的区域检测
    /// 优先检测日韩字符，因为汉字在日语中也很常见
    fn detect_region(title: &str, artist: &str) -> String {
        let text = format!("{} {}", title, artist);
        let mut has_kana = false;
        let mut has_hangul = false;
        let mut has_hanzi = false;
        let mut has_simplified = false;

        // 常见的简体中文特有字符（不包含在日文汉字中，或用法显著不同）
        let simplified_chars = [
            '这', '个', '们', '为', '对', '时', '说', '爱', '书', '长', '风', '车', '兴', '发',
            '东', '无', '历', '尘', '跃', '优', '伤', '鸟', '乌', '见', '贝', '见', '页', '马',
            '鱼',
        ];

        for c in text.chars() {
            let u = c as u32;
            // 平假名 & 片假名 (日语)
            if (0x3040..=0x309F).contains(&u) || (0x30A0..=0x30FF).contains(&u) {
                has_kana = true;
            }
            // 谚文 (韩语)
            else if (0xAC00..=0xD7AF).contains(&u) {
                has_hangul = true;
            }
            // 汉字 (中日韩通用)
            else if (0x4E00..=0x9FFF).contains(&u) {
                has_hanzi = true;
                if simplified_chars.contains(&c) {
                    has_simplified = true;
                }
            }
        }

        if has_hangul {
            "韩国".to_string()
        } else if has_kana {
            "日本".to_string()
        } else if has_simplified {
            "华语".to_string()
        } else if has_hanzi {
            // Hanzi without Hangul/kana/simplified → hardcoded "日本" (no user profile).
            "日本".to_string()
        } else {
            "欧美".to_string()
        }
    }

    /// 分析音乐收藏
    pub fn analyze(&self, song_list: Vec<(String, String)>) -> MusicAnalysis {
        // song_list: [(title, artist), ...]
        let mut artist_songs: HashMap<String, usize> = HashMap::new();
        let mut genre_artists: HashMap<String, Vec<String>> = HashMap::new();
        let mut region_count: HashMap<String, usize> = HashMap::new();
        let mut unknown_songs: Vec<(String, String)> = Vec::new();

        let total_songs = song_list.len();

        for (title, artist) in song_list {
            if let Some(entry) = self.find(&artist) {
                // 已知歌手 - 统计
                *artist_songs.entry(artist.clone()).or_default() += 1;

                // 按类型分组
                for genre in &entry.genres {
                    genre_artists
                        .entry(genre.clone())
                        .or_default()
                        .push(artist.clone());
                }

                // 统计地区
                *region_count.entry(entry.region.clone()).or_default() += 1;
            } else {
                // 未知歌曲 - 尝试通过标题和歌手名判断区域
                let region = Self::detect_region(&title, &artist);
                *region_count.entry(region).or_default() += 1;

                // 保留原样并记录
                unknown_songs.push((title, artist.clone()));
            }
        }

        // 找出最喜欢的歌手（按歌曲数排序，至少2首）
        let mut favorite_artists: Vec<(String, usize)> = artist_songs
            .into_iter()
            .filter(|(_, count)| *count > 1)
            .collect();
        favorite_artists.sort_by_key(|b| Reverse(b.1));
        let favorite_artists: Vec<String> = favorite_artists
            .into_iter()
            .take(10)
            .map(|(artist, _)| artist)
            .collect();

        // 生成类型分析
        let genre_analysis: Vec<ArtistGenreAnalysis> = {
            let mut analysis: Vec<_> = genre_artists
                .into_iter()
                .map(|(genre, mut artists)| {
                    artists.sort();
                    artists.dedup();
                    let count = artists.len();
                    let percentage = (count as f32 / total_songs as f32) * 100.0;

                    ArtistGenreAnalysis {
                        genre,
                        count,
                        percentage,
                    }
                })
                .filter(|a| a.percentage >= 5.0 || a.count > 1) // keep if ≥5% or count>1
                .collect();

            // 按数量排序
            analysis.sort_by_key(|b| Reverse(b.count));
            analysis
        };

        // 生成摘要
        let top_genre = genre_analysis
            .first()
            .map(|g| g.genre.as_str())
            .unwrap_or("");

        let mut regions: Vec<(&String, &usize)> = region_count.iter().collect();
        regions.sort_by(|a, b| b.1.cmp(a.1));
        let top_region = regions.first().map(|(r, _)| r.as_str()).unwrap_or("");

        let mut preference_parts = Vec::new();
        if !top_region.is_empty() {
            preference_parts.push(top_region);
        }
        if !top_genre.is_empty() {
            preference_parts.push(top_genre);
        }
        let preference_str = preference_parts.join("");

        let summary = if favorite_artists.is_empty() {
            if preference_str.is_empty() {
                format!("Collected {total_songs} songs")
            } else {
                format!("Collected {total_songs} songs, prefers {preference_str} music")
            }
        } else {
            format!(
                "Collected {} songs, prefers {} music, often listens to {}",
                total_songs,
                if preference_str.is_empty() {
                    "diverse"
                } else {
                    &preference_str
                },
                favorite_artists
                    .iter()
                    .take(3)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };

        MusicAnalysis {
            total_songs,
            artist_count: favorite_artists.len(),
            genre_analysis,
            region_distribution: region_count,
            summary,
            favorite_artists,
            unknown_songs,
        }
    }
}

impl Default for ArtistDatabase {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_database() -> ArtistDatabase {
        let mut db = ArtistDatabase {
            entries: HashMap::new(),
        };
        for name in ["周杰伦", "陈奕迅", "林俊杰"] {
            db.add_entry(ArtistEntry {
                name: name.to_string(),
                genres: vec!["流行".to_string()],
                region: "华语".to_string(),
                style: Vec::new(),
            });
        }
        db
    }

    #[test]
    fn test_find() {
        let db = test_database();
        assert!(db.find("周杰伦").is_some());
        assert!(db.find("不存在的歌手").is_none());
    }

    #[test]
    fn test_analyze() {
        let db = test_database();
        let song_list = vec![
            ("晴天".to_string(), "周杰伦".to_string()),
            ("十年".to_string(), "陈奕迅".to_string()),
            ("江南".to_string(), "林俊杰".to_string()),
        ];

        let analysis = db.analyze(song_list);
        assert_eq!(analysis.total_songs, 3);
        assert!(analysis.summary.contains("流行"));
    }
}
