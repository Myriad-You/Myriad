// The synthetic sample data for one language: stand-ins for everything that varies by
// language or would need an outside service. The local instance answers the rest.
import { Buffer } from 'node:buffer'
import fs from 'node:fs'
import path from 'node:path'
import process from 'node:process'
import sharp from 'sharp'

import { art, avatar } from './art.mjs'
import * as C from './content.mjs'
import { mocks, overrides } from './render.mjs'

const enc = encodeURIComponent
export const cover = (seed, title = '', w = 400, h = 560, sub = '') => `/api/demo-art/c/${enc(seed)}.png?w=${w}&h=${h}&t=${enc(title)}&s=${enc(sub)}`
export const face = (seed) => `/api/demo-art/a/${enc(seed)}.png`
const artCache = new Map()

const day = (offsetDays, hh = 12, mm = 0) => new Date(Date.UTC(2026, 9, 23 - offsetDays, hh - 9, mm)).toISOString()

export function layout() {
  const w = (id, type, x, y, size, config) => ({ id, type, size, position: { x, y }, ...(config ? { config } : {}) })
  const standard = [
    w('w-welcome', 'welcome', 0, 0, '2x2'),
    w('w-s-github', 'social-network', 2, 0, '1x1', { platformId: 'github' }),
    w('w-s-steam', 'social-network', 3, 0, '1x1', { platformId: 'steam' }),
    w('w-s-bangumi', 'social-network', 2, 1, '1x1', { platformId: 'bangumi' }),
    w('w-s-mail', 'social-network', 3, 1, '1x1', { platformId: 'custom_mail' }),
    w('w-weather', 'weather', 0, 2, '2x2'),
    w('w-r-steam', 'report-steam', 4, 0, '4x2', { clickAction: 'report', platformId: 'steam' }),
    w('w-r-bangumi', 'report-bangumi', 8, 0, '4x2', { clickAction: 'report', platformId: 'bangumi' }),
    w('w-persona', 'agent-persona', 12, 0, '2x2'),
    w('w-friends', 'friend-links', 14, 0, '2x2'),
    w('w-r-netease', 'report-netease', 2, 2, '4x2', { clickAction: 'report', platformId: 'netease' }),
    w('w-r-github', 'report-github', 6, 2, '4x2', { clickAction: 'report', platformId: 'github' }),
    w('w-music', 'music-player', 10, 2, '2x2'),
    w('w-activity', 'recent-activity', 12, 2, '4x2'),
  ]
  return { v: 2, standard, free: [] }
}

