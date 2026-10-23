// Synthetic sample content for the README animations. Every name, title, account and number is invented.
export const LOCALES = { zh: 'zh-CN', ja: 'ja-JP', en: 'en-US' }

export const OWNER = {
  zh: { name: '星野澄', bio: '「慢慢来，比较快。」' },
  ja: { name: '星野すみ', bio: '「ゆっくり行こう。」' },
  en: { name: 'Sumi Hoshino', bio: '“Slow is smooth, smooth is fast.”' },
}
export const SITE = { zh: '澄的小站', ja: 'すみの部屋', en: "Sumi's Room" }
export const PERSONA = 'Arael'
export const QUOTE = {
  zh: { text: '把每一个平凡的日子，过成值得记下的一页。', from: '书签' },
  ja: { text: 'ありふれた一日を、書き残したくなる一頁に。', from: 'しおり' },
  en: { text: 'Turn an ordinary day into a page worth keeping.', from: 'Bookmark' },
}

// Games keep English titles in every locale; other works are localized.
export const GAMES = [
  ['Orbital Garden', 410], ['Quiet Harbor', 236], ['Paper Lanterns', 128], ['Starfall Tactics', 97], ['Tidewatch', 74],
  ['Ember Road', 61], ['Glass Meridian', 48], ['Little Lighthouse', 33], ['Cumulus Rally', 21], ['Night Signals', 12],
]
export const ANIME = {
  zh: ['星落之港', '夜灯之街', '纸飞机与夏天', '潮汐观测部', '第七号灯塔', '雨停之后', '银河铁路便利店', '晚风邮差', '橘色的坡道', '月面图书馆'],
  ja: ['星降る港', '夜灯の街', '紙飛行機と夏', '潮汐観測部', '七番目の灯台', '雨上がりのあと', '銀河鉄道コンビニ', '晩風の郵便屋', 'オレンジの坂道', '月面図書館'],
  en: ['Harbor of Falling Stars', 'Lamplight Street', 'Paper Planes in Summer', 'The Tide Watch Club', 'Lighthouse No. 7', 'After the Rain', 'Galaxy Line Mart', 'The Evening Courier', 'Orange Hill Road', 'Library on the Moon'],
}
export const SONGS = [
  ['Night Signals', 'Lumen'], ['Tidal Echo', 'Mare'], ['Paper Moon', 'Kanata'], ['Afterglow Lines', 'Lumen'], ['Small Hours', 'Orbit Club'],
  ['Glass Garden', 'Mare'], ['Northbound', 'Kanata'], ['Satellite Waltz', 'Orbit Club'], ['Low Tide', 'Lumen'], ['Station Lights', 'Hikari Trio'],
]
export const VIDEOS = {
  zh: ['一个人的城市散步：黄昏篇', '用一周做出一盏纸灯', '独立游戏开发日志 #12', '海边小站的四季', '十分钟学会胶片冲洗', '夜车窗外'],
  ja: ['ひとりの街歩き：夕暮れ編', '一週間で紙灯りをつくる', 'インディー開発日誌 #12', '海辺の小駅の四季', '十分でわかるフィルム現像', '夜行列車の窓から'],
  en: ['A Solo City Walk at Dusk', 'Making a Paper Lamp in a Week', 'Indie Devlog #12', 'Four Seasons at a Seaside Station', 'Developing Film in Ten Minutes', 'From a Night Train Window'],
}
export const BOOKS = {
  zh: ['灯塔观测日志', '慢慢来的生活', '海与星的地图'],
  ja: ['灯台観測日誌', 'ゆっくり暮らす', '海と星の地図'],
  en: ['The Lighthouse Logbook', 'Living Slowly', 'A Map of Sea and Stars'],
}
export const FRIENDS = {
  zh: [['纸灯笼工作室', '做游戏，也做灯'], ['潮汐笔记', '写海边的事'], ['小行星书房', '读书与随笔']],
  ja: [['紙灯りスタジオ', 'ゲームと灯りづくり'], ['潮汐ノート', '海辺のこと'], ['小惑星書房', '読書と随筆']],
  en: [['Paper Lantern Studio', 'Games and lamps'], ['Tide Notes', 'Life by the sea'], ['Asteroid Reading Room', 'Books and essays']],
}

