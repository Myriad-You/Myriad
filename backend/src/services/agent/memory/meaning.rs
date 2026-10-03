//! What a memory means, as a vector, so recall finds it by meaning as well as
//! by its words. Asked "can you recommend some cultural events", a person
//! thinks of the one who wanted language exchanges although no word is
//! shared; words alone found it among a few dozen memories and lost it among
//! a thousand (LongMemEval, needed memories found: 54 of 58 alone, 43 among
//! three hundred, 39 among a thousand). By meaning as well: 54 and 52, as
//! alone; in Chinese, answer lines among the first five 17 of 22 against 9
//! by words (`perplexity/pplx-embed-v1-0.6b`, the cheapest that held up).
//!
//! Vectors live in `agent_memory_embeddings`, one per memory, under the model
//! that made them and a digest of the text: an edited memory, or another
//! model, is embedded again. Recall embeds what it is asked in the same
//! request as a few memories still without one, so memories gain vectors as
//! they come up and nothing runs in the background. When no model is set, or
//! it does not answer in time, recall goes by words alone.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use sea_orm::{ConnectionTrait, DatabaseBackend, DbErr, Statement};
use sha2::{Digest, Sha256};

use crate::models::entities::agent_memories;
use crate::services::analyzer::AiAnalyzer;
use crate::services::retained_cache::RetainedCache;

/// Past this, recall goes by words: a reply does not wait on meaning.
const ASK_WITHIN: Duration = Duration::from_secs(2);
/// Memories without a vector embedded alongside one question.
const FILL_PER_ASK: usize = 63;
/// A memory stands out by meaning when it is this many deviations above how
/// close the rest of them are: what closeness means differs by model, how
/// far above the rest does not.
pub const STANDS_OUT: f64 = 2.0;
/// Fewer memories with vectors than this give no sense of the rest.
const AT_LEAST: usize = 8;

/// The embedding model in use, when one is set.
pub struct Meaning {
    analyzer: AiAnalyzer,
    model: String,
}

impl Meaning {
    pub async fn current() -> Option<Self> {
        let analyzer =
            crate::services::ai::create_lite_embedding_analyzer_with_timeout(Some(ASK_WITHIN))
                .await?;
        let model = crate::GLOBAL_DYNAMIC_CONFIG
            .read()
            .await
            .aux_embedding_model
            .trim()
            .to_string();
        Some(Self { analyzer, model })
    }
}

type Vector = Arc<Vec<f32>>;

/// What was asked lately, by model and text: a message and its cues are
/// embedded once, then recalled with many times.
static ASKED: LazyLock<Mutex<RetainedCache<(String, String), Vector>>> =
    LazyLock::new(|| Mutex::new(RetainedCache::new(512, Duration::from_secs(600))));
/// Memory texts embedded lately, by model and digest: the same words need
/// one request, whoever they are about.
static KNOWN: LazyLock<Mutex<RetainedCache<(String, String), Vector>>> =
    LazyLock::new(|| Mutex::new(RetainedCache::new(4096, Duration::from_secs(3600))));

fn cached(
    cache: &Mutex<RetainedCache<(String, String), Vector>>,
    model: &str,
    key: &str,
) -> Option<Vector> {
    cache
        .lock()
        .ok()?
        .get(&(model.to_string(), key.to_string()))
        .cloned()
}

fn keep(cache: &Mutex<RetainedCache<(String, String), Vector>>, model: &str, key: &str, v: Vector) {
    if let Ok(mut cache) = cache.lock() {
        cache.insert((model.to_string(), key.to_string()), v);
    }
}

/// Of the text as embedded.
pub fn digest(text: &str) -> String {
    let hash = Sha256::digest(text.trim().as_bytes());
    hash.iter()
        .take(16)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn to_bytes(vector: &[f32]) -> Vec<u8> {
    vector
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

fn from_bytes(bytes: &[u8]) -> Option<Vec<f32>> {
    (bytes.len() % 4 == 0 && !bytes.is_empty()).then(|| {
        bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|chunk| f32::from_le_bytes(*chunk))
            .collect()
    })
}

pub fn cosine(left: &[f32], right: &[f32]) -> f64 {
    if left.len() != right.len() || left.is_empty() {
        return 0.0;
    }
    let (mut dot, mut l, mut r) = (0.0f64, 0.0f64, 0.0f64);
    for (a, b) in left.iter().zip(right) {
        let (a, b) = (f64::from(*a), f64::from(*b));
        dot += a * b;
        l += a * a;
        r += b * b;
    }
    if l == 0.0 || r == 0.0 {
        0.0
    } else {
        dot / (l.sqrt() * r.sqrt())
    }
}

/// Embed `texts` not yet asked, together: the message and its cues before
/// recall goes through them one by one.
pub async fn warm(texts: &[String]) {
    let Some(meaning) = Meaning::current().await else {
        return;
    };
    let wanted: Vec<String> = texts
        .iter()
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty() && cached(&ASKED, &meaning.model, text).is_none())
        .collect();
    if wanted.is_empty() {
        return;
    }
    if let Ok(Ok(vectors)) = tokio::time::timeout(ASK_WITHIN, meaning.analyzer.embed(&wanted)).await
    {
        for (text, vector) in wanted.iter().zip(vectors) {
            keep(&ASKED, &meaning.model, text, Arc::new(vector));
        }
    }
}

