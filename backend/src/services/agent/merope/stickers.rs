//! Her stickers (see `myriad_merope::stickers`): kept, offered to her by
//! what they mean, made when she decides to, and counted a month at a time.
//!
//! A sticker is a picture of her, so it is only hers while she still looks
//! like it: one drawn of how she looked before (another name, look or
//! portrait) is not offered again. A group's sticker is offered only there.

use chrono::{Datelike, TimeZone, Utc};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, Condition, DatabaseConnection, EntityTrait, PaginatorTrait,
    QueryFilter, QueryOrder, Set,
};

use crate::models::entities::merope_stickers::{self, Column, Entity, Model};
use crate::services::agent::memory::lexical;
use crate::services::image_generation::{self, ImageBackground, ImageReference};

pub use myriad_merope::stickers::{Choice, MONTHLY};

/// Stickers offered to her in a turn, at most.
const OFFERED: usize = 8;

/// The project logo's sticker: the look her stickers are drawn in.
const STYLE_REFERENCE_BYTES: &[u8] =
    include_bytes!("../../../../assets/merope/sticker-style-reference.webp");

pub fn style_reference() -> Result<ImageReference, image_generation::ImageGenerationError> {
    ImageReference::new(STYLE_REFERENCE_BYTES.to_vec(), "image/webp")
}

/// The stickers last offered in each conversation, so the number she picks
/// is the sticker she saw.
static OFFERED_NOW: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<String, Vec<String>>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

/// The conversation a turn's stickers are offered in.
pub fn key(request: &crate::services::agent::UserRequest) -> String {
    let session = request
        .context
        .as_ref()
        .and_then(|context| context.session_id.as_deref())
        .unwrap_or("");
    format!("{}:{session}", request.user_id)
}

/// Remember what was offered in conversation `key`.
pub fn offer(key: &str, stickers: &[Model]) {
    if let Ok(mut offered) = OFFERED_NOW.lock() {
        if offered.len() > 1024 {
            offered.clear();
        }
        offered.insert(
            key.to_string(),
            stickers.iter().map(|sticker| sticker.id.clone()).collect(),
        );
    }
}

/// The sticker numbered `number` (from 1) among those offered in `key`.
pub async fn offered_as(db: &DatabaseConnection, key: &str, number: u8) -> Option<Model> {
    let id = OFFERED_NOW
        .lock()
        .ok()?
        .get(key)?
        .get(usize::from(number).checked_sub(1)?)
        .cloned()?;
    Entity::find_by_id(id).one(db).await.ok().flatten()
}

/// What she chose in a turn, for the chat app: the sticker to send, by
/// id, or what to make. A number she was not offered is nothing.
pub async fn chosen(
    db: &DatabaseConnection,
    request: &crate::services::agent::UserRequest,
    choice: Choice,
) -> Option<serde_json::Value> {
    chosen_in(db, &key(request), choice).await
}

/// [`chosen`], for what was offered under `key`.
pub async fn chosen_in(
    db: &DatabaseConnection,
    key: &str,
    choice: Choice,
) -> Option<serde_json::Value> {
    match choice {
        Choice::Send(number) => {
            let sticker = offered_as(db, key, number).await?;
            Some(serde_json::json!({ "send": sticker.id }))
        }
        Choice::Make { picture, meaning } => Some(serde_json::json!({
            "make": { "picture": picture, "meaning": meaning }
        })),
    }
}

/// Her stickers section for a turn in a chat app, with what was offered
/// remembered under `key`: none if she has none and can make none.
pub async fn section(
    db: &DatabaseConnection,
    key: &str,
    venue: Option<&str>,
    talk: &str,
) -> Option<String> {
    current_identity(db).await?;
    let stickers = offered(db, venue, talk).await;
    offer(key, &stickers);
    let meanings: Vec<String> = stickers
        .iter()
        .map(|sticker| sticker.meaning.clone())
        .collect();
    myriad_merope::stickers::format_sticker_section(&meanings, left_this_month(db).await)
}