// Report scripts: a neutral narrator, short lines. `fact` ties a line to a number the card shows.
export const REPORTS = {
  steam: {
    zh: { headline: '慢节奏经营里，一坐就是一下午', chapters: [
      ['画像', [['games_count', '33 款游戏，1,100 多小时，大半花在两三款上。']]],
      ['口味', [[null, '《Orbital Garden》《Quiet Harbor》这类慢节奏经营占了大头。'], [null, '偶尔换换口味，来一局《Starfall Tactics》。']]],
      ['亮点', [['top_game_hours', '《Orbital Garden》单独 410 小时，是库里最长的一段。']]]] },
    ja: { headline: 'のんびり経営で、気づけば午後が終わる', chapters: [
      ['横顔', [['games_count', '33 本、1,100 時間あまり。その大半は二、三本に。']]],
      ['好み', [[null, '『Orbital Garden』『Quiet Harbor』のような、ゆっくりした経営ものが中心。'], [null, 'たまに気分を変えて『Starfall Tactics』を一局。']]],
      ['ハイライト', [['top_game_hours', '『Orbital Garden』だけで 410 時間。ライブラリでいちばん長い付き合い。']]]] },
    en: { headline: 'An afternoon gone, one slow build at a time', chapters: [
      ['Portrait', [['games_count', '33 games and 1,100+ hours, most of them spent on two or three.']]],
      ['Taste', [[null, 'Slow management games like Orbital Garden and Quiet Harbor take the lead.'], [null, 'Now and then a round of Starfall Tactics for a change of pace.']]],
      ['Highlight', [['top_game_hours', 'Orbital Garden alone: 410 hours, the longest stretch in the library.']]]] },
  },
  bangumi: {
    zh: { headline: '看完才打分，打分不手软', chapters: [
      ['肖像', [['done_count', '42 部作品看完了 36 部，清单很少积压。']]],
      ['口味', [[null, '偏爱安静的日常和海边的故事。'], ['top_subject', '《星落之港》给了满分，是这份清单的中心。']]],
      ['亮点', [['top_subject_rate', '七部作品拿到 10 分，从不为了平均而压分。']]]] },
    ja: { headline: '見終えてから採点、点は辛くない', chapters: [
      ['肖像', [['done_count', '42 作品のうち 36 作品を見終え、積みはほとんどない。']]],
      ['好み', [[null, '静かな日常と、海辺の物語が好き。'], ['top_subject', '『星降る港』は満点。このリストの中心にある。']]],
      ['ハイライト', [['top_subject_rate', '10 点が七作品。平均に合わせて点を下げたりしない。']]]] },
    en: { headline: 'Scores only after the finale, and never stingy', chapters: [
      ['Portrait', [['done_count', '36 of 42 titles finished; the backlog stays short.']]],
      ['Taste', [[null, 'Quiet slice-of-life and stories set by the sea.'], ['top_subject', 'Harbor of Falling Stars got a perfect score and sits at the center of the list.']]],
      ['Highlight', [['top_subject_rate', 'Seven titles rated 10, never marked down to look balanced.']]]] },
  },
  netease: {
    zh: { headline: '夜里的合成器，和不说话的旋律', chapters: [
      ['轮廓', [['playlist_count', '24 张歌单，按时间段排好：清晨、通勤、深夜。']]],
      ['口味', [[null, 'Lumen 和 Mare 轮流出现，器乐比人声多。'], ['recent_song', '最近循环的是《Night Signals》。']]],
      ['亮点', [['total_songs', '1,280 首收藏里，有一半没有歌词。']]]] },
    ja: { headline: '夜のシンセと、言葉のない旋律', chapters: [
      ['輪郭', [['playlist_count', 'プレイリストは 24 本。朝、通勤、深夜と時間帯で分けてある。']]],
      ['好み', [[null, 'Lumen と Mare が交互に。歌ものよりインストが多い。'], ['recent_song', '最近のリピートは「Night Signals」。']]],
      ['ハイライト', [['total_songs', 'お気に入り 1,280 曲のうち、半分は歌詞がない。']]]] },
    en: { headline: 'Late-night synths and melodies without words', chapters: [
      ['Outline', [['playlist_count', '24 playlists, sorted by time of day: morning, commute, late night.']]],
      ['Taste', [[null, 'Lumen and Mare take turns; instrumentals outnumber vocals.'], ['recent_song', 'On repeat lately: Night Signals.']]],
      ['Highlight', [['total_songs', 'Half of the 1,280 saved songs have no lyrics.']]]] },
  },
  github: {
    zh: { headline: '小而完整的项目，一个一个做完', chapters: [
      ['侧写', [['repos_count', '12 个仓库，大多是做完就收尾的小工具。']]],
      ['口味', [['top_language', 'TypeScript 为主，偶尔写点 Rust。']]],
      ['高光', [['top_repo_stars', '「lanterns」收到 320 颗星，是一个给静态站用的灯笼主题。']]]] },
    ja: { headline: '小さく完結したものを、ひとつずつ', chapters: [
      ['横顔', [['repos_count', 'リポジトリは 12。ほとんどが作り切った小さな道具。']]],
      ['好み', [['top_language', '主に TypeScript、ときどき Rust。']]],
      ['ハイライト', [['top_repo_stars', '「lanterns」に 320 スター。静的サイト向けの提灯テーマ。']]]] },
    en: { headline: 'Small, finished projects, one at a time', chapters: [
      ['Profile', [['repos_count', '12 repositories, mostly small tools that were carried to the finish.']]],
      ['Taste', [['top_language', 'Mostly TypeScript, with a little Rust.']]],
      ['Highlight', [['top_repo_stars', '“lanterns”, a lantern theme for static sites, has 320 stars.']]]] },
  },
}