/// Vectors already kept for `rows` under `model`, where the text is still
/// the one embedded.
async fn stored<C: ConnectionTrait>(
    db: &C,
    model: &str,
    rows: &[agent_memories::Model],
) -> Result<HashMap<String, Vector>, DbErr> {
    if rows.is_empty() {
        return Ok(HashMap::new());
    }
    let ids: Vec<String> = rows.iter().map(|row| row.id.clone()).collect();
    let found = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT memory_id, digest, vector FROM agent_memory_embeddings \
             WHERE model = $1 AND memory_id = ANY($2)",
            [model.into(), ids.into()],
        ))
        .await?;
    let digests: HashMap<&str, String> = rows
        .iter()
        .map(|row| (row.id.as_str(), digest(&row.content)))
        .collect();
    Ok(found
        .iter()
        .filter_map(|row| {
            let id: String = row.try_get("", "memory_id").ok()?;
            let kept: String = row.try_get("", "digest").ok()?;
            (digests.get(id.as_str()) == Some(&kept)).then_some(())?;
            let bytes: Vec<u8> = row.try_get("", "vector").ok()?;
            Some((id, Arc::new(from_bytes(&bytes)?)))
        })
        .collect())
}

async fn store<C: ConnectionTrait>(
    db: &C,
    model: &str,
    row: &agent_memories::Model,
    vector: &[f32],
) -> Result<(), DbErr> {
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO agent_memory_embeddings (memory_id, model, digest, vector, created_at) \
         VALUES ($1, $2, $3, $4, NOW()) \
         ON CONFLICT (memory_id) DO UPDATE SET model = EXCLUDED.model, \
         digest = EXCLUDED.digest, vector = EXCLUDED.vector, created_at = EXCLUDED.created_at",
        [
            row.id.clone().into(),
            model.into(),
            digest(&row.content).into(),
            to_bytes(vector).into(),
        ],
    ))
    .await
    .map(|_| ())
}

/// Requests that failed or did not answer in time, since start: recall went
/// by words those times.
static FAILED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

#[cfg(test)]
pub fn failed() -> u64 {
    FAILED.load(std::sync::atomic::Ordering::Relaxed)
}

/// How close each of `rows` is to `query` in meaning, by memory id; empty
/// when there is no model, it did not answer in time, or too few memories
/// have vectors to tell. Embeds up to `fill` memories still without one in
/// the same request.
pub async fn closeness<C: ConnectionTrait>(
    db: &C,
    rows: &[agent_memories::Model],
    query: &str,
    fill: usize,
) -> HashMap<String, f64> {
    closeness_within(db, rows, query, fill, ASK_WITHIN).await
}

