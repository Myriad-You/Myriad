//! What she is shown of her nodes when she answers someone in a group,
//! while she reads them: that group's audience alone (`group:<id>`, kept
//! with the site owner), never a private talk's, another group's or her
//! own.
//!
//! - **Chosen for their words** ([`super::recall`]): the group's topics and
//!   its members' own nodes there, picked and listed by the core as in a
//!   private talk; the speaker's own node there in the place of 「关于你」.
//!   What is about anyone else is said as theirs, by the name the group
//!   knows them by.
//! - **Every time**: from the group's own node, the bits she and the group
//!   share and what the last days there were like (the freshest, and an
//!   older one their words bring back), how the group takes her (an entry,
//!   never a view: what people there said does not count as evidence, N4),
//!   and how she sums the group up when her upkeep has; from the speaker's
//!   own node there, what still stings that they did there.
//!
//! What a group turn has always read from elsewhere stays as it was: what
//! the same person said in her other groups, her other groups' own bits
//! and days, what she heard in groups about things, her own life, how much
//! what they did in private weighs (never what it was), and the group's
//! own threads. Every section is worded as the one it replaces.

use chrono::{DateTime, Utc};
use sea_orm::DatabaseConnection;

use super::{
    Error, audience,
    context::{NodeTurn, head_lines, open_sores, section},
    hanging,
    recall::{Here, Own},
    shown,
};
use crate::models::entities::agent_memories;
use crate::services::agent::memory::unified;
use crate::services::agent::merope::{bits, sore};

/// The group a turn is in.
#[derive(Debug, Clone)]
pub(crate) struct Group {
    /// The group's own node.
    pub node: String,
    pub here: Here,
}

/// This turn's read of the group `venue` (`telegram:-100123`), answering
/// `user_id`; none without a site owner to keep it.
pub(crate) async fn load(
    db: &DatabaseConnection,
    user_id: i32,
    venue: &str,
) -> Result<Option<NodeTurn>, Error> {
    let Some(owner) = hanging::site_owner(db).await else {
        return Ok(None);
    };
    let group = audience::group(venue);
    let about = myriad_nodes::node_id(&group, &audience::person_key(&group, user_id))?;
    let her = myriad_nodes::node_id(&group, "her")?;
    let node = myriad_nodes::node_id(&group, &format!("group:{venue}"))?;
    let shown = shown::shown(db, owner, &group, None).await?;
    let mut sores = shown
        .node(&about)
        .map(|node| open_sores(&node.head, &shown.memories))
        .unwrap_or_default();
    sores.sort_by_key(|sore| sore.since);
    let names = crate::services::channel_group::names_here(db, venue).await;
    Ok(Some(NodeTurn {
        own: Own {
            about: Some(about),
            her: Some(her),
        },
        shown,
        sores,
        intents: Vec::new(),
        group: Some(Group {
            node,
            here: Here {
                speaker: user_id,
                names,
            },
        }),
    }))
}

impl NodeTurn {
    /// The memory rows of `source` on the group's own node, its entries
    /// and spans, freshest first, as a venue's rows are read.
    fn group_rows(&self, source: &str) -> Vec<agent_memories::Model> {
        let Some(node) = self
            .group
            .as_ref()
            .and_then(|group| self.shown.node(&group.node))
        else {
            return Vec::new();
        };
        let mut rows: Vec<agent_memories::Model> = ["entries", "spans"]
            .iter()
            .flat_map(|field| node.head[field].as_array().into_iter().flatten())
            .flat_map(|item| shown::cited(&item["refs"]))
            .filter_map(|id| self.shown.memories.get(&id))
            .filter(|row| row.source == source)
            .cloned()
            .collect();
        rows.sort_by(|a, b| {
            b.updated_at
                .cmp(&a.updated_at)
                .then_with(|| a.id.cmp(&b.id))
        });
        rows.dedup_by(|a, b| a.id == b.id);
        rows.truncate(unified::VENUE_LOOKED_OVER as usize);
        rows
    }

    /// What she and this group share: the freshest `limit`, then any older
    /// one `words` bring back. In a private talk, theirs on 「关于你」.
    pub(crate) async fn bits_shown(
        &self,
        db: &DatabaseConnection,
        limit: usize,
        words: Option<&str>,
    ) -> Option<String> {
        if self.group.is_none() {
            return self.bits_section(limit);
        }
        let rows = unified::fresh_and_brought_among(
            db,
            self.group_rows(bits::SOURCE),
            limit,
            words,
            bits::BROUGHT_BACK,
        )
        .await;
        let shared: Vec<(String, String)> = rows.iter().filter_map(bits::bit_of).collect();
        myriad_merope::speaking::format_bits_section(&shared, true)
    }

    /// What the last days in this group were like: the last `limit`, and
    /// any older one `words` bring back, oldest first. None in private.
    pub(crate) async fn group_days(
        &self,
        db: &DatabaseConnection,
        limit: usize,
        words: Option<&str>,
    ) -> Vec<(String, String)> {
        if self.group.is_none() {
            return Vec::new();
        }
        let mut days: Vec<(String, String)> = unified::fresh_and_brought_among(
            db,
            self.group_rows(bits::DAY_SOURCE),
            limit,
            words,
            bits::BROUGHT_BACK,
        )
        .await
        .into_iter()
        .map(bits::day_of_row)
        .collect();
        days.sort_by(|left, right| left.0.cmp(&right.0));
        days
    }

    /// How this group takes her, as she last found it: an entry on its
    /// node.
    pub(crate) fn group_lands_section(&self, now: DateTime<Utc>) -> Option<String> {
        let row = self.group_rows("lands").into_iter().next()?;
        myriad_merope::speaking::format_lands_section(
            &row.content,
            row.created_at.with_timezone(&Utc),
            now,
            true,
        )
    }

    /// How she sums this group up, when her upkeep has: its node's head.
    pub(crate) fn group_head_section(&self) -> Option<String> {
        let node = self.shown.node(&self.group.as_ref()?.node)?;
        section(
            "What you make of this group",
            "How you sum this group up, as you last went over what you keep of it: yours to go by, never to recite.",
            "group_node",
            &head_lines(node, &self.shown),
        )
    }

    /// What still stings that they did in this group.
    pub(crate) fn sores_here(&self, now: DateTime<Utc>) -> Option<String> {
        sore::section(&self.sores, now)
    }
}
