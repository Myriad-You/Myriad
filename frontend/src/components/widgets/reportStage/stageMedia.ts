/**
 * 舞台上跟着字幕出现的图：报告卡数据里的音乐封面、番剧/视频封面、游戏封面。
 * 每句台词提到了哪部作品（标题，或者歌手），这句就配上那几张；事实自带的图也算。
 */

import { resolveLibraryItemUrl } from '../../../utils/libraryItemUrl'

export type StageMediaShape = 'square' | 'poster' | 'wide' | 'banner'

export interface StageMedia {
  title: string
  cover: string
  /** 歌手之类，台词提到它也算提到了这张。 */
  by?: string
  shape: StageMediaShape
  /** 原站地址：卡片数据自带 id（番剧 season_id、条目 url 之类）时直接拼出来。 */
  href?: string
}

/** 卡片数据里带封面的几个列表，按卡面用到的顺序。 */
const MEDIA_LISTS = ['library_items', 'top_subjects', 'top_titles', 'recent_videos'] as const
/** 一份报告最多带多少张：书架只摆前几张，匹配也只在这些里找。 */
const MEDIA_LIMIT = 16
/** 一句最多配几张。 */
const MEDIA_PER_BEAT = 4

// 站内绝对路径或 http(s)。「//host」「/\host」浏览器都当成别的站，不放行。
const SAFE_MEDIA = /^(\/(?![/\\])|https?:\/\/)/i

function asRecord(value: unknown): Record<string, unknown> | null {
  return value && typeof value === 'object' && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null
}

function asText(value: unknown): string {
  return typeof value === 'string' ? value.trim() : ''
}

/** 封面的比例按条目类型定，图还没下来就占好位，出来时不跳。 */
function shapeOf(type: string, platform: string): StageMediaShape {
  const kind = type.toLowerCase()
  if (kind === 'music' || kind === 'song' || kind === 'album' || platform === 'netease') return 'square'
  if (kind === 'video' || platform === 'youtube') return 'wide'
  if (kind === 'game' || platform === 'steam') return 'banner'
  return 'poster'
}

/** 从一条报告的卡片数据里取出带封面的作品，按封面去重。 */
export function readStageMedia(cardVisuals: unknown, platform: string): StageMedia[] {
  const visuals = asRecord(cardVisuals)
  if (!visuals) return []
  const media: StageMedia[] = []
  const seen = new Set<string>()
  for (const key of MEDIA_LISTS) {
    const list = visuals[key]
    if (!Array.isArray(list)) continue
    for (const raw of list) {
      const item = asRecord(raw)
      const cover = asText(item?.cover) || asText(item?.image) || asText(item?.cover_url)
      const title = asText(item?.title) || asText(item?.name)
      if (!cover || !title || !SAFE_MEDIA.test(cover) || seen.has(cover)) continue
      seen.add(cover)
      const by = asText(item?.artist) || asText(item?.author)
      const type = asText(item?.type)
      const href = resolveLibraryItemUrl({ id: '', platform, item_type: type, metadata: item })
      media.push({
        title,
        cover,
        ...(by ? { by } : {}),
        shape: shapeOf(type, platform),
        ...(href ? { href } : {}),
      })
      if (media.length >= MEDIA_LIMIT) return media
    }
  }
  return media
}

/** 比对用：不分大小写、全半角，去掉空白和标点符号。 */
function normalize(text: string): string {
  return text.normalize('NFKC').toLowerCase().replaceAll(/[\s\p{P}\p{S}]/gu, '')
}

const SUBTITLE_SPLIT = /[（(【[「《：:｜|/\-–—~～]/u
const SEASON_SUFFIX = /(第[一二三四五六七八九十百\d]+[季期部章]|season\d+|s\d+)$/u

/** 一张图能被哪些说法点到：全名、去掉副标题和季数的主名、歌手。太短的不算，免得误配。 */
function keysOf(item: StageMedia): string[] {
  const main = item.title.split(SUBTITLE_SPLIT)[0] ?? ''
  const keys = new Set<string>()
  for (const name of [item.title, main, item.by ?? '']) {
    const key = normalize(name)
    keys.add(key)
    keys.add(key.replace(SEASON_SUFFIX, ''))
  }
  // 中日文两个字已经是一个名字；拉丁字母至少三个。
  return [...keys].filter((key) => (/^[\x00-\x7F]+$/.test(key) ? key.length >= 3 : key.length >= 2))
}

/** 这句台词提到了哪些作品，按在句子里出现的先后。 */
export function mediaMentionedIn(text: string, media: readonly StageMedia[]): StageMedia[] {
  const haystack = normalize(text)
  if (!haystack) return []
  return media
    .map((item) => {
      const positions = keysOf(item)
        .map((key) => haystack.indexOf(key))
        .filter((index) => index >= 0)
      return { item, at: positions.length > 0 ? Math.min(...positions) : -1 }
    })
    .filter((entry) => entry.at >= 0)
    .sort((a, b) => a.at - b.at)
    .slice(0, MEDIA_PER_BEAT)
    .map((entry) => entry.item)
}