function steamReport(L) {
  const games = C.GAMES.map(([t, h], i) => ({ title: t, hours: h, cover: cover(`game-${i}`, t, 460, 215) }))
  return {
    platform: 'steam', locale: C.LOCALES[L],
    card_visuals: { games_count: 33, hardcore_score: 42, total_playtime: 1120, library_items: games.map(g => ({ cover: g.cover, title: g.title, type: 'game' })) },
    facts: [{ id: 'games_count', value: 33 }, { id: 'total_playtime_hours', value: 1120 }, { id: 'top_game', value: games[0].title, media: games[0].cover }, { id: 'top_game_hours', value: 410 }],
  }
}
function bangumiReport(L) {
  const titles = C.ANIME[L]
  const rates = [10, 10, 9, 10, 9, 10, 8, 10, 9, 10]
  const items = titles.map((t, i) => ({ cover: cover(`anime-${i}`, t), platform: 'bangumi', rate: rates[i], title: t, type: 'anime', url: '' }))
  return {
    platform: 'bangumi', locale: C.LOCALES[L],
    card_visuals: {
      collection_type_distribution: { done: 36, doing: 4, wish: 2 }, favorite_tags: {}, library_items: items,
      status_counts: { doing: 4, done: 36, dropped: 0, on_hold: 0, wish: 2 },
      subject_type_distribution: { anime: 30, book: 6, game: 6 },
      taste_profile: { zh: '安静日常爱好者', ja: '静かな日常好き', en: 'Quiet slice-of-life fan' }[L],
      top_subjects: items.slice(0, 6).map((it, i) => ({ collection_type: 'done', cover: it.cover, rate: it.rate, subject_id: 9000 + i, subject_type: 'anime', title: it.title, updated_at: day(i * 9) })),
    },
    facts: [{ id: 'done_count', value: 36 }, { id: 'doing_count', value: 4 }, { id: 'wish_count', value: 2 }, { id: 'dropped_count', value: 0 }, { id: 'top_subject', value: titles[0], media: items[0].cover }, { id: 'top_subject_rate', value: 10 }],
  }
}
function neteaseReport(L) {
  const songs = C.SONGS.map(([t, a], i) => ({ artist: a, cover: cover(`song-${i}`, '', 300, 300), fee: 0, isVip: false, title: t, type: 'music' }))
  const tags = { zh: ['安静', '夜晚', '合成器', '器乐'], ja: ['静けさ', '夜', 'シンセ', 'インスト'], en: ['Calm', 'Night', 'Synth', 'Instrumental'] }[L]
  return {
    platform: 'netease', locale: C.LOCALES[L],
    card_visuals: { follower_count: 128, level: 7, library_items: songs, mood_keywords: tags.map((tag, i) => ({ tag, color: ['#7C8CD6', '#4B5A9E', '#B48EE0', '#7FB7C9'][i] })), playlist_count: 24, soul_color: '#2A3366' },
    facts: [{ id: 'follower_count', value: 128 }, { id: 'playlist_count', value: 24 }, { id: 'total_songs', value: 1280 }, { id: 'artist_count', value: 86 }, { id: 'top_region', value: { zh: '日本', ja: '日本', en: 'Japan' }[L] }, { id: 'recent_song', value: 'Night Signals — Lumen', media: songs[0].cover }],
  }
}
function githubReport(L) {
  const cal = []; let seed = 7
  // Daily counts, listed one weekday row at a time like the real report.
  for (let w = 0; w < 7; w++) { for (let k = 0; k < 53; k++) {
    const d = new Date(Date.UTC(2025, 9, 19 + k * 7 + w)); if (d > new Date(Date.UTC(2026, 9, 22))) continue
    seed = (seed * 9301 + 49297) % 233280; const r = seed / 233280
    const busy = 0.35 + 0.65 * Math.sin(k / 7 + 0.6) ** 2; const weekend = w === 0 || w === 6 ? 0.5 : 1
    cal.push({ date: d.toISOString().slice(0, 10), count: r < 0.28 ? 0 : Math.round(r * 14 * busy * weekend) })
  }
}
  const total = cal.reduce((a, c) => a + c.count, 0); const active = cal.filter(c => c.count > 0).length
  const desc = { zh: ['给静态站用的灯笼主题', '把 RSS 收进一页的阅读器', '小巧的番茄钟', '把照片 EXIF 变成卡片'], ja: ['静的サイト向けの提灯テーマ', 'RSS を一枚にまとめるリーダー', '小さなポモドーロタイマー', '写真の EXIF をカードに'], en: ['A lantern theme for static sites', 'An RSS reader on a single page', 'A tiny pomodoro timer', 'Turns photo EXIF into cards'] }[L]
  const repos = [['lanterns', 'TypeScript', 320, 41], ['one-page-feeds', 'TypeScript', 96, 12], ['tomato', 'Rust', 58, 6], ['exif-card', 'TypeScript', 46, 5]].map(([title, language, stars, forks], i) => ({ title, language, stars, forks, description: desc[i], type: 'repo' }))
  return {
    platform: 'github', locale: C.LOCALES[L],
    card_visuals: { contribution_calendar: cal, contribution_level: 'active', languages: [{ name: 'TypeScript', percentage: 68 }, { name: 'Rust', percentage: 22 }, { name: 'CSS', percentage: 10 }], library_items: repos, repos_count: 12, total_contributions: total, total_stars: 520 },
    facts: [{ id: 'total_contributions', value: total }, { id: 'active_days', value: active }, { id: 'repos_count', value: 12 }, { id: 'total_stars', value: 520 }, { id: 'contribution_level', value: 'active' }, { id: 'top_language', value: 'TypeScript' }, { id: 'top_language_share', value: 68 }, { id: 'top_repo', value: 'lanterns' }, { id: 'top_repo_stars', value: 320 }],
  }
}
const KEYS = ['portrait', 'taste', 'highlight']
export function reports(L) {
  const out = [steamReport(L), bangumiReport(L), neteaseReport(L), githubReport(L)].map((r, i) => {
    const s = C.REPORTS[r.platform][L]
    return {
      ...r, created_at: day(1, 9, 10 + i),
      script: { headline: s.headline, chapters: s.chapters.map(([title, beats], k) => ({ key: KEYS[k], title, beats: beats.map(([fact, text]) => (fact ? { fact, text } : { text })) })) },
      summary: s.headline,
    }
  })
  return { success: true, user_id: 1, created_at: day(1, 9, 10), platform_reports: out }
}