/// Draw a new sticker of her and keep it: what it shows, what it means, and
/// the group it belongs to (none: anywhere). Billed to `owner`, who hosts
/// her; none when this month's are used up or drawing fails.
pub async fn make_sticker(
    db: &DatabaseConnection,
    owner: i32,
    picture: &str,
    meaning: &str,
    venue: Option<&str>,
) -> Option<Model> {
    if left_this_month(db).await == 0 {
        tracing::info!("[Merope] no stickers left to make this month");
        return None;
    }
    let persona = super::get_persona(db).await.ok().flatten()?;
    let portrait = persona
        .portrait_asset_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())?
        .to_string();
    let visual_profile = persona
        .visual_profile
        .clone()
        .unwrap_or(serde_json::Value::Null);
    let identity = myriad_merope::stickers::identity_key(&persona.name, &visual_profile, &portrait);
    let anchor = image_generation::load_local_reference(&portrait)
        .await
        .ok()?;
    let style = style_reference().ok()?;
    let prompt = myriad_merope::stickers::sticker_prompt(&persona.name, &visual_profile, picture);
    let dynamic = crate::GLOBAL_DYNAMIC_CONFIG.read().await.clone();
    let config = image_generation::config_from_dynamic(&dynamic).ok()?;
    let size = myriad_merope::stickers::SIZE;
    let generated = match crate::services::ai_cost_ledger::with_site_ai_ledger(
        owner,
        "merope",
        "sticker",
        image_generation::generate_image_with_references(
            &config,
            &prompt,
            size,
            size,
            &[anchor, style],
            Some(ImageBackground::Transparent),
        ),
    )
    .await
    {
        Ok(generated) => generated,
        Err(error) => {
            tracing::warn!(%error, "[Merope] could not draw a sticker");
            return None;
        }
    };
    let id = uuid::Uuid::new_v4().simple().to_string();
    let actor = crate::services::media::MediaActor::admin(owner).ok()?;
    let persisted = image_generation::persist_generated_with_status(
        db,
        crate::services::media::MediaContext::site(
            actor,
            crate::services::media::MediaSource::Generated,
        )
        .with_producer_key(format!("merope-sticker:{id}")),
        generated,
        "sticker",
        crate::services::media::MediaExposure::Public,
    )
    .await
    .ok()?;
    let row = merope_stickers::ActiveModel {
        id: Set(id),
        picture: Set(picture.to_string()),
        meaning: Set(meaning.to_string()),
        venue: Set(venue.map(str::to_string)),
        asset_id: Set(persisted.url),
        identity: Set(identity),
        sent: Set(0),
        last_sent_at: Set(None),
        created_at: Set(Utc::now().fixed_offset()),
    };
    match row.insert(db).await {
        Ok(model) => {
            tracing::info!(
                venue = venue.unwrap_or("anywhere"),
                "[Merope] made a sticker"
            );
            Some(model)
        }
        Err(error) => {
            tracing::warn!(%error, "[Merope] could not keep a sticker she made");
            None
        }
    }
}

/// Groups looked at a night, at most, and their jokes each.
const JOKE_GROUPS: i64 = 5;
const JOKES: u64 = 10;

/// At night, for each group whose jokes came up lately: whether she makes a
/// sticker of one of them for that group, to send when it comes up again.
/// One a group a night at most; billed to `owner`.
pub async fn for_group_jokes(db: &DatabaseConnection, owner: i32) {
    if current_identity(db).await.is_none() {
        return;
    }
    let soul = crate::services::agent::identity::get_speaking_soul()
        .await
        .unwrap_or_default();
    for venue in super::bits::groups_lately(db, 3, JOKE_GROUPS).await {
        if left_this_month(db).await == 0 {
            return;
        }
        let jokes = super::bits::in_group(db, &venue, JOKES).await;
        if jokes.is_empty() {
            continue;
        }
        let here: Vec<String> = Entity::find()
            .filter(Column::Venue.eq(venue.as_str()))
            .all(db)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|sticker| sticker.meaning)
            .collect();
        let input = serde_json::json!({
            "jokes": jokes.iter().enumerate()
                .map(|(index, (handle, how))| serde_json::json!({"index": index, "joke": handle, "how": how}))
                .collect::<Vec<_>>(),
            "stickersHere": here,
            "leftThisMonth": left_this_month(db).await,
        })
        .to_string();
        let Ok(raw) = super::call::Ask::new(super::call::Voice::Hers, owner, "joke_sticker")
            .within(std::time::Duration::from_secs(45))
            .json_raw(
                &myriad_merope::stickers::joke_system(&soul),
                &input,
                myriad_merope::stickers::JOKE_SCHEMA,
                &myriad_merope::stickers::joke_schema(),
            )
            .await
        else {
            continue;
        };
        let Some(Some((shows, means))) = myriad_merope::stickers::parse_joke(&raw, jokes.len())
        else {
            continue;
        };
        if make_sticker(db, owner, &shows, &means, Some(&venue))
            .await
            .is_some()
        {
            tracing::info!(%venue, "[Merope] made a sticker of a group's joke");
        }
    }
}