async fn closeness_within<C: ConnectionTrait>(
    db: &C,
    rows: &[agent_memories::Model],
    query: &str,
    fill: usize,
    within: Duration,
) -> HashMap<String, f64> {
    let query = query.trim();
    if query.is_empty() || rows.is_empty() {
        return HashMap::new();
    }
    let Some(meaning) = Meaning::current().await else {
        return HashMap::new();
    };
    let model = meaning.model.as_str();
    // What is on hand first; the store only for the rest.
    let mut vectors: HashMap<String, Vector> = HashMap::new();
    let mut unknown: Vec<agent_memories::Model> = Vec::new();
    for row in rows {
        match cached(&KNOWN, model, &digest(&row.content)) {
            Some(vector) => {
                vectors.insert(row.id.clone(), vector);
            }
            None => unknown.push(row.clone()),
        }
    }
    let Ok(kept) = stored(db, model, &unknown).await else {
        return HashMap::new();
    };
    for row in &unknown {
        if let Some(vector) = kept.get(&row.id) {
            keep(&KNOWN, model, &digest(&row.content), vector.clone());
            vectors.insert(row.id.clone(), vector.clone());
        }
    }
    // Rows come newest first: the newest without a vector are embedded first.
    let missing: Vec<&agent_memories::Model> = unknown
        .iter()
        .filter(|row| !vectors.contains_key(&row.id))
        .take(fill)
        .collect();
    // What was asked, and the memories still without a vector, in two
    // requests side by side: a slow batch must not cost the question.
    let cached_ask = cached(&ASKED, model, query);
    let ask = async {
        match &cached_ask {
            Some(_) => None,
            None => Some(
                tokio::time::timeout(within, meaning.analyzer.embed(&[query.to_string()])).await,
            ),
        }
    };
    let texts: Vec<String> = missing
        .iter()
        .map(|row| row.content.trim().to_string())
        .collect();
    let batch = async {
        if texts.is_empty() {
            None
        } else {
            Some(tokio::time::timeout(within, meaning.analyzer.embed(&texts)).await)
        }
    };
    let (asking, batch) = futures::join!(ask, batch);
    let mut asked = cached_ask;
    if let Some(answered) = asking {
        match answered {
            Ok(Ok(mut embedded)) if !embedded.is_empty() => {
                let vector = Arc::new(embedded.swap_remove(0));
                keep(&ASKED, model, query, vector.clone());
                asked = Some(vector);
            }
            Ok(Ok(_)) | Ok(Err(_)) | Err(_) => {
                FAILED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                tracing::warn!("embedding what was asked failed or timed out; recalling by words");
            }
        }
    }
    match batch {
        Some(Ok(Ok(embedded))) => {
            for (row, vector) in missing.iter().zip(embedded) {
                let vector = Arc::new(vector);
                keep(&KNOWN, model, &digest(&row.content), vector.clone());
                if let Err(error) = store(db, model, row, &vector).await {
                    tracing::warn!(%error, "a memory's meaning could not be kept; it is worked out again next time");
                }
                vectors.insert(row.id.clone(), vector);
            }
        }
        Some(_) => tracing::debug!("memories not embedded this time; they are next time"),
        None => {}
    }
    let Some(asked) = asked else {
        return HashMap::new();
    };
    if vectors.len() < AT_LEAST {
        return HashMap::new();
    }
    vectors
        .into_iter()
        .map(|(id, vector)| (id, cosine(&asked, &vector)))
        .collect()
}

/// Embed every one of `rows` still without a vector, `FILL_PER_ASK` at a
/// time and without the reply's hurry (for a store filled at once, as a
/// bench does).
#[cfg(test)]
pub async fn fill<C: ConnectionTrait>(db: &C, rows: &[agent_memories::Model]) {
    const FILL_WITHIN: Duration = Duration::from_secs(60);
    let Some(meaning) = Meaning::current().await else {
        return;
    };
    for _ in 0..rows.len().div_ceil(FILL_PER_ASK) + 1 {
        let found = stored(db, &meaning.model, rows)
            .await
            .map_or(0, |found| found.len());
        if found >= rows.len() {
            return;
        }
        let _ = closeness_within(db, rows, "·", FILL_PER_ASK, FILL_WITHIN).await;
    }
}

/// How much each memory stands out by meaning from the rest, for the
/// closeness of all of them: `(closeness - mean) / deviation`, only those
/// at least `STANDS_OUT`.
pub fn standing_out(closeness: &HashMap<String, f64>) -> HashMap<String, f64> {
    if closeness.len() < AT_LEAST {
        return HashMap::new();
    }
    let count = closeness.len() as f64;
    let mean = closeness.values().sum::<f64>() / count;
    let deviation = (closeness
        .values()
        .map(|value| (value - mean).powi(2))
        .sum::<f64>()
        / count)
        .sqrt();
    if deviation <= f64::EPSILON {
        return HashMap::new();
    }
    closeness
        .iter()
        .map(|(id, value)| (id.clone(), (value - mean) / deviation))
        .filter(|(_, z)| *z >= STANDS_OUT)
        .collect()
}

/// `FILL_PER_ASK` for callers outside.
pub const FILL: usize = FILL_PER_ASK;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_vector_round_trips_and_closeness_is_the_angle() {
        let vector = vec![0.5f32, -1.25, 3.0];
        assert_eq!(from_bytes(&to_bytes(&vector)).unwrap(), vector);
        assert!(from_bytes(&[1, 2, 3]).is_none());
        assert!((cosine(&[1.0, 0.0], &[2.0, 0.0]) - 1.0).abs() < 1e-9);
        assert!(cosine(&[1.0, 0.0], &[0.0, 3.0]).abs() < 1e-9);
        assert_eq!(cosine(&[1.0], &[1.0, 2.0]), 0.0);
        assert_eq!(digest(" 猫 "), digest("猫"));
        assert_ne!(digest("猫"), digest("狗"));
    }

    #[test]
    fn only_what_stands_out_from_the_rest_counts() {
        let mut closeness: HashMap<String, f64> = (0..20)
            .map(|index| (format!("m{index}"), 0.30 + f64::from(index % 3) * 0.01))
            .collect();
        closeness.insert("close".into(), 0.80);
        let out = standing_out(&closeness);
        assert_eq!(out.keys().collect::<Vec<_>>(), vec!["close"]);
        // Too few to know what the rest are like.
        let few: HashMap<String, f64> = [("a".to_string(), 0.9), ("b".to_string(), 0.1)].into();
        assert!(standing_out(&few).is_empty());
    }
}