function activities(L) {
  const A = C.ANIME[L]
  const list = [
    { platform_name: 'steam', title: 'Steam', change_date: day(0, 9, 40), changes: [{ kind: 'metric_changed', metric: 'playtime_minutes', old: 24480, new: 24600, delta: 120, subject_title: 'Orbital Garden', subject_image: cover('game-0', 'Orbital Garden', 460, 215) }] },
    { platform_name: 'bangumi', title: 'Bangumi', change_date: day(0, 1, 15), changes: [{ kind: 'item_added', subject_title: A[2], subject_image: cover('anime-2', A[2]) }] },
    { platform_name: 'github', title: 'GitHub', change_date: day(1, 22, 5), changes: [{ kind: 'metric_changed', metric: 'stars', old: 316, new: 320, delta: 4, subject_title: 'lanterns' }] },
    { platform_name: 'netease', title: C.LOCALES[L] === 'en-US' ? 'NetEase Music' : '网易云音乐', change_date: day(1, 20, 30), changes: [{ kind: 'item_added', subject_title: 'Night Signals', subject_image: cover('song-0', '', 300, 300) }] },
    { platform_name: 'steam', title: 'Steam', change_date: day(2, 18, 0), changes: [{ kind: 'item_added', subject_title: 'Little Lighthouse', subject_image: cover('game-7', 'Little Lighthouse', 460, 215) }] },
    { platform_name: 'bangumi', title: 'Bangumi', change_date: day(3, 23, 10), changes: [{ kind: 'item_added', subject_title: A[5], subject_image: cover('anime-5', A[5]) }] },
  ].map(a => ({ ...a, event_type: 'updated', change_count: a.changes.length }))
  return { success: true, count: list.length, activities: list }
}

function friends(L) {
  return { success: true, sources: C.FRIENDS[L].map(([name, description], i) => ({
    id: 900 + i, name, description, icon: face(`friend-${i}`), site_url: `https://friend${i + 1}.example.org/`, url: `https://friend${i + 1}.example.org/feed.xml`,
    category: 'friend_links', enabled: true, admin_only: false, error_count: 0, feed_type: 'rss', source_type: 'rss', item_count: 12, unread_count: 0, pulses: [], recent_items: [],
    theme_color: ['#c27aa0', '#5a8fc2', '#7a9a5a'][i], created_at: 1784211450612, last_fetched_at: 1792714349672, last_success_at: 1792714349672, update_interval: 30,
    ai_style_tags: null, card_size: null, has_extra_config: false, last_error: null, rsshub_route: null, sort_order: i,
  })) }
}

