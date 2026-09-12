// SmartFilter per-platform content analysis implementations.

use serde_json::Value;
use std::cmp::Reverse;

use super::helpers::*;

impl SmartFilter {
    pub(crate) fn filter_mal(data: &Value) -> Result<SmartFilteredData, String> {
        let user = data.get("user");
        let anime_list = data
            .get("anime_list")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let manga_list = data
            .get("manga_list")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let user_summary = UserSummary {
            username: user
                .and_then(|u| u.get("name"))
                .and_then(|v| v.as_str())
                .unwrap_or("MyAnimeList user")
                .to_string(),
            user_id: user
                .and_then(|u| u.get("id"))
                .and_then(|v| v.as_i64())
                .map(|id| id.to_string())
                .unwrap_or_default(),
            level: None,
            stats: UserStats {
                follower_count: None,
                following_count: None,
                total_content: anime_list.len() + manga_list.len(),
            },
        };

        let mut subject_type_distribution = std::collections::HashMap::new();
        let mut collection_type_distribution = std::collections::HashMap::new();
        let mut tag_distribution = std::collections::HashMap::new();
        let mut subjects = Vec::new();

        let parse_entry = |entry: &Value, media_kind: &str| -> Option<MalSubjectItem> {
            let node = entry.get("node")?;
            let list_status = entry.get("list_status");
            let subject_id = node.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
            let title = node
                .get("title")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .or_else(|| {
                    node.pointer("/alternative_titles/en")
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.is_empty())
                })
                .unwrap_or("Unknown")
                .to_string();
            let cover = node
                .pointer("/main_picture/medium")
                .or_else(|| node.pointer("/main_picture/large"))
                .and_then(|v| v.as_str())
                .map(str::to_string);
            let raw_status = list_status
                .and_then(|s| s.get("status"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let collection_type = Self::mal_status_label(raw_status).to_string();
            let rate = list_status
                .and_then(|s| s.get("score"))
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            let updated_at = list_status
                .and_then(|s| s.get("updated_at"))
                .and_then(|v| v.as_str())
                .map(str::to_string);

            Some(MalSubjectItem {
                subject_id,
                title,
                subject_type: media_kind.to_string(),
                collection_type,
                rate,
                cover,
                updated_at,
            })
        };

        for entry in &anime_list {
            if let Some(item) = parse_entry(entry, "anime") {
                *subject_type_distribution
                    .entry("anime".to_string())
                    .or_insert(0) += 1;
                *collection_type_distribution
                    .entry(item.collection_type.clone())
                    .or_insert(0) += 1;
                if let Some(genres) = entry
                    .get("node")
                    .and_then(|n| n.get("genres"))
                    .and_then(|v| v.as_array())
                {
                    for genre in genres {
                        if let Some(name) = genre
                            .get("name")
                            .and_then(|v| v.as_str())
                            .filter(|s| !s.is_empty())
                        {
                            *tag_distribution.entry(name.to_string()).or_insert(0) += 1;
                        }
                    }
                }
                subjects.push(item);
            }
        }

        for entry in &manga_list {
            if let Some(item) = parse_entry(entry, "manga") {
                *subject_type_distribution
                    .entry("manga".to_string())
                    .or_insert(0) += 1;
                *collection_type_distribution
                    .entry(item.collection_type.clone())
                    .or_insert(0) += 1;
                if let Some(genres) = entry
                    .get("node")
                    .and_then(|n| n.get("genres"))
                    .and_then(|v| v.as_array())
                {
                    for genre in genres {
                        if let Some(name) = genre
                            .get("name")
                            .and_then(|v| v.as_str())
                            .filter(|s| !s.is_empty())
                        {
                            *tag_distribution.entry(name.to_string()).or_insert(0) += 1;
                        }
                    }
                }
                subjects.push(item);
            }
        }

        let mut top_rated_subjects = subjects.clone();
        top_rated_subjects.sort_by_key(|b| Reverse(b.rate));
        top_rated_subjects.truncate(20);

        let watching_subjects = subjects
            .iter()
            .filter(|item| item.collection_type == "doing")
            .take(20)
            .cloned()
            .collect::<Vec<_>>();

        let mut recent_updates = subjects.clone();
        recent_updates.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        recent_updates.truncate(20);

        let mean_score = user
            .and_then(|u| u.pointer("/anime_statistics/mean_score"))
            .and_then(|v| v.as_f64());
        let days_watched = user
            .and_then(|u| u.pointer("/anime_statistics/num_days"))
            .and_then(|v| v.as_f64());

        let collection_summary = format!(
            "MyAnimeList collection: {} titles (anime {} / manga {}), completed {}, currently {}",
            subjects.len(),
            subject_type_distribution
                .get("anime")
                .copied()
                .unwrap_or_default(),
            subject_type_distribution
                .get("manga")
                .copied()
                .unwrap_or_default(),
            collection_type_distribution
                .get("done")
                .copied()
                .unwrap_or_default(),
            collection_type_distribution
                .get("doing")
                .copied()
                .unwrap_or_default()
        );

        Ok(SmartFilteredData {
            platform: "mal".to_string(),
            user_summary,
            content_analysis: ContentAnalysis::Mal(MalAnalysis {
                collection_summary,
                subject_type_distribution,
                collection_type_distribution,
                tag_distribution,
                top_rated_subjects,
                watching_subjects,
                recent_updates,
                mean_score,
                days_watched,
            }),
            raw_unknown_content: vec![],
        })
    }

    /// Xbox 成就过滤：OpenXBL achievements bundle → 成就向画像
    ///
    /// 可拿字段全部榨干：profile settings（头像/GS/账号档/信誉/展示名）+
    /// titles（进度/封面/设备/最近游玩）。平均完成度只计「有成就系统」的作品，
    /// 避免 PC 商店无成就条目把均值压到接近 0。
    pub(crate) fn filter_xbox(data: &Value) -> Result<SmartFilteredData, String> {
        let fallback_gamertag = data
            .get("gamertag")
            .and_then(|v| v.as_str())
            .unwrap_or("Xbox Gamer")
            .to_string();
        let xuid = data
            .get("xuid")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();

        // profile settings → GS / 头像 / 展示名 / 账号档 / 信誉
        let mut gamerscore: i64 = 0;
        let mut avatar: Option<String> = None;
        let mut display_gamertag: Option<String> = None;
        let mut account_tier: Option<String> = None;
        let mut reputation: Option<String> = None;
        if let Some(settings) = data
            .pointer("/profile/profileUsers/0/settings")
            .and_then(|v| v.as_array())
        {
            for s in settings {
                let id = s.get("id").and_then(|v| v.as_str()).unwrap_or("");
                let val = s.get("value").and_then(|v| v.as_str()).unwrap_or("");
                if val.is_empty() {
                    continue;
                }
                match id {
                    "Gamerscore" => {
                        gamerscore = val.parse::<i64>().unwrap_or(0);
                    }
                    "GameDisplayPicRaw" | "PublicGamerpic" => {
                        if avatar.is_none() {
                            avatar = Some(Self::normalize_xbox_media_url(val));
                        }
                    }
                    "UniqueModernGamertag" | "ModernGamertag" | "Gamertag" => {
                        // 优先 UniqueModern（含 #suffix），已有更完整值则不覆盖
                        if display_gamertag
                            .as_ref()
                            .map(|g| !g.contains('#'))
                            .unwrap_or(true)
                            || id == "UniqueModernGamertag"
                        {
                            display_gamertag = Some(val.to_string());
                        }
                    }
                    "AccountTier" => account_tier = Some(val.to_string()),
                    "XboxOneRep" => reputation = Some(val.to_string()),
                    _ => {}
                }
            }
        }

        let gamertag = display_gamertag
            .clone()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(fallback_gamertag);

        let raw_titles = data
            .pointer("/achievements/titles")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let titles: Vec<XboxTitleItem> = raw_titles
            .iter()
            .filter_map(|t| {
                let name = t.get("name").and_then(|v| v.as_str())?.to_string();
                // 过滤掉非游戏条目（如 App）
                if t.get("type").and_then(|v| v.as_str()) == Some("App") {
                    return None;
                }
                // 过滤明显启动器/壳应用
                let name_l = name.to_lowercase();
                if name_l.contains("launcher") || name_l.ends_with(" app") {
                    return None;
                }
                let ach = t.get("achievement");
                let earned = ach
                    .and_then(|a| a.get("currentAchievements"))
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);
                let total = ach
                    .and_then(|a| a.get("totalAchievements"))
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);
                let gs_earned = ach
                    .and_then(|a| a.get("currentGamerscore"))
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);
                let gs_total = ach
                    .and_then(|a| a.get("totalGamerscore"))
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);
                let progress = ach
                    .and_then(|a| a.get("progressPercentage"))
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
                let devices = t
                    .get("devices")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|d| d.as_str().map(str::to_string))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                let display_image = t
                    .get("displayImage")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .map(Self::normalize_xbox_media_url);
                Some(XboxTitleItem {
                    title_id: t
                        .get("titleId")
                        .and_then(|v| {
                            v.as_str()
                                .map(str::to_string)
                                .or_else(|| v.as_u64().map(|n| n.to_string()))
                        })
                        .unwrap_or_default(),
                    name,
                    display_image,
                    achievements_earned: earned,
                    achievements_total: total,
                    gamerscore_earned: gs_earned,
                    gamerscore_total: gs_total,
                    progress,
                    last_played: t
                        .pointer("/titleHistory/lastTimePlayed")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                    devices,
                })
            })
            .collect();

        let games_count = titles.len();
        // 只在「有成就系统」的作品上算完成度，避免无成就 PC 条目稀释均值
        let achievement_titles: Vec<&XboxTitleItem> = titles
            .iter()
            .filter(|t| t.achievements_total > 0 || t.gamerscore_total > 0)
            .collect();
        let achievement_games = achievement_titles.len();
        let completed_games = titles.iter().filter(|t| t.progress >= 100.0).count();
        let total_achievements_earned: i64 = titles.iter().map(|t| t.achievements_earned).sum();
        let total_achievements_available: i64 = achievement_titles
            .iter()
            .map(|t| t.achievements_total)
            .sum();
        let average_completion = if achievement_games > 0 {
            achievement_titles.iter().map(|t| t.progress).sum::<f64>() / achievement_games as f64
        } else {
            0.0
        };

        // 硬核指数 0-100：完成度主导 + 全成就密度 + GS 规模（log）+ 成就解锁量
        // 目标：轻度玩家（几十 GS）落在 10-30，中坚 40-70，猎人 80+
        let hardcore_score = {
            let completion_part = (average_completion * 0.45).clamp(0.0, 45.0);
            let complete_ratio = if achievement_games > 0 {
                completed_games as f64 / achievement_games as f64
            } else {
                0.0
            };
            let complete_part = (complete_ratio * 25.0).clamp(0.0, 25.0);
            let gs_part = if gamerscore > 0 {
                // log10(1+gs) / log10(1+100000) * 20 → 100k GS 打满 20 分
                let ratio =
                    ((1.0 + gamerscore as f64).ln() / (1.0_f64 + 100_000.0).ln()).clamp(0.0, 1.0);
                ratio * 20.0
            } else {
                0.0
            };
            let ach_part = if total_achievements_earned > 0 {
                // 500 成就打满 10 分
                ((total_achievements_earned as f64 / 500.0).min(1.0)) * 10.0
            } else {
                0.0
            };
            (completion_part + complete_part + gs_part + ach_part)
                .round()
                .clamp(0.0, 100.0) as i64
        };

        let mut recent_titles = titles.clone();
        // 最近游玩：有 last_played 的排前面；同时间优先有封面的
        recent_titles.sort_by(|a, b| {
            b.last_played
                .cmp(&a.last_played)
                .then_with(|| b.display_image.is_some().cmp(&a.display_image.is_some()))
        });
        recent_titles.truncate(20);

        // 完成度排序时只看有进度的游戏，避免一堆 0% 噪音
        let mut top_completed = titles;
        top_completed.retain(|t| t.achievements_earned > 0 || t.progress > 0.0);
        top_completed.sort_by(|a, b| {
            b.progress
                .partial_cmp(&a.progress)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(b.gamerscore_earned.cmp(&a.gamerscore_earned))
        });
        top_completed.truncate(20);

        let gaming_summary = format!(
            "Xbox Gamerscore {}, {} games ({} with achievements), {} completed, unlocked {}/{} achievements, average completion {:.1}%",
            gamerscore,
            games_count,
            achievement_games,
            completed_games,
            total_achievements_earned,
            total_achievements_available,
            average_completion
        );

        Ok(SmartFilteredData {
            platform: "xbox".to_string(),
            user_summary: UserSummary {
                username: gamertag.clone(),
                user_id: xuid,
                level: None,
                stats: UserStats {
                    follower_count: None,
                    following_count: None,
                    total_content: games_count,
                },
            },
            content_analysis: ContentAnalysis::Xbox(XboxAnalysis {
                gaming_summary,
                gamerscore,
                games_count,
                achievement_games,
                completed_games,
                total_achievements_earned,
                total_achievements_available,
                average_completion,
                hardcore_score,
                account_tier,
                avatar,
                display_gamertag: Some(gamertag),
                reputation,
                recent_titles,
                top_completed_titles: top_completed,
            }),
            raw_unknown_content: vec![],
        })
    }

    /// PSN 奖杯过滤：trophySummary + trophyTitles + social_metadata → 奖杯向画像
    pub(crate) fn filter_psn(data: &Value) -> Result<SmartFilteredData, String> {
        let fallback_id = data
            .get("online_id")
            .and_then(|v| v.as_str())
            .unwrap_or("PSN player")
            .to_string();
        let account_id = data
            .get("account_id")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();

        // social_metadata：头像 / 展示名 / Plus
        let social = data.get("social_metadata");
        let mut avatar: Option<String> = social
            .and_then(|s| {
                s.get("avatarUrl")
                    .or_else(|| s.get("avatar"))
                    .or_else(|| s.pointer("/avatarUrls/0/avatarUrl"))
            })
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(Self::normalize_https_media_url);
        let display_from_social = social
            .and_then(|s| s.get("onlineId").or_else(|| s.get("online_id")))
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let is_plus = social.and_then(|s| {
            s.get("isPlus").or_else(|| s.get("plus")).and_then(|v| {
                v.as_bool().or_else(|| {
                    v.as_i64()
                        .map(|n| n != 0)
                        .or_else(|| v.as_str().map(|s| s == "true" || s == "1"))
                })
            })
        });
        // 有时头像在 profile 结构外
        if avatar.is_none() {
            avatar = data
                .pointer("/social_metadata/profilePictureUrls/0/profilePictureUrl")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(Self::normalize_https_media_url);
        }

        let online_id = display_from_social
            .clone()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(fallback_id);

        let summary = data.get("trophy_summary");
        let trophy_level = summary
            .and_then(|s| s.get("trophyLevel"))
            .and_then(|v| {
                v.as_i64()
                    .or_else(|| v.as_str().and_then(|s| s.parse::<i64>().ok()))
            })
            .unwrap_or(0);
        let earned = summary.and_then(|s| s.get("earnedTrophies"));
        let count_of = |kind: &str| -> i64 {
            earned
                .and_then(|e| e.get(kind))
                .and_then(|v| v.as_i64())
                .unwrap_or(0)
        };
        let platinum_count = count_of("platinum");
        let gold_count = count_of("gold");
        let silver_count = count_of("silver");
        let bronze_count = count_of("bronze");
        let total_trophies = platinum_count + gold_count + silver_count + bronze_count;

        let raw_titles = data
            .get("trophy_titles")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let titles: Vec<PsnTitleItem> = raw_titles
            .iter()
            .filter_map(|t| {
                let name = t
                    .get("trophyTitleName")
                    .and_then(|v| v.as_str())?
                    .to_string();
                // 跳过空壳名称
                if name.trim().is_empty() {
                    return None;
                }
                let earned = t.get("earnedTrophies");
                let earned_of = |kind: &str| -> i64 {
                    earned
                        .and_then(|e| e.get(kind))
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0)
                };
                let icon_url = t
                    .get("trophyTitleIconUrl")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .map(Self::normalize_https_media_url);
                Some(PsnTitleItem {
                    name,
                    platform: t
                        .get("trophyTitlePlatform")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    icon_url,
                    progress: t.get("progress").and_then(|v| v.as_i64()).unwrap_or(0),
                    earned_platinum: earned_of("platinum"),
                    earned_gold: earned_of("gold"),
                    earned_silver: earned_of("silver"),
                    earned_bronze: earned_of("bronze"),
                    last_updated: t
                        .get("lastUpdatedDateTime")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                })
            })
            .collect();

        let games_count = titles.len();
        let completed_games = titles.iter().filter(|t| t.progress >= 100).count();
        // 只对有进度或有奖杯的作品算平均完成度，避免 0% 占位稀释
        let progressed: Vec<&PsnTitleItem> = titles
            .iter()
            .filter(|t| {
                t.progress > 0
                    || t.earned_platinum + t.earned_gold + t.earned_silver + t.earned_bronze > 0
            })
            .collect();
        let average_progress = if !progressed.is_empty() {
            progressed.iter().map(|t| t.progress as f64).sum::<f64>() / progressed.len() as f64
        } else if games_count > 0 {
            titles.iter().map(|t| t.progress as f64).sum::<f64>() / games_count as f64
        } else {
            0.0
        };

        // 猎人指数：白金主导 + 等级 + 完成度 + 通关密度
        // 目标：0 白金轻度 ~10-30，数枚白金 40-70，双位数白金/高完成 80+
        let hardcore_score = {
            let plat_part = ((platinum_count as f64) * 4.0).min(40.0);
            let level_part = if trophy_level > 0 {
                // lv 1→~0, lv 100→~20, lv 400→~25 封顶
                ((trophy_level as f64).ln() / (400.0_f64).ln() * 25.0).clamp(0.0, 25.0)
            } else {
                0.0
            };
            let completion_part = (average_progress * 0.25).clamp(0.0, 25.0);
            let complete_ratio = if games_count > 0 {
                completed_games as f64 / games_count as f64
            } else {
                0.0
            };
            let complete_part = (complete_ratio * 10.0).clamp(0.0, 10.0);
            (plat_part + level_part + completion_part + complete_part)
                .round()
                .clamp(0.0, 100.0) as i64
        };

        let mut recent_titles = titles.clone();
        recent_titles.sort_by(|a, b| {
            b.last_updated
                .cmp(&a.last_updated)
                .then_with(|| b.icon_url.is_some().cmp(&a.icon_url.is_some()))
        });
        recent_titles.truncate(20);

        let mut top_completed = titles;
        // 完成度排序：有进度优先，白金优先
        top_completed.retain(|t| {
            t.progress > 0
                || t.earned_platinum + t.earned_gold + t.earned_silver + t.earned_bronze > 0
        });
        top_completed.sort_by(|a, b| {
            b.progress
                .cmp(&a.progress)
                .then(b.earned_platinum.cmp(&a.earned_platinum))
                .then(
                    (b.earned_gold + b.earned_silver + b.earned_bronze)
                        .cmp(&(a.earned_gold + a.earned_silver + a.earned_bronze)),
                )
        });
        top_completed.truncate(20);

        let trophy_summary_text = format!(
            "PSN trophy level {}, platinum {} / gold {} / silver {} / bronze {} ({} total), {} games, {} at 100%, average completion {:.1}%",
            trophy_level,
            platinum_count,
            gold_count,
            silver_count,
            bronze_count,
            total_trophies,
            games_count,
            completed_games,
            average_progress
        );

        Ok(SmartFilteredData {
            platform: "psn".to_string(),
            user_summary: UserSummary {
                username: online_id.clone(),
                user_id: account_id,
                level: Some(format!("Lv.{trophy_level}")),
                stats: UserStats {
                    follower_count: None,
                    following_count: None,
                    total_content: games_count,
                },
            },
            content_analysis: ContentAnalysis::Psn(PsnAnalysis {
                trophy_summary_text,
                trophy_level,
                platinum_count,
                gold_count,
                silver_count,
                bronze_count,
                total_trophies,
                games_count,
                completed_games,
                average_progress,
                hardcore_score,
                avatar,
                display_online_id: Some(online_id),
                is_plus,
                recent_titles,
                top_completed_titles: top_completed,
            }),
            raw_unknown_content: vec![],
        })
    }

    pub(crate) fn filter_x(data: &Value) -> Result<SmartFilteredData, String> {
        let user = data.get("user").unwrap_or(&Value::Null);
        let username = user
            .get("username")
            .or_else(|| user.get("name"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        let user_id = user
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let metrics = user.get("public_metrics").cloned().unwrap_or(Value::Null);
        let follower_count = metrics.get("followers_count").and_then(|v| v.as_i64());
        let following_count = metrics.get("following_count").and_then(|v| v.as_i64());
        let tweet_count_metric = metrics
            .get("tweet_count")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);

        let tweets = data
            .get("tweets")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let mut language_distribution = std::collections::HashMap::new();
        let mut total_likes = 0i64;
        let mut total_retweets = 0i64;
        let mut total_replies = 0i64;
        let mut total_impressions = 0i64;

        let mut post_items: Vec<XPostItem> = Vec::new();
        for tweet in &tweets {
            let id = tweet
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let text = tweet
                .get("text")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let created_at = tweet
                .get("created_at")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let pm = tweet.get("public_metrics").cloned().unwrap_or(Value::Null);
            let like_count = pm.get("like_count").and_then(|v| v.as_i64()).unwrap_or(0);
            let retweet_count = pm
                .get("retweet_count")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            let reply_count = pm.get("reply_count").and_then(|v| v.as_i64()).unwrap_or(0);
            let impression_count = pm
                .get("impression_count")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);

            total_likes += like_count;
            total_retweets += retweet_count;
            total_replies += reply_count;
            total_impressions += impression_count;

            if let Some(lang) = tweet.get("lang").and_then(|v| v.as_str()) {
                *language_distribution.entry(lang.to_string()).or_insert(0) += 1;
            }

            post_items.push(XPostItem {
                id,
                text,
                created_at,
                like_count,
                retweet_count,
                reply_count,
                impression_count,
            });
        }

        let mut top_posts = post_items.clone();
        top_posts.sort_by(|a, b| {
            (b.like_count + b.retweet_count * 2).cmp(&(a.like_count + a.retweet_count * 2))
        });
        top_posts.truncate(10);

        let recent_posts: Vec<XPostItem> = post_items.into_iter().take(20).collect();
        let fetched_count = tweets.len();

        let post_summary = format!(
            "X account @{} has about {} posts; fetched {} timeline items, {} likes, {} reposts, {} replies",
            username, tweet_count_metric, fetched_count, total_likes, total_retweets, total_replies
        );

        // 关注列表：按粉丝数排序取样本，简介截断以控制 token
        const MAX_FOLLOWING_SAMPLE: usize = 50;
        const MAX_FOLLOWING_DESC_CHARS: usize = 80;

        let following = data
            .get("following")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let following_fetched = following.len();

        let mut following_sample: Vec<XFollowingItem> = following
            .iter()
            .map(|account| XFollowingItem {
                username: account
                    .get("username")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                name: account
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                description: account
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .chars()
                    .take(MAX_FOLLOWING_DESC_CHARS)
                    .collect(),
                follower_count: account
                    .pointer("/public_metrics/followers_count")
                    .and_then(|v| v.as_i64()),
                verified: account
                    .get("verified")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
                profile_image_url: account
                    .get("profile_image_url")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .map(str::to_string),
            })
            .collect();
        // Known follower counts first (desc); missing metrics last (not fake 0)
        following_sample.sort_by(|a, b| match (a.follower_count, b.follower_count) {
            (Some(x), Some(y)) => y.cmp(&x),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        });
        following_sample.truncate(MAX_FOLLOWING_SAMPLE);

        let following_summary = if following_fetched > 0 {
            format!(
                "Following {} accounts (fetched {}), sample of top {} by follower count; follows reflect interest circles",
                following_count.unwrap_or(following_fetched as i64),
                following_fetched,
                following_sample.len()
            )
        } else {
            String::new()
        };

        let user_name = user
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or(&username)
            .to_string();
        let user_avatar = user
            .get("profile_image_url")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_string);

        Ok(SmartFilteredData {
            platform: "x".to_string(),
            user_summary: UserSummary {
                username,
                user_id,
                level: None,
                stats: UserStats {
                    follower_count,
                    following_count,
                    total_content: fetched_count,
                },
            },
            content_analysis: ContentAnalysis::X(XAnalysis {
                post_summary,
                user_name,
                user_avatar,
                following_summary,
                following_sample,
                engagement_stats: XEngagementStats {
                    // Profile tweet_count when present; else scraped sample size
                    total_posts: if tweet_count_metric > 0 {
                        tweet_count_metric as usize
                    } else {
                        fetched_count
                    },
                    total_likes_received: total_likes,
                    total_retweets_received: total_retweets,
                    total_replies_received: total_replies,
                    total_impressions,
                    // `liked_posts_count` 恒 0；字段保留给报告结构
                    liked_posts_count: 0,
                },
                recent_posts,
                top_posts,
                language_distribution,
            }),
            raw_unknown_content: vec![],
        })
    }

    pub(crate) fn filter_discord(data: &Value) -> Result<SmartFilteredData, String> {
        let user = data.get("user").unwrap_or(&Value::Null);

        let user_id = user
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let username = user
            .get("username")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        let global_name = user
            .get("global_name")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());
        let display_name = global_name.clone().unwrap_or_else(|| username.clone());

        let premium_type = user
            .get("premium_type")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let level = match premium_type {
            1 => Some("Nitro Classic".to_string()),
            2 => Some("Nitro".to_string()),
            3 => Some("Nitro Basic".to_string()),
            _ => None,
        };

        // 画像素材（均来自 identify scope）
        let avatar_url = user
            .get("avatar")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .filter(|_| !user_id.is_empty())
            .map(|hash| {
                let ext = if hash.starts_with("a_") { "gif" } else { "png" };
                format!(
                    "https://cdn.discordapp.com/avatars/{}/{}.{}?size=256",
                    user_id, hash, ext
                )
            });
        let banner_url = user
            .get("banner")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .filter(|_| !user_id.is_empty())
            .map(|hash| {
                let ext = if hash.starts_with("a_") { "gif" } else { "png" };
                format!(
                    "https://cdn.discordapp.com/banners/{}/{}.{}?size=600",
                    user_id, hash, ext
                )
            });
        let accent_color = user
            .get("accent_color")
            .and_then(|v| v.as_u64())
            .map(|c| format!("#{:06X}", c & 0xFF_FFFF));
        let badges = Self::discord_public_flags_badges(
            user.get("public_flags")
                .and_then(|v| v.as_u64())
                .unwrap_or(0),
        );
        let mfa_enabled = user
            .get("mfa_enabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let (created_at, account_age_years) = Self::discord_snowflake_created(&user_id);

        let guilds = data
            .get("guilds")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let connections_raw = data
            .get("connections")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let mut owned_guild_count = 0usize;
        let mut admin_guild_count = 0usize;
        let mut manage_guild_count = 0usize;
        let mut community_guild_count = 0usize;
        let mut partnered_or_verified_count = 0usize;
        let mut total_member_reach: u64 = 0;
        let mut total_online_reach: u64 = 0;
        let mut guilds_preview = Vec::new();

        for guild in &guilds {
            let id = guild
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let name = guild
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            let owner = guild
                .get("owner")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if owner {
                owned_guild_count += 1;
            }

            let perms =
                Self::discord_permissions_highlight(guild.get("permissions").and_then(|v| {
                    v.as_str()
                        .and_then(|s| s.parse::<u64>().ok())
                        .or_else(|| v.as_u64())
                }));
            if perms.iter().any(|p| p == "ADMINISTRATOR") {
                admin_guild_count += 1;
            }
            if perms
                .iter()
                .any(|p| p == "MANAGE_GUILD" || p == "ADMINISTRATOR")
            {
                manage_guild_count += 1;
            }

            // with_counts=true 附带的真实规模 —— 用来排序有效社区、算总触达
            let member_count = guild
                .get("approximate_member_count")
                .and_then(|v| v.as_u64());
            let presence_count = guild
                .get("approximate_presence_count")
                .and_then(|v| v.as_u64());
            total_member_reach = total_member_reach.saturating_add(member_count.unwrap_or(0));
            total_online_reach = total_online_reach.saturating_add(presence_count.unwrap_or(0));

            let features: Vec<String> = guild
                .get("features")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|f| f.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();
            let feature_highlight = Self::discord_guild_feature_highlight(&features);
            if feature_highlight.iter().any(|f| f == "COMMUNITY") {
                community_guild_count += 1;
            }
            if feature_highlight
                .iter()
                .any(|f| f == "PARTNERED" || f == "VERIFIED")
            {
                partnered_or_verified_count += 1;
            }

            let icon_url = match (
                guild
                    .get("icon")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty()),
                id.is_empty(),
            ) {
                (Some(icon), false) => {
                    let ext = if icon.starts_with("a_") { "gif" } else { "png" };
                    Some(format!(
                        "https://cdn.discordapp.com/icons/{}/{}.{}",
                        id, icon, ext
                    ))
                }
                _ => None,
            };

            guilds_preview.push(DiscordGuildItem {
                id,
                name,
                icon_url,
                owner,
                permissions_highlight: perms,
                member_count,
                presence_count,
                feature_highlight,
            });
        }

        // 展示优先级：自建 > 管理 > 规模（成员数）> 名称
        // 把最能体现社区身份的服务器顶到前面，弱化只是路人成员的噪声小服。
        guilds_preview.sort_by(|a, b| {
            b.owner
                .cmp(&a.owner)
                .then_with(|| {
                    let a_admin = a.permissions_highlight.iter().any(|p| p == "ADMINISTRATOR");
                    let b_admin = b.permissions_highlight.iter().any(|p| p == "ADMINISTRATOR");
                    b_admin.cmp(&a_admin)
                })
                .then_with(|| {
                    b.member_count
                        .unwrap_or(0)
                        .cmp(&a.member_count.unwrap_or(0))
                })
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        guilds_preview.truncate(30);

        let mut connections = Vec::new();
        let mut linked_platforms = Vec::new();
        for conn in &connections_raw {
            let conn_type = conn
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            let name = conn
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let id = conn
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let verified = conn
                .get("verified")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let visibility = conn.get("visibility").and_then(|v| v.as_i64()).unwrap_or(0);

            if !conn_type.is_empty()
                && conn_type != "unknown"
                && !linked_platforms.iter().any(|p| p == &conn_type)
            {
                linked_platforms.push(conn_type.clone());
            }

            connections.push(DiscordConnectionItem {
                r#type: conn_type,
                name,
                id,
                verified,
                visibility,
            });
        }
        linked_platforms.sort();

        let verified_connection_count = connections.iter().filter(|c| c.verified).count();

        // 交叉校验：raw 中可注入 myriad_cross_refs（由 profile 拉取时写入）
        let cross_refs = data
            .get("myriad_cross_refs")
            .cloned()
            .unwrap_or(Value::Null);
        let steam_id_cfg = cross_refs
            .get("steam_id")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty());
        let github_username_cfg = cross_refs
            .get("github_username")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty());

        let mut cross_check = std::collections::HashMap::new();

        let steam_conn = connections.iter().find(|c| c.r#type == "steam");
        cross_check.insert(
            "steam".to_string(),
            DiscordCrossCheck {
                discord_linked: steam_conn.is_some(),
                myriad_configured: steam_id_cfg.is_some(),
                id_match: match (steam_conn, steam_id_cfg) {
                    (Some(c), Some(sid)) => {
                        let sid_l = sid.to_lowercase();
                        Some(
                            (!c.id.is_empty() && c.id.eq_ignore_ascii_case(sid))
                                || (!c.name.is_empty() && c.name.eq_ignore_ascii_case(sid))
                                || c.id.to_lowercase().contains(&sid_l)
                                || sid_l.contains(&c.id.to_lowercase()),
                        )
                    }
                    _ => None,
                },
                name_match: None,
            },
        );

        let github_conn = connections.iter().find(|c| c.r#type == "github");
        cross_check.insert(
            "github".to_string(),
            DiscordCrossCheck {
                discord_linked: github_conn.is_some(),
                myriad_configured: github_username_cfg.is_some(),
                id_match: None,
                name_match: match (github_conn, github_username_cfg) {
                    (Some(c), Some(gh)) => {
                        let gh_l = gh.trim_start_matches('@').to_lowercase();
                        Some(
                            (!c.name.is_empty() && c.name.eq_ignore_ascii_case(&gh_l))
                                || (!c.id.is_empty() && c.id.eq_ignore_ascii_case(&gh_l)),
                        )
                    }
                    _ => None,
                },
            },
        );

        let reach_phrase = if total_member_reach >= 10_000 {
            format!("reach ~{:.1}k people", total_member_reach as f64 / 1_000.0)
        } else if total_member_reach > 0 {
            format!("reach ~{total_member_reach} people")
        } else {
            String::new()
        };
        let age_phrase = account_age_years
            .filter(|y| *y >= 1)
            .map(|y| format!(", {y}-year-old account"))
            .unwrap_or_default();
        let community_summary = format!(
            "Discord user {display_name}{age_phrase} joined {} servers (owned {}, manages {}{}), linked {} third-party accounts ({} verified)",
            guilds.len(),
            owned_guild_count,
            manage_guild_count,
            if reach_phrase.is_empty() {
                String::new()
            } else {
                format!(", {reach_phrase}")
            },
            connections.len(),
            verified_connection_count
        );

        let profile = DiscordProfile {
            display_name: display_name.clone(),
            username,
            avatar_url,
            banner_url,
            accent_color,
            nitro: level.clone(),
            created_at,
            account_age_years,
            badges,
            mfa_enabled,
        };

        Ok(SmartFilteredData {
            platform: "discord".to_string(),
            user_summary: UserSummary {
                username: display_name,
                user_id,
                level,
                stats: UserStats {
                    follower_count: None,
                    following_count: None,
                    total_content: guilds.len(),
                },
            },
            content_analysis: ContentAnalysis::Discord(DiscordAnalysis {
                community_summary,
                profile,
                guild_stats: DiscordGuildStats {
                    guild_count: guilds.len(),
                    owned_guild_count,
                    admin_guild_count,
                    manage_guild_count,
                    total_member_reach,
                    total_online_reach,
                    community_guild_count,
                    partnered_or_verified_count,
                },
                guilds_preview,
                connections,
                identity_graph: DiscordIdentityGraph {
                    linked_platforms,
                    cross_check,
                },
            }),
            raw_unknown_content: vec![],
        })
    }

    /// 从 Discord permissions 位掩码提取关注权限标签
    pub(crate) fn discord_permissions_highlight(permissions: Option<u64>) -> Vec<String> {
        let Some(bits) = permissions else {
            return Vec::new();
        };
        // https://discord.com/developers/docs/topics/permissions
        const ADMINISTRATOR: u64 = 1 << 3;
        const MANAGE_CHANNELS: u64 = 1 << 4;
        const MANAGE_GUILD: u64 = 1 << 5;
        const MANAGE_ROLES: u64 = 1 << 28;
        const MANAGE_MESSAGES: u64 = 1 << 13;
        const KICK_MEMBERS: u64 = 1 << 1;
        const BAN_MEMBERS: u64 = 1 << 2;

        let mut out = Vec::new();
        if bits & ADMINISTRATOR != 0 {
            out.push("ADMINISTRATOR".to_string());
            return out;
        }
        if bits & MANAGE_GUILD != 0 {
            out.push("MANAGE_GUILD".to_string());
        }
        if bits & MANAGE_CHANNELS != 0 {
            out.push("MANAGE_CHANNELS".to_string());
        }
        if bits & MANAGE_ROLES != 0 {
            out.push("MANAGE_ROLES".to_string());
        }
        if bits & MANAGE_MESSAGES != 0 {
            out.push("MANAGE_MESSAGES".to_string());
        }
        if bits & KICK_MEMBERS != 0 {
            out.push("KICK_MEMBERS".to_string());
        }
        if bits & BAN_MEMBERS != 0 {
            out.push("BAN_MEMBERS".to_string());
        }
        out
    }

    /// 从 Discord snowflake ID 解出创建时间（RFC3339）与账号年龄（整年）。
    /// Discord ID 高 42 位是自 Discord epoch(2015-01-01) 起的毫秒时间戳，
    /// 无需任何 API 调用即可得到账号/服务器的诞生日期。
    pub(crate) fn discord_snowflake_created(id: &str) -> (Option<String>, Option<i64>) {
        const DISCORD_EPOCH_MS: i64 = 1_420_070_400_000;
        let Some(snowflake) = id.trim().parse::<u64>().ok().filter(|n| *n > 0) else {
            return (None, None);
        };
        let ts_ms = (snowflake >> 22) as i64 + DISCORD_EPOCH_MS;
        let Some(created) = chrono::DateTime::from_timestamp_millis(ts_ms) else {
            return (None, None);
        };
        let now = chrono::Utc::now();
        // 用天数换算年龄，避免闰年月份运算的边角
        let years = ((now - created).num_days() / 365).max(0);
        (Some(created.to_rfc3339()), Some(years))
    }

    /// 解 public_flags 位掩码 → 面向展示的徽章标签。
    /// https://discord.com/developers/docs/resources/user#user-object-user-flags
    pub(crate) fn discord_public_flags_badges(flags: u64) -> Vec<String> {
        const HYPESQUAD_EVENTS: u64 = 1 << 2;
        const BUG_HUNTER_1: u64 = 1 << 3;
        const HOUSE_BRAVERY: u64 = 1 << 6;
        const HOUSE_BRILLIANCE: u64 = 1 << 7;
        const HOUSE_BALANCE: u64 = 1 << 8;
        const EARLY_SUPPORTER: u64 = 1 << 9;
        const BUG_HUNTER_2: u64 = 1 << 14;
        const VERIFIED_DEVELOPER: u64 = 1 << 17;
        const CERTIFIED_MODERATOR: u64 = 1 << 18;
        const ACTIVE_DEVELOPER: u64 = 1 << 22;

        let mut out = Vec::new();
        if flags & HOUSE_BRAVERY != 0 {
            out.push("HypeSquad Bravery".to_string());
        }
        if flags & HOUSE_BRILLIANCE != 0 {
            out.push("HypeSquad Brilliance".to_string());
        }
        if flags & HOUSE_BALANCE != 0 {
            out.push("HypeSquad Balance".to_string());
        }
        if flags & HYPESQUAD_EVENTS != 0 {
            out.push("HypeSquad Events".to_string());
        }
        if flags & EARLY_SUPPORTER != 0 {
            out.push("Early Supporter".to_string());
        }
        if flags & ACTIVE_DEVELOPER != 0 {
            out.push("Active Developer".to_string());
        }
        if flags & VERIFIED_DEVELOPER != 0 {
            out.push("Early Verified Bot Dev".to_string());
        }
        if flags & (BUG_HUNTER_1 | BUG_HUNTER_2) != 0 {
            out.push("Bug Hunter".to_string());
        }
        if flags & CERTIFIED_MODERATOR != 0 {
            out.push("Moderator Alumni".to_string());
        }
        out
    }

    /// 从 guild features 数组挑出能体现社区身份/规格的标签，忽略其余噪声特性。
    pub(crate) fn discord_guild_feature_highlight(features: &[String]) -> Vec<String> {
        // 仅保留有含义、面向读者的少量特性；其余（如 NEWS、THREADS_ENABLED）丢弃。
        const KEEP: &[&str] = &["PARTNERED", "VERIFIED", "COMMUNITY", "DISCOVERABLE"];
        let mut out = Vec::new();
        for f in features {
            let upper = f.to_ascii_uppercase();
            if KEEP.contains(&upper.as_str()) && !out.contains(&upper) {
                out.push(upper);
            }
        }
        out
    }
}