/// One of her stickers, by id.
pub async fn by_id(db: &DatabaseConnection, id: &str) -> Option<Model> {
    Entity::find_by_id(id.to_string())
        .one(db)
        .await
        .ok()
        .flatten()
}

/// What she chose in a turn (see [`chosen`]), as a sticker: the one to
/// send, or one made now (billed to the site's owner, who hosts her; `venue`
/// is where a new one belongs, none for anywhere). Making one takes a while.
pub async fn resolve(
    db: &DatabaseConnection,
    chosen: &serde_json::Value,
    venue: Option<&str>,
) -> Option<Model> {
    if let Some(id) = chosen.get("send").and_then(serde_json::Value::as_str) {
        return by_id(db, id).await;
    }
    let make = chosen.get("make")?;
    let picture = make.get("picture")?.as_str()?;
    let meaning = make.get("meaning")?.as_str()?;
    let owner = crate::services::site_owner::site_owner_user_id(db)
        .await
        .ok()?;
    make_sticker(db, owner, picture, meaning, venue).await
}

/// The picture of a sticker, to send: bytes and their type.
pub async fn picture(sticker: &Model) -> Option<(Vec<u8>, String)> {
    image_generation::read_public_local_media(&sticker.asset_id).await
}

/// Who she is drawn as now; none without a portrait to draw her from.
pub async fn current_identity(db: &DatabaseConnection) -> Option<String> {
    let persona = super::get_persona(db).await.ok().flatten()?;
    let portrait = persona
        .portrait_asset_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())?;
    Some(myriad_merope::stickers::identity_key(
        &persona.name,
        persona
            .visual_profile
            .as_ref()
            .unwrap_or(&serde_json::Value::Null),
        portrait,
    ))
}

/// Her stickers that are still of her and belong here (`venue` is a group,
/// or none for anywhere else), those the talk touches first, then those
/// she sends most.
pub async fn offered(db: &DatabaseConnection, venue: Option<&str>, talk: &str) -> Vec<Model> {
    let Some(identity) = current_identity(db).await else {
        return Vec::new();
    };
    let belongs = match venue {
        Some(venue) => Condition::any()
            .add(Column::Venue.is_null())
            .add(Column::Venue.eq(venue)),
        None => Condition::all().add(Column::Venue.is_null()),
    };
    let rows = Entity::find()
        .filter(Column::Identity.eq(identity))
        .filter(belongs)
        .order_by_desc(Column::Sent)
        .order_by_desc(Column::CreatedAt)
        .all(db)
        .await
        .unwrap_or_default();
    let documents: Vec<lexical::Document> = rows
        .iter()
        .map(|row| lexical::Document {
            text: &row.meaning,
            concepts: &[],
        })
        .collect();
    let mut order: Vec<(usize, f64)> = lexical::score_all(talk, &documents)
        .into_iter()
        .enumerate()
        .map(|(index, score)| {
            let touched = if score.strong { score.value } else { 0.0 };
            // A group's own stickers come before those for anywhere: there
            // are few, and they are why she made them.
            let own = if rows[index].venue.is_some() {
                1.0
            } else {
                0.0
            };
            (index, touched + own)
        })
        .collect();
    // Stable: ties keep the most-sent-first order.
    order.sort_by(|a, b| b.1.total_cmp(&a.1));
    order
        .into_iter()
        .take(OFFERED)
        .map(|(index, _)| rows[index].clone())
        .collect()
}

/// Stickers she made this month (on the site's clock).
pub async fn made_this_month(db: &DatabaseConnection) -> usize {
    let now = chrono::Local::now();
    let Some(start) = chrono::Local
        .with_ymd_and_hms(now.year(), now.month(), 1, 0, 0, 0)
        .single()
    else {
        return MONTHLY;
    };
    Entity::find()
        .filter(Column::CreatedAt.gte(start.with_timezone(&Utc)))
        .count(db)
        .await
        .map_or(MONTHLY, |count| count as usize)
}

/// How many more she can make this month.
pub async fn left_this_month(db: &DatabaseConnection) -> usize {
    MONTHLY.saturating_sub(made_this_month(db).await)
}

/// She sent it.
pub async fn sent(db: &DatabaseConnection, sticker: &Model) {
    let row = merope_stickers::ActiveModel {
        id: Set(sticker.id.clone()),
        sent: Set(sticker.sent.saturating_add(1)),
        last_sent_at: Set(Some(Utc::now().fixed_offset())),
        ..Default::default()
    };
    if let Err(error) = row.update(db).await {
        tracing::warn!(%error, "[Merope] could not count a sticker she sent");
    }
}