export function libraryItems(L) {
  const items = []
  const add = (platform, item_type, id, title, c, metadata = {}) => items.push({ id, platform, item_type, title, cover: c, metadata })
  for (const [i, t] of [...C.GAMES.map((g) => g[0]), ...C.MORE_GAMES].entries()) add('Steam', 'game', `steam_game_${9100 + i}`, t, cover(`game-${i}`, t, 460, 215), { playtime_forever: (C.GAMES[i]?.[1] ?? 6) * 60 })
  for (const [i, t] of [...C.ANIME[L], ...C.MORE_ANIME[L]].entries()) add('Bangumi', 'anime', `bangumi_anime_${9000 + i}`, t, cover(`anime-${i}`, t))
  for (const [i, [t, a]] of [...C.SONGS, ...C.MORE_SONGS].entries()) add('Netease', 'music', `netease_song_${100 + i}`, t, cover(`song-${i}`, '', 300, 300), { ar: [{ name: a }], al: { name: t } })
  for (const [i, t] of [...C.VIDEOS[L], ...C.MORE_VIDEOS[L]].entries()) add('Bilibili', 'video', `bilibili_video_${i}`, t, cover(`video-${i}`, t, 480, 270))
  for (const [i, t] of [...C.BOOKS[L], ...C.MORE_BOOKS[L]].entries()) add('Bangumi', 'book', `bangumi_book_${9500 + i}`, t, cover(`book-${i}`, t))
  // Interleave by type so every part of the canvas mixes kinds.
  const byType = {}; for (const it of items) (byType[it.item_type] ||= []).push(it)
  const out = []; const lists = Object.values(byType)
  for (let k = 0; out.length < items.length; k++) { for (const l of lists) { if (l[k]) out.push(l[k])
}
}
  return out
}
function library(L, u) {
  const all = libraryItems(L)
  const type = u.searchParams.get('type')
  const pool = type ? all.filter(i => i.item_type === type) : all
  const count = (t, src) => ({ count: all.filter(i => i.item_type === t && i.platform === src).length, source: src })
  const available_sources = { game: [count('game', 'Steam')], anime: [count('anime', 'Bangumi')], music: [count('music', 'Netease')], video: [count('video', 'Bilibili')], book: [count('book', 'Bangumi')] }
  if (u.searchParams.get('counts_only') === 'true') return { success: true, available_sources, total: all.length, raw_total: all.length }
  const offset = +(u.searchParams.get('offset') || 0); const limit = +(u.searchParams.get('limit') || 120)
  const page = pool.slice(offset, offset + limit)
  return { success: true, empty: false, items: page, available_sources, has_more: offset + limit < pool.length, next_offset: offset + limit < pool.length ? offset + limit : null, offset, limit, returned: page.length, total: pool.length, raw_total: all.length, type, message: '',
    preferences: { layout: 'canvas', categories: { anime: ['Bangumi'], book: ['Bangumi'], game: ['Steam'], music: ['Netease'], video: ['Bilibili'] } } }
}

function weather() {
  const days = [0, 1, 2, 3].map(i => `2026-10-${23 + i}`)
  return {
    forecast: { current: { temperature_2m: 19.4, weather_code: 1, relative_humidity_2m: 58, apparent_temperature: 19, wind_speed_10m: 9 }, daily: { time: days, weather_code: [1, 2, 61, 0], temperature_2m_max: [22, 21, 18, 23], temperature_2m_min: [14, 15, 13, 14] } },
    aqi: { current: { us_aqi: 32 } },
  }
}

function silentWav(seconds = 4, rate = 8000) {
  const n = seconds * rate; const b = Buffer.alloc(44 + n)
  b.write('RIFF', 0); b.writeUInt32LE(36 + n, 4); b.write('WAVEfmt ', 8); b.writeUInt32LE(16, 16); b.writeUInt16LE(1, 20); b.writeUInt16LE(1, 22)
  b.writeUInt32LE(rate, 24); b.writeUInt32LE(rate, 28); b.writeUInt16LE(1, 32); b.writeUInt16LE(8, 34); b.write('data', 36); b.writeUInt32LE(n, 40); b.fill(128, 44)
  return b
}

