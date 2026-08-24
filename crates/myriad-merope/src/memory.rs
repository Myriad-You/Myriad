use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryCandidate {
    pub id: String,
    pub content: String,
    pub importance: f64,
    pub created_at: DateTime<Utc>,
    pub last_accessed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScoredMemory {
    pub memory: MemoryCandidate,
    pub score: f64,
}

pub fn rank_memories(
    memories: impl IntoIterator<Item = MemoryCandidate>,
    now: DateTime<Utc>,
    half_life_days: f64,
    limit: usize,
) -> Vec<ScoredMemory> {
    let half_life_days = half_life_days.clamp(1.0, 365.0);
    let mut scored = memories
        .into_iter()
        .map(|memory| {
            let reference_at = memory.last_accessed_at.unwrap_or(memory.created_at);
            let age_days = (now - reference_at).num_seconds().max(0) as f64 / 86_400.0;
            let recency_score = 2f64.powf(-age_days / half_life_days);
            let score = memory.importance.clamp(0.0, 1.0) * 0.6 + recency_score * 0.4;
            ScoredMemory { memory, score }
        })
        .collect::<Vec<_>>();
    scored.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| right.memory.created_at.cmp(&left.memory.created_at))
            .then_with(|| left.memory.id.cmp(&right.memory.id))
    });
    scored.truncate(limit);
    scored
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn ranking_combines_importance_and_exponential_recency() {
        let now = Utc::now();
        let ranked = rank_memories(
            [
                MemoryCandidate {
                    id: "important-old".into(),
                    content: "old".into(),
                    importance: 1.0,
                    created_at: now - Duration::days(90),
                    last_accessed_at: None,
                },
                MemoryCandidate {
                    id: "fresh".into(),
                    content: "fresh".into(),
                    importance: 0.5,
                    created_at: now,
                    last_accessed_at: None,
                },
            ],
            now,
            30.0,
            2,
        );
        assert_eq!(ranked[0].memory.id, "fresh");
        assert!(ranked[0].score > ranked[1].score);
    }
}
