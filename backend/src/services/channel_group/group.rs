//! What she keeps in mind of each group while the process runs: its recent lines and her state there.

use super::*;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub(super) struct Line {
    pub(super) at: chrono::DateTime<chrono::Utc>,
    pub(super) message_id: Option<String>,
    pub(super) name: String,
    /// Who said it, as the platform knows them: how she can mention them.
    #[serde(default)]
    pub(super) from: Option<String>,
    pub(super) text: String,
    pub(super) hers: bool,
    /// Pictures in it, and what she saw of each once she looked.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) images: Vec<GroupImage>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) seen: Vec<Option<myriad_merope::seeing::Seen>>,
}

impl Line {
    /// What it says, with its pictures as she saw them.
    pub(super) fn said(&self) -> String {
        let pictures: Vec<String> = self
            .images
            .iter()
            .enumerate()
            .map(|(index, image)| {
                myriad_merope::seeing::as_said(
                    self.seen.get(index).and_then(Option::as_ref),
                    image.hint.as_deref(),
                    image.sticker,
                )
            })
            .collect();
        [self.text.clone(), pictures.join("")]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
pub(super) struct StoredLines {
    pub(super) lines: Vec<Line>,
}

/// Whether a line is still one she keeps in mind of the group.
pub(super) fn within(line: &Line, window: Duration) -> bool {
    (chrono::Utc::now() - line.at)
        .to_std()
        .map_or(true, |age| age < window)
}

#[derive(Default)]
pub(super) struct Group {
    pub(super) lines: VecDeque<Line>,
    /// Whether the lines kept before a restart were brought back.
    pub(super) restored: bool,
    pub(super) busy: bool,
    pub(super) last_reply: Option<Instant>,
    pub(super) touched: Option<Instant>,
    /// The latest line that did not call her by name and she has not
    /// looked at yet, since when lines have gone unlooked at, and whether
    /// she is looking now.
    pub(super) pending: Option<GroupLine>,
    pub(super) unjudged_since: Option<Instant>,
    pub(super) judging: bool,
    /// When she was last called there, and when she means to glance at it.
    pub(super) called: Option<Instant>,
    pub(super) glance_at: Option<Instant>,
    /// Her speaking up unasked there lately, oldest first: when, and whether
    /// anyone took it up.
    pub(super) spoke_up: VecDeque<(Instant, bool)>,
    /// Whom she answered lately, in order, and whom she stopped answering
    /// (see `LOOP_ROUNDS`).
    pub(super) answered: VecDeque<(String, Instant)>,
    pub(super) paused: HashMap<String, Instant>,
    /// Lines that spoke to her while she was busy, oldest first.
    pub(super) waiting: VecDeque<GroupLine>,
    /// Lines someone added after one that called her, answered with it (see
    /// `reading`): not answered again on their own.
    pub(super) read_with: VecDeque<String>,
    /// Until when she is muted there, herself or with everyone (see
    /// `muting`).
    pub(super) muted_until: Option<chrono::DateTime<chrono::Utc>>,
    pub(super) everyone_muted_until: Option<chrono::DateTime<chrono::Utc>>,
    /// Replies today to people outside the community: the day, and how many.
    pub(super) stranger_replies: Option<(chrono::NaiveDate, u32)>,
    /// The last line she took in for what she heard (see `take_in`).
    pub(super) heard_upto: Option<chrono::DateTime<chrono::Utc>>,
    /// How often each picture was sent there lately (by its key): one sent
    /// again is the group's (see `bits::picture_again`).
    pub(super) pictures: HashMap<String, u32>,
    /// The latest line seen there and the token it came with: where she
    /// would say something first (see `share_first`).
    pub(super) reach: Option<(GroupLine, String)>,
    /// Messages and waits not yet written to the group's ledger.
    pub(super) ledger: Vec<myriad_merope::talk_shape::Typed>,
    pub(super) ledger_waits: Vec<f64>,
    /// Pieces of their messages and hers not yet written to it.
    pub(super) ledger_theirs: myriad_merope::contrast::Counts,
    pub(super) ledger_hers: myriad_merope::contrast::Counts,
}

pub(super) enum Turn {
    Began,
    Busy,
    Resting(Duration),
}

/// Count one more of today's, unless `limit` is reached.
pub(super) fn count_today(slot: &mut Option<(chrono::NaiveDate, u32)>, limit: u32) -> bool {
    let today = chrono::Local::now().date_naive();
    let count = match *slot {
        Some((day, count)) if day == today => count,
        _ => 0,
    };
    if count >= limit {
        return false;
    }
    *slot = Some((today, count + 1));
    true
}

pub(super) static GROUPS: LazyLock<Mutex<HashMap<String, Group>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// The Chat session each sender has in each group, so their turns in that
/// group supersede only each other.
pub(super) static SESSIONS: LazyLock<Mutex<HashMap<(String, i32), String>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub(super) fn with_group<T>(venue: &str, act: impl FnOnce(&mut Group) -> T) -> Option<T> {
    let mut groups = GROUPS.lock().ok()?;
    if !groups.contains_key(venue) && groups.len() >= MAX_GROUPS {
        if let Some(stalest) = groups
            .iter()
            .filter(|(_, group)| !group.busy)
            .min_by_key(|(_, group)| group.touched)
            .map(|(id, _)| id.clone())
        {
            groups.remove(&stalest);
        }
    }
    let group = groups.entry(venue.to_string()).or_default();
    group.touched = Some(Instant::now());
    Some(act(group))
}

pub(super) fn push_line(group: &mut Group, line: Line) {
    group.lines.retain(|line| within(line, TRANSCRIPT_FOR));
    group.lines.push_back(line);
    while group.lines.len() > TRANSCRIPT_LINES {
        group.lines.pop_front();
    }
}

/// Lines kept from before a restart, merged under the ones heard since.
pub(super) fn merge_restored(group: &mut Group, stored: Vec<Line>) {
    let mut lines: Vec<Line> = stored;
    for line in group.lines.drain(..) {
        let known = line.message_id.is_some()
            && lines.iter().any(|kept| kept.message_id == line.message_id);
        if !known {
            lines.push(line);
        }
    }
    lines.sort_by_key(|line| line.at);
    for line in lines {
        push_line(group, line);
    }
}
