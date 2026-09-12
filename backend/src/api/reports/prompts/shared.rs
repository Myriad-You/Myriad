//! Shared report-prompt blocks. Ban lists only — no golden samples.

/// Ground every claim in Data.
pub const EVIDENCE: &str = "\
Use Data only. Keep numbers as-is; do not convert units. *_summary fields are machine digests: do not restate them. \
Skip an empty axis. Do not invent accounts, works, videos, counts, servers, or brand deals.";

/// What the user actually reads. Cards already show the raw numbers.
pub const PROSE: &str = "\
summary: one portrait, ≤50 chars. No username, no number dump. \
insights: 3×≤60 chars on different Cover axes; thin data may use 2, never pad to 5. \
Name axis = judgment + one Look name. Structure axes use fields only. \
Home hook: vibe / taste_profile / mood_keywords, else first insight. Do not restate summary. \
No 'xx overview:' labels. Do not stack three insights on one fact. \
Write pace / seriousness / social distance; do not only list genres.";

/// Shared refuse list. No "good" counterparts — those get copied as templates.
pub const AVOID: &str = "\
Refuse: content-rich, wide-ranging, very unique, social butterfly, TBD, content creator, interesting soul, seasoned fan. \
Also refuse: 内容丰富、涉猎广泛、很有个性、技术实力强、社交达人、很活跃、品味独特、数据丰富、\
二次元浓度高、深夜听歌、待观察、内容创作者、很有想法、细腻的收藏家、开源贡献突出、\
很活跃的社区、硬核大佬、游戏库丰富、资深爱好者、有趣的灵魂。";

/// Home-card vibe. YouTube / X / Discord.
pub const VIBE: &str = "\
vibe: ≤20 chars, one line. No newlines, quotes, or courtesy. \
Refuse: TBD, content creator, interesting soul, always shipping, 待观察、内容创作者、很有想法、有趣的灵魂、持续输出。";

/// X graphs: keep only identifiable taste; drop leftovers instead of a catch-all.
pub const MASS_ACCOUNTS: &str = "\
Keep only accounts whose name/description shows a real interest. If unreadable, drop them; do not dump leftovers into a catch-all circle. \
Circles must cover different facets; do not pile most accounts into one. \
Do not name circles as generic bins like games, tech, entertainment, news, or 游戏、科技、娱乐、新闻.";

/// Bangumi and MAL. taste_profile is the badge hook.
pub const CATALOG_VISUALS: &str = "\
taste_profile (≤20 chars. Refuse: delicate ACG collector, seasoned anime fan, high anime density, 细腻的 ACG 收藏家、资深番剧爱好者、二次元浓度高).";

/// Xbox / PSN: the source has no playtime.
pub const CONSOLE_NO_PLAYTIME: &str = "\
There is no playtime. Do not infer hours played. Speak only from achievement/trophy progress and scores.";

/// Last instruction before Data.
pub const CHECK: &str = "\
Before you finish, check these seven; fix any miss: \
1. Three (or fewer) insights sit on different Cover axes \
2. Name axes include a real Look name; structure axes invent no names \
3. summary has no username and no raw number dump \
4. No AVOID phrases, no 'xx overview:' labels \
5. No works or accounts that are not in Look \
6. If vibe / taste_profile / mood_keywords exist, the short hook is not on the refuse list \
7. User-visible copy language matches # Language";

/// JSON envelope the parser in generate.rs expects.
pub const JSON_ENVELOPE: &str = "\
Return one valid JSON object only. No markdown, no code fence:\n\
{\n\
  \"summary\": \"portrait, ≤50 chars\",\n\
  \"insights\": [\"insights on different axes\"],\n\
  \"card_visuals\": {}\n\
}\n\
Follow # card_visuals: if it asks for an empty object, return {}. Otherwise write the listed fields.";