// Extra library-only entries so the canvas is not sparse.
export const MORE_GAMES = ['Moss & Mortar', 'Hollow Bells', 'Kite Season', 'Copper Valley', 'Sunday Arcade', 'Lumen Drift', 'Pocket Archipelago', 'Rainy Platform', 'Velvet Engine', 'Winter Postcards']
export const MORE_ANIME = {
  zh: ['候鸟与风筝', '七月的观星台', '旧书店的猫', '放学后的模型部', '海雾邮局', '向日葵铁道'],
  ja: ['渡り鳥と凧', '七月の天文台', '古書店の猫', '放課後模型部', '海霧郵便局', 'ひまわり鉄道'],
  en: ['Kites and Migrating Birds', 'The July Observatory', 'The Bookshop Cat', 'After-School Model Club', 'Sea Fog Post Office', 'Sunflower Railway'],
}
export const MORE_SONGS = [['Blue Hour', 'Mare'], ['Lantern Walk', 'Kanata'], ['Quiet Engine', 'Orbit Club'], ['Harbor Lights', 'Hikari Trio'], ['Slow Orbit', 'Lumen'], ['Paper Boats', 'Kanata'], ['Midnight Ferry', 'Mare'], ['Soft Static', 'Orbit Club'], ['Window Seat', 'Hikari Trio'], ['First Snow', 'Lumen']]
export const MORE_VIDEOS = {
  zh: ['做一个会发光的书架', '在海边录下的雨声', '旧相机修复记', '一日三餐的便当'],
  ja: ['光る本棚をつくる', '海辺で録った雨音', '古いカメラの修理記', '三食のお弁当'],
  en: ['Building a Glowing Bookshelf', 'Rain Recorded by the Sea', 'Restoring an Old Camera', 'Three Bento Lunches'],
}
export const MORE_BOOKS = {
  zh: ['夜航船', '海边的小镇', '给未来的信'],
  ja: ['夜の航路', '海辺の小さな町', '未来への手紙'],
  en: ['Night Voyage', 'A Small Town by the Sea', 'Letters to the Future'],
}

export const CHAT = {
  zh: { ask: '晚上想放松一下，有推荐吗？', reply: '看到你今天又在《Orbital Garden》里待了两个小时。换换口味吧——《纸飞机与夏天》只有十二集，很适合今晚慢慢看。' },
  ja: { ask: '今夜はのんびりしたいな。何かおすすめある？', reply: '今日も『Orbital Garden』に二時間いたね。気分を変えて『紙飛行機と夏』はどう？全十二話だから、今夜ゆっくり見るのにちょうどいいよ。' },
  en: { ask: 'I want to unwind tonight. Any ideas?', reply: 'You spent another two hours in Orbital Garden today. How about a change of pace? Paper Planes in Summer is only twelve episodes, just right for a slow evening.' },
}

