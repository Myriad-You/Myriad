//! Shared report-prompt blocks. Ban lists only — no golden samples.

/// Ground every claim in Data.
pub const EVIDENCE: &str = "\
只依据 Data。数字用原值，不换单位。*_summary 与各对象下的 summary 都是机器摘要，禁止复述或扩写。\
没有的就跳过该轴，禁止用空话补全。禁止编造账号、作品、视频、播放量、服务器、合作品牌。";

/// What the user actually reads. Cards already show the raw numbers.
pub const PROSE: &str = "\
summary：一句画像，≤50 字。禁用户名、禁报账数字。\
insights：3 条，各 ≤60 字，各占 Cover 里不同的一轴。薄数据宁可 2 条，禁止凑 5 条。\
点名轴 = 判断 + Look 里一个真名。结构轴（数量/占比/日历）用字段说话，不编名字。\
首页 hook 优先 vibe / taste_profile / mood_keywords；没有才用第一条 insight。禁复述 summary。\
禁「观看偏好：」「收藏概况：」冒号标签。禁三条挤在同一现象。\
会拿去蒸馏性格：写节奏/认真程度/社交距离，禁只列品类。";

/// Shared refuse list. No "good" counterparts — those get copied as templates.
pub const AVOID: &str = "\
禁：内容丰富、涉猎广泛、很有个性、技术实力强、社交达人、很活跃、品味独特、数据丰富、\
二次元浓度高、深夜听歌、待观察、内容创作者、很有想法、细腻的收藏家、开源贡献突出、\
很活跃的社区、硬核大佬、游戏库丰富、资深爱好者、有趣的灵魂。";

/// Home-card vibe. YouTube / X / Discord.
pub const VIBE: &str = "\
vibe：≤20 汉字，一行。禁换行、引号、客套。\
禁：待观察、内容创作者、很有想法、有趣的灵魂、持续输出。";

/// News / chain / support accounts are not taste. X graphs.
pub const MASS_ACCOUNTS: &str = "\
禁把新闻媒体、连锁品牌、便利店、官方客服、抽奖羊毛号当品味。\
禁圈层名写成游戏、科技、娱乐这种大类。";

/// Bangumi and MAL. taste_profile is the badge hook.
pub const CATALOG_VISUALS: &str = "\
taste_profile（≤20 字。禁：细腻的 ACG 收藏家、资深番剧爱好者、二次元浓度高）。";

/// Xbox / PSN: the source has no playtime.
pub const CONSOLE_NO_PLAYTIME: &str = "\
没有游玩时长。禁推断肝了多少小时。只从成就/奖杯进度和分数说话。";

/// Last instruction before Data.
pub const CHECK: &str = "\
写完核这七条，破一条就改：\
1. 三条（或更少）各占 Cover 不同轴\
2. 点名轴含 Look 真名；结构轴不编名字\
3. summary 无用户名、无纯数字报账\
4. 没有 AVOID 里的词，没有「xx概况：」\
5. 没写 Look 里不存在的作品或账号\
6. 若有 vibe / taste_profile / mood_keywords，短钩不在禁列\
7. 用户可见文案语种与 # Language 一致";

/// JSON envelope the parser in generate.rs expects.
pub const JSON_ENVELOPE: &str = "\
只返回一个合法 JSON 对象，不要 markdown，不要代码块：\n\
{\n\
  \"summary\": \"不超过50字的画像\",\n\
  \"insights\": [\"覆盖不同轴的洞察\"],\n\
  \"card_visuals\": {}\n\
}\n\
card_visuals 按 # card_visuals：要求空对象就交 {}，否则按所列字段写。";