export function install(L, { persona = true } = {}) {
  const loc = C.LOCALES[L]
  const owner = C.OWNER[L]
  mocks.push({ url: /^\/api\/demo-art\//, body: async (req, u) => {
    const key = u.pathname + u.search
    if (!artCache.has(key)) {
      const [, , , kind, file] = u.pathname.split('/')
      const seed = decodeURIComponent(file.replace(/\.png$/, ''))
      const q = u.searchParams
      artCache.set(key, kind === 'a' ? await avatar({ seed, size: 192 }) : await art({ seed, title: seed.startsWith('game-') ? (q.get('t') || '') : '', w: +q.get('w') || 400, h: +q.get('h') || 560 }))
    }
    return artCache.get(key)
  }, ct: 'image/png' })
  mocks.push({ url: '/api/reports/latest', body: reports(L) })
  mocks.push({ url: /^\/api\/library(\?|$)/, body: (req, u) => library(L, u) })
  mocks.push({ url: /^\/api\/agent\/notifications\/stream/, status: 204, body: '' })
  mocks.push({ url: /^\/api\/activities/, body: activities(L) })
  mocks.push({ url: /^\/api\/phantasi\/sources/, body: friends(L) })
  mocks.push({ url: '/api/profile/user-info', body: { success: true, source: 'platform', user_info: { name: owner.name, bio: owner.bio, avatar: face('owner'), platform: 'GitHub' } } })
  mocks.push({ url: '/api/proxy/hitokoto', body: { id: 1, hitokoto: C.QUOTE[L].text, from: C.QUOTE[L].from, from_who: null, type: 'k' } })
  mocks.push({ url: '/api/proxy/client-geo', body: { status: 'success', city: { zh: '横滨', ja: '横浜', en: 'Yokohama' }[L], country: 'Japan', countryCode: 'JP', lat: 35.44, lon: 139.64, source: 'demo' } })
  mocks.push({ url: /^\/api\/proxy\/music\/netease\/playlist\//, body: { code: 200, source: 'netease', playlistId: '1', songs: C.SONGS.map(([name, artist], i) => ({ id: String(100 + i), name, artist, album: name, cover: cover(`song-${i}`, '', 300, 300), duration: 180 + i * 17, isVip: false })) } })
  mocks.push({ url: '/api/steam/presence', body: { success: true, data: { personastate: 1, personastate_label: 'Online', is_online: true, is_in_game: false, gameextrainfo: null, gameid: null, avatar: face('owner'), personaname: 'sumi', recent_2weeks_minutes: 1110 } } })
  mocks.push({ url: /^\/api\/proxy\/music\/(netease|kugou)\/lyrics/, status: 404, body: { success: false } })
  mocks.push({ url: /^\/api\/proxy\/music\/netease\/audio\//, body: silentWav(), ct: 'audio/wav' })
  const W = weather()
  mocks.push({ url: /^https:\/\/api\.open-meteo\.com\//, body: W.forecast })
  mocks.push({ url: /^https:\/\/air-quality-api\.open-meteo\.com\//, body: W.aqi })

  overrides.push({ test: /^\/api\/config\/ui/, fn: (d) => {
    const c = d.config || d
    c.dashboard_layout = JSON.stringify(layout())
    c.dashboard_layout_mode = 'standard'
    c.dashboard_title = 'Myriad'
    c.custom_platforms = JSON.stringify([{ id: 'custom_mail', name: { zh: '邮箱', ja: 'メール', en: 'Email' }[L], username: '', iconType: 'react-icons', iconLibrary: 'si', iconName: 'SiMaildotru', iconUrl: null, color: '#0078D4', darkColor: '#4aa3ff', linkType: 'popup', popupData: { type: 'text', text: 'hello@example.org' } }])
    c.music_playlist_id = '1'; c.music_enabled = 'true'; c.music_source = 'netease'
    return d
  } })
  overrides.push({ test: /^\/api\/auth\/me/, fn: (d) => { const u = d.user || d; if (u.username) Object.assign(u, { username: 'sumi', display_name: owner.name, locale: loc, avatar_url: face('owner') }); return d } })
  overrides.push({ test: /^\/api\/config\/public/, fn: (d) => {
    const vals = { GitHub: 'sumi-hoshino-demo', Steam: '0', Bangumi: 'sumi-demo', 'Netease Music': '1' }
    for (const p of d.platforms || []) { if (vals[p.name]) { p.enabled = true; p.config_fields[0].value = vals[p.name] }
}
    d.agentPersonaName = C.PERSONA
    if (persona) { d.meropeEnabled = true; d.aiAvailability = { chat: true, image: true, persona: true, personaName: true, pro: true, standard: true } }
    return d
  } })
}

// Her face: the bundled character's rig package, kept outside the repository.
// README_MEDIA_RIG is a folder with active.json (as /api/merope/rig/active answers,
// texture URL /api/merope/rig/assets/demo-atlas) and atlas.png.
export function installRig(L) {
  const RIG_DIR = process.env.README_MEDIA_RIG
  if (!RIG_DIR) throw new Error('README_MEDIA_RIG is not set')
  mocks.push({ url: '/api/merope/rig/active', body: JSON.parse(fs.readFileSync(path.join(RIG_DIR, 'active.json'), 'utf8')) })
  mocks.push({ url: '/api/merope/rig/assets/demo-atlas', body: fs.readFileSync(path.join(RIG_DIR, 'atlas.png')), ct: 'image/png' })
  mocks.push({ url: '/api/merope/rig/see-through/status', body: {} })
  mocks.push({ url: '/api/merope/rig/parts', body: { parts: [] } })
  mocks.push({ url: '/api/merope/rig/expressions', body: { expressions: {}, drawing: [] } })
  mocks.push({ url: /^\/api\/merope\/rig\/see-through\/turn-keyforms/, body: { job: null } })
  mocks.push({ url: '/api/agent/persona/visual-from-portrait', method: 'POST', body: { visualIdentity: {}, clothingStyle: '' } })
  mocks.push({ url: '/api/agent/persona', method: 'PUT', body: async (req) => ({ name: C.PERSONA, portraitAssetId: null, avatarAssetId: null, hasCustomPersona: true, ...JSON.parse(req.postData() || '{}') }) })
  // The live rig covers the master portrait; it only needs to exist.
  mocks.push({ url: '/api/merope/rig/assets/demo-portrait', body: async () => sharp({ create: { width: 64, height: 64, channels: 4, background: { r: 0, g: 0, b: 0, alpha: 0 } } }).png().toBuffer(), ct: 'image/png' })
  mocks.push({ url: '/api/agent/doing', body: { doing: null, now: '2026-10-23T01:20:00Z' } })
  mocks.push({ url: '/api/agent/intentions', body: { intentions: [] } })
  mocks.push({ url: '/api/speech/status', body: { available: false, tts_enabled: false, asr_enabled: false, persona_speech_enabled: false } })
  mocks.push({ url: '/api/agent/session/cancel-chat', body: { success: true } })
  mocks.push({ url: '/api/agent/persona', method: 'GET', body: { name: C.PERSONA, portraitAssetId: null, avatarAssetId: null, hasCustomPersona: true, personality: '', persona: null, visualProfile: { visualIdentity: C.LOOK[L] }, mood: 72, moodRevision: 1, arousal: 62, activity: 'idle', doNotDisturb: false, doNotDisturbActive: false, zone: '' } })
}