// What the workbench shows as her look; it describes the bundled character.
export const LOOK = {
  zh: { faceDesign: '圆脸，眉眼柔和', eyeDesign: '紫色大眼，睫毛细长', hairShape: '粉色及肩短发，发尾渐变成淡紫', hairLayerPlan: '刘海、两侧发束、后发分层', upperBodySilhouette: '宽松水手服外套', outfitConstruction: '白色水手领上衣，淡紫开衫', sleeveArmDesign: '长袖，袖口宽松', materialPlan: '棉质，开衫带细花纹', heroAccessory: '星形发夹、鸟笼耳坠', paletteHint: '粉、白、淡紫', motif: '星星' },
  ja: { faceDesign: '丸顔、やわらかな目元', eyeDesign: '紫の大きな瞳、細く長いまつげ', hairShape: '肩までのピンクの髪、毛先は薄紫に', hairLayerPlan: '前髪、サイドの房、後ろ髪を分けて', upperBodySilhouette: 'ゆったりしたセーラー風の上着', outfitConstruction: '白いセーラー襟のブラウスに薄紫のカーディガン', sleeveArmDesign: '長袖、袖口はゆったり', materialPlan: 'コットン、カーディガンに小さな花柄', heroAccessory: '星のヘアピン、鳥かごのイヤリング', paletteHint: 'ピンク・白・薄紫', motif: '星' },
  en: { faceDesign: 'Round face, soft eyes', eyeDesign: 'Large violet eyes, long fine lashes', hairShape: 'Shoulder-length pink hair fading to lavender', hairLayerPlan: 'Bangs, side locks and back hair as layers', upperBodySilhouette: 'Loose sailor-style jacket', outfitConstruction: 'White sailor-collar blouse under a lavender cardigan', sleeveArmDesign: 'Long, loose sleeves', materialPlan: 'Cotton; small floral pattern on the cardigan', heroAccessory: 'Star hairpins, birdcage earrings', paletteHint: 'Pink, white, lavender', motif: 'Stars' },
}

export const BLOG = {
  zh: {
    description: '慢慢写，慢慢看。',
    posts: [
      ['海边小站的四季', '春天的站台上有很多风筝，夏天的长椅很烫，秋天的列车晚点得刚好，冬天的候车室有一台总是坏掉的暖气。\n\n我把这四个季节都拍了下来，放在资料库里。', 21],
      ['给静态站做一个灯笼主题', '灯笼主题的想法来自夏天的庙会：每篇文章都是一盏灯，标题写在灯面上。\n\n做完之后最满意的是暗色模式——灯会自己亮起来。', 12],
      ['一周做出一盏纸灯', '周一画图纸，周三裁竹条，周五糊纸，周日终于亮了。\n\n比想象中难的是让纸面保持平整。', 5],
    ],
    draft: { title: '雨停之后的散步', body: '傍晚雨停了，沿着坡道一直走到海边。路灯一盏一盏亮起来的时候，想起今天还没听完的那张专辑。' },
  },
  ja: {
    description: 'ゆっくり書いて、ゆっくり読む。',
    posts: [
      ['海辺の小駅の四季', '春のホームには凧がたくさん、夏のベンチは熱くて、秋の列車はちょうどいいくらいに遅れ、冬の待合室にはいつも調子の悪いストーブがある。\n\n四つの季節をすべて写真に撮って、ライブラリにしまった。', 21],
      ['静的サイトに提灯のテーマを', '提灯テーマのアイデアは夏祭りから。記事の一つひとつが提灯で、タイトルは灯りの面に書いてある。\n\nいちばん気に入っているのはダークモード。灯りがひとりでにともる。', 12],
      ['一週間で紙灯りをつくる', '月曜に図面を描き、水曜に竹ひごを切り、金曜に紙を貼って、日曜にやっと灯った。\n\n思ったより難しかったのは、紙をぴんと張ったままにすること。', 5],
    ],
    draft: { title: '雨上がりの散歩', body: '夕方に雨がやんだので、坂道を海までずっと歩いた。街灯がひとつずつともるころ、今日聴きかけのアルバムを思い出した。' },
  },
  en: {
    description: 'Write slowly, read slowly.',
    posts: [
      ['Four Seasons at a Seaside Station', 'In spring the platform is full of kites; in summer the benches are hot; in autumn the trains run just late enough; in winter the waiting room has a heater that never quite works.\n\nI photographed all four seasons and put them in the library.', 21],
      ['A Lantern Theme for Static Sites', 'The idea came from a summer festival: every post is a lantern, its title written on the shade.\n\nMy favorite part is dark mode, where the lanterns light themselves.', 12],
      ['Making a Paper Lamp in a Week', 'Monday: drawings. Wednesday: bamboo strips. Friday: paper. Sunday: it finally lit up.\n\nThe hard part was keeping the paper taut.', 5],
    ],
    draft: { title: 'A Walk After the Rain', body: 'The rain stopped in the evening, so I followed the hill road all the way to the sea. As the streetlights came on one by one, I remembered the album I had left half-played.' },
  },
}
