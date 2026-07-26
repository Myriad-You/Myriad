/**
 * 音乐播放器 - 支持网易云音乐和QQ音乐歌单播放
 */

import { API_URL } from '../config'
import { isUserInChinaMainland } from './geoLocation'

/**
 * 网易云「仅解析播放链」接口：后端 302 到 HTTPS CDN，音频字节仍直连网易。
 * 替代已失效于 HTTPS 站点的 outer/url（会跳到 http CDN → Mixed Content）。
 */
export function getNeteasePlayUrl(songId: string): string {
  return `${API_URL}/api/proxy/music/netease/play-url/${songId}`
}

/**
 * 网易云全量音频代理（字节经本机回传）。海外 / 需 CORS / 方案 C 降级时使用。
 */
export function getNeteaseProxyAudioUrl(songId: string): string {
  return `${API_URL}/api/proxy/music/netease/audio/${songId}`
}

/**
 * 是否为网易「直连」播放地址（play-url / 旧 outer / 已解析的 CDN）。
 * 已是全量代理 `/audio/` 时返回 false，避免方案 C 死循环。
 */
export function isNeteaseDirectPlayUrl(url: string): boolean {
  if (!url) return false
  if (url.includes('/api/proxy/music/netease/audio/')) return false
  if (url.includes('/api/proxy/music/netease/play-url/')) return true
  // 旧缓存 outer、或 302 后浏览器侧偶发残留的 CDN 绝对地址
  if (
    url.includes('music.126.net') ||
    url.includes('music.163.com/song/media') ||
    url.includes('music.163.com/song/media/outer')
  ) {
    return true
  }
  return false
}

/**
 * 方案 C：直连失败时的全量代理 URL。
 * 非网易、或已经是代理地址时返回 null。
 */
export function getNeteaseProxyFallbackUrl(
  song: Pick<Song, 'id' | 'source' | 'url'>,
): string | null {
  if (song.source !== 'netease') return null
  if (!isNeteaseDirectPlayUrl(song.url)) return null
  return getNeteaseProxyAudioUrl(song.id)
}

/**
 * 获取网易云音乐音频URL
 * 根据用户地理位置决定策略：
 * - 国内：play-url 解析后 302 到 HTTPS CDN（直连网易，无 Mixed Content）
 * - 海外：全量代理拉流
 *
 * @param songId 歌曲ID
 * @param useProxy 是否强制使用全量代理（覆盖自动检测）
 * @returns 音频URL（可直接赋给 audio.src）
 */
export async function getNeteaseAudioUrl(
  songId: string,
  useProxy?: boolean,
): Promise<string> {
  // 如果显式指定了是否使用代理
  if (useProxy !== undefined) {
    return useProxy
      ? getNeteaseProxyAudioUrl(songId)
      : getNeteasePlayUrl(songId)
  }

  // 自动检测是否需要代理
  const inChina = await isUserInChinaMainland()

  if (inChina) {
    // 中国大陆：只解析 HTTPS CDN 链，音频仍直连网易
    return getNeteasePlayUrl(songId)
  } else {
    // 海外用户：通过后端全量代理
    return getNeteaseProxyAudioUrl(songId)
  }
}

/**
 * 获取 QQ 音乐音频 URL（经后端解析 vkey 并代理）
 *
 * 旧直连 `ws.stream.qqmusic.qq.com/{songmid}.m4a?fromtag=46` 已失效（403）。
 * 统一走后端 `/api/proxy/music/qq/audio/{songmid}`，按需取临时播放链。
 */
export function getQQAudioUrl(songMid: string): string {
  return `${API_URL}/api/proxy/music/qq/audio/${songMid}`
}

/**
 * 节流函数 - 限制函数执行频率
 * @param func 要节流的函数
 * @param wait 等待时间（毫秒）
 */
export function throttle<T extends (...args: any[]) => any>(
  func: T,
  wait: number,
): ((...args: Parameters<T>) => void) & { cancel: () => void } {
  let timeout: NodeJS.Timeout | null = null
  let previous = 0

  const throttled = function (this: any, ...args: Parameters<T>) {
    const now = Date.now()
    const remaining = wait - (now - previous)

    if (remaining <= 0 || remaining > wait) {
      if (timeout) {
        clearTimeout(timeout)
        timeout = null
      }
      previous = now
      func.apply(this, args)
    } else if (!timeout) {
      timeout = setTimeout(() => {
        previous = Date.now()
        timeout = null
        func.apply(this, args)
      }, remaining)
    }
  } as ((...args: Parameters<T>) => void) & { cancel: () => void }

  throttled.cancel = () => {
    if (timeout) {
      clearTimeout(timeout)
      timeout = null
    }
    previous = 0
  }

  return throttled
}

/**
 * 防抖函数 - 延迟执行函数
 * @param func 要防抖的函数
 * @param wait 等待时间（毫秒）
 */
export function debounce<T extends (...args: any[]) => any>(
  func: T,
  wait: number,
): (...args: Parameters<T>) => void {
  let timeout: NodeJS.Timeout | null = null

  return function (this: any, ...args: Parameters<T>) {
    if (timeout) {
      clearTimeout(timeout)
    }

    timeout = setTimeout(() => {
      func.apply(this, args)
    }, wait)
  }
}

export type MusicSource = 'netease' | 'qq'

export interface Song {
  id: string
  name: string
  artist: string
  album: string
  cover: string
  url: string
  duration: number // 秒
  source: MusicSource
  // VIP歌曲标识
  isVip?: boolean // 是否为VIP歌曲
  isTrial?: boolean // 是否为试听版本
  trialDuration?: number // 试听时长（秒）
}

export interface LyricLine {
  time: number // 秒
  text: string
  translation?: string // 整行翻译（按时间就近对齐挂载，见 attachLyricTranslation）
}

// 逐字歌词单个 token（一个字/词）
export interface WordLyricToken {
  time: number // 秒，绝对开始时间
  duration: number // 秒，该字/词的持续时长
  text: string
}

// 逐字歌词单行（含逐字 token）
export interface WordLyricLine {
  time: number // 秒，行开始时间
  duration: number // 秒，行持续时长
  text: string // 整行文本（token 拼接）
  words: WordLyricToken[]
  translation?: string // 整行翻译（按时间就近对齐挂载）
}

// 逐字歌词结果：逐行(lines) 作为兜底 + 逐字(verbatim) 作为增强
export interface VerbatimLyricsResult {
  lines: LyricLine[]
  verbatim: WordLyricLine[]
  // 逐行翻译原始数组（网易 ytlrc 优先、tlyric 兜底，目前恒为中文）。
  // 已按时间就近挂到 lines/verbatim 各行 translation 字段；保留原始数组
  // 供跨源（酷狗）verbatim 采纳后再次对齐
  translation: LyricLine[]
}

export type VerbatimLyricsSource = 'netease' | 'kugou' | ''

export interface LyricsWithVerbatimResult extends VerbatimLyricsResult {
  source: MusicSource
  hasVerbatim: boolean
  verbatimSource: VerbatimLyricsSource
  hasTranslation: boolean
  translationLang: 'zh' | '' // 翻译目标语言（Phase 1 只有网易中文翻译源）
}

// 歌词缓存（限制最大100首，使用LRU策略）
const lyricsCache = new Map<string, LyricLine[]>()
const MAX_LYRICS_CACHE_SIZE = 100

// 添加歌词到缓存（LRU策略）
function addToLyricsCache(key: string, lyrics: LyricLine[]): void {
  // 如果已存在，先删除再添加（保证最新的在最后）
  if (lyricsCache.has(key)) {
    lyricsCache.delete(key)
  }

  // 如果达到上限，删除最旧的（第一个）
  if (lyricsCache.size >= MAX_LYRICS_CACHE_SIZE) {
    const firstKey = lyricsCache.keys().next().value
    if (firstKey) {
      lyricsCache.delete(firstKey)
    }
  }

  lyricsCache.set(key, lyrics)
}

// 歌单缓存（内存 + SessionStorage）
interface PlaylistCacheEntry {
  data: Song[]
  timestamp: number
}

const playlistMemoryCache = new Map<string, PlaylistCacheEntry>()
const PLAYLIST_CACHE_DURATION = 7 * 24 * 60 * 60 * 1000 // 7天
// v3：网易国内改 play-url（HTTPS CDN 302），淘汰 outer/url 旧缓存
const PLAYLIST_STORAGE_KEY = 'myriad_playlist_cache_v3'
const MAX_PLAYLIST_CACHE_SIZE = 5

function setPlaylistMemoryCache(
  cacheKey: string,
  entry: PlaylistCacheEntry,
): void {
  playlistMemoryCache.delete(cacheKey)
  playlistMemoryCache.set(cacheKey, entry)
  while (playlistMemoryCache.size > MAX_PLAYLIST_CACHE_SIZE) {
    const oldestKey = playlistMemoryCache.keys().next().value
    if (oldestKey === undefined) break
    playlistMemoryCache.delete(oldestKey)
  }
}

// 头部制作信息行（制作人/作词/作曲/编曲…）：网易云 lrc 常把 credit 挤在 0~10s，
// 它们不是歌词——最后一行 credit 会作为「歌词」高亮挂到真人声进来为止（乱轴观感）
const CREDIT_LINE_RE =
  /^(制作人|出品|监制|作词|作曲|编曲|歌词|翻译|混音|母带|录音|和声|吉他|贝斯|键盘|弦乐|[鼓词曲]|企划|统筹|发行|OP|SP|Produce[rd]?|Lyric(?:s|ist)?|Compose[rd]?|Arrange[rd]?|Mix(?:ing)?|Master(?:ing)?)\s*[:：]/i

/**
 * 解析LRC格式歌词
 */
export function parseLyrics(lrcText: string): LyricLine[] {
  const lines = lrcText.split('\n')
  const lyrics: LyricLine[] = []

  for (const line of lines) {
    // 匹配时间标签 [mm:ss.xx] 或 [mm:ss]
    const match = line.match(/\[(\d{2}):(\d{2})(?:\.(\d{2,3}))?\](.*)/)
    if (match) {
      const minutes = Number.parseInt(match[1], 10)
      const seconds = Number.parseInt(match[2], 10)
      const milliseconds = match[3]
        ? Number.parseInt(match[3].padEnd(3, '0'), 10)
        : 0
      const text = match[4].trim()
      const time = minutes * 60 + seconds + milliseconds / 1000

      // 过滤头部 credit 行（双条件：前 15s + 命中制作信息模式，避免误杀真歌词）
      if (text && !(time < 15 && CREDIT_LINE_RE.test(text))) {
        lyrics.push({ time, text })
      }
    }
  }

  // 按时间排序
  return lyrics.sort((a, b) => a.time - b.time)
}

/**
 * 解析网易云 yrc 逐字歌词格式
 *
 * 行格式: `[行起始ms,行时长ms](字起始ms,字时长ms,0)字(字起始ms,字时长ms,0)字...`
 * 以 `{` 开头的行是 JSON 元数据（作词/翻译等），跳过。
 */
export function parseYrc(yrcText: string): WordLyricLine[] {
  if (!yrcText) return []

  const rawLines = yrcText.split('\n')
  const result: WordLyricLine[] = []
  const headerRe = /^\[(\d+),(\d+)\]/
  // 每个 token: (起始ms,时长ms,附加)文本 —— 文本读到下一个左括号前
  const wordRe = /\((\d+),(\d+),\d+\)([^(]*)/g

  for (const raw of rawLines) {
    const line = raw.trim()
    if (!line || line.charAt(0) === '{') continue

    const header = headerRe.exec(line)
    if (!header) continue

    const lineStart = Number(header[1]) / 1000
    const lineDuration = Number(header[2]) / 1000

    const words: WordLyricToken[] = []
    let text = ''
    wordRe.lastIndex = 0
    // 正则逐个 exec：赋值移出条件，满足 no-cond-assign
    let m: RegExpExecArray | null = wordRe.exec(line)
    while (m !== null) {
      const wordText = m[3]
      words.push({
        time: Number(m[1]) / 1000,
        duration: Number(m[2]) / 1000,
        text: wordText,
      })
      text += wordText
      m = wordRe.exec(line)
    }

    if (words.length === 0) continue
    result.push({
      time: lineStart,
      duration: lineDuration,
      text: text.trim(),
      words,
    })
  }

  return result.sort((a, b) => a.time - b.time)
}

/**
 * 解析酷狗 KRC 逐字歌词格式
 *
 * 行格式: `[行起始ms,行时长ms]<字偏移ms,字时长ms,0>字<字偏移ms,字时长ms,0>字...`
 * 注意：字偏移是相对「行起始」的，绝对时间 = 行起始 + 字偏移（与网易云 yrc 的绝对时间不同）。
 * 以 `[ti:]` `[ar:]` `[offset:]` 等元数据行不匹配 `[数字,数字]`，自动跳过。
 */
export function parseKrc(krcText: string): WordLyricLine[] {
  if (!krcText) return []

  const result: WordLyricLine[] = []
  const headerRe = /^\[(\d+),(\d+)\]/
  // token: <偏移ms,时长ms,附加>文本 —— 文本读到下一个 `<` 前
  const wordRe = /<(\d+),(\d+),\d+>([^<]*)/g

  for (const raw of krcText.split('\n')) {
    const line = raw.trim()
    if (!line || line.charAt(0) !== '[') continue

    const header = headerRe.exec(line)
    if (!header) continue // 跳过 [ti:]/[ar:]/[offset:] 等元数据行

    const lineStart = Number(header[1]) / 1000
    const lineDuration = Number(header[2]) / 1000

    const words: WordLyricToken[] = []
    let text = ''
    wordRe.lastIndex = 0
    // 赋值移出条件，满足 no-cond-assign
    let m: RegExpExecArray | null = wordRe.exec(line)
    while (m !== null) {
      const wordText = m[3]
      words.push({
        time: lineStart + Number(m[1]) / 1000, // 相对偏移转绝对时间
        duration: Number(m[2]) / 1000,
        text: wordText,
      })
      text += wordText
      m = wordRe.exec(line)
    }

    if (words.length === 0) continue
    result.push({
      time: lineStart,
      duration: lineDuration,
      text: text.trim(),
      words,
    })
  }

  return result.sort((a, b) => a.time - b.time)
}

/**
 * 从缓存获取歌单
 */
function getPlaylistFromCache(cacheKey: string): Song[] | null {
  // 1. 先检查内存缓存
  const memoryCache = playlistMemoryCache.get(cacheKey)
  if (
    memoryCache &&
    Date.now() - memoryCache.timestamp < PLAYLIST_CACHE_DURATION
  ) {
    // 刷新 LRU 顺序，避免常用歌单被优先淘汰。
    setPlaylistMemoryCache(cacheKey, memoryCache)
    return memoryCache.data
  }
  if (memoryCache) playlistMemoryCache.delete(cacheKey)

  // 2. 检查 SessionStorage
  try {
    const storageData = sessionStorage.getItem(PLAYLIST_STORAGE_KEY)
    if (storageData) {
      const allCache = JSON.parse(storageData) as Record<
        string,
        PlaylistCacheEntry
      >
      const cached = allCache[cacheKey]

      if (cached && Date.now() - cached.timestamp < PLAYLIST_CACHE_DURATION) {
        // 恢复到内存缓存
        setPlaylistMemoryCache(cacheKey, cached)
        return cached.data
      }
    }
  } catch (_error) {
    // SessionStorage 读取失败，静默处理
  }

  return null
}

/**
 * 将歌单存入缓存
 */
function savePlaylistToCache(cacheKey: string, songs: Song[]): void {
  const entry: PlaylistCacheEntry = {
    data: songs,
    timestamp: Date.now(),
  }

  // 1. 存入内存缓存
  setPlaylistMemoryCache(cacheKey, entry)

  // 2. 存入 SessionStorage（限制总大小）
  try {
    const storageData = sessionStorage.getItem(PLAYLIST_STORAGE_KEY)
    const allCache: Record<string, PlaylistCacheEntry> = storageData
      ? JSON.parse(storageData)
      : {}

    // 清理过期缓存
    Object.keys(allCache).forEach((key) => {
      if (Date.now() - allCache[key].timestamp > PLAYLIST_CACHE_DURATION) {
        delete allCache[key]
      }
    })

    // 添加新缓存
    allCache[cacheKey] = entry

    // 限制缓存数量（与内存缓存一致，最多5个歌单）
    const keys = Object.keys(allCache)
    if (keys.length > MAX_PLAYLIST_CACHE_SIZE) {
      // 删除最旧的
      const oldestKey = keys.reduce((oldest, key) => {
        return allCache[key].timestamp < allCache[oldest].timestamp
          ? key
          : oldest
      }, keys[0])
      delete allCache[oldestKey]
    }

    sessionStorage.setItem(PLAYLIST_STORAGE_KEY, JSON.stringify(allCache))
  } catch (error) {
    // SessionStorage 写入失败（可能配额已满），仅保留内存缓存
    console.warn('Failed to save playlist to SessionStorage:', error)
  }
}

/**
 * 清空歌单缓存
 */
export function clearPlaylistCache(): void {
  playlistMemoryCache.clear()
  try {
    sessionStorage.removeItem(PLAYLIST_STORAGE_KEY)
  } catch (_error) {
    // 静默处理
  }
}

/**
 * 清空歌词缓存
 */
export function clearLyricsCache(): void {
  lyricsCache.clear()
  verbatimLyricsCache.clear()
  kugouVerbatimCache.clear()
}

/**
 * 获取网易云音乐歌单（带缓存）
 * 会根据用户地理位置自动决定音频URL是使用代理还是直连
 */
export async function getNeteasePlaylist(playlistId: string): Promise<Song[]> {
  const cacheKey = `netease-${playlistId}`

  // 检查缓存
  const cached = getPlaylistFromCache(cacheKey)
  if (cached) {
    return cached
  }

  try {
    // 预先检测用户地理位置（并行执行，不阻塞歌单请求）
    const geoPromise = isUserInChinaMainland()

    // 通过后端代理访问网易云音乐API（歌单信息始终通过代理获取，确保稳定性）
    const response = await fetch(
      `${API_URL}/api/proxy/music/netease/playlist/${playlistId}`,
    )

    if (!response.ok) {
      throw new Error('Failed to fetch playlist')
    }

    const data = await response.json()

    // NetEase API 返回格式: { code: 200, result: { playlist: { tracks: [...] } } }
    // 或者可能是: { playlist: { tracks: [...] } }
    if (data.code && data.code !== 200) {
      // 网易云常见错误码:
      // -447: 服务器忙碌/频率限制
      // -460: 地理位置限制(海外IP)
      // -462: 版权限制
      if (data.code === -447) {
        throw new Error('网易云API访问频率过高,请稍后再试或使用QQ音乐')
      } else if (data.code === -460 || data.code === -462) {
        throw new Error('该歌单因版权或地理位置限制无法播放,建议使用QQ音乐')
      }
      throw new Error(data.message || `网易云API错误 (${data.code})`)
    }

    const tracks = data.result?.playlist?.tracks || data.playlist?.tracks || []
    if (tracks.length === 0) {
      throw new Error('歌单为空或无可用歌曲')
    }

    // 等待地理位置检测结果
    const inChina = await geoPromise
    console.log(
      `[MusicPlayer] 歌单加载完成，用户在中国大陆: ${inChina}，${inChina ? 'play-url 直连 CDN' : '全量代理'}`,
    )

    const songs = tracks.map((track: any) => {
      // 网易云音乐API v6返回格式：ar(艺术家数组), al(专辑对象), dt(时长毫秒)
      // 兼容旧格式：artists, album, duration
      const artists = track.ar || track.artists || []
      const album = track.al || track.album || {}
      const duration = track.dt || track.duration || 0

      // 直接使用后端返回的isVip字段（后端已经根据fee字段处理好了）
      const isVip = track.isVip || false
      const isTrial = false // 网易云playlist接口不返回试听信息
      const trialDuration = undefined

      // 国内：后端解析临时链并 302 到 HTTPS CDN（音频直连网易，无 Mixed Content）
      // 海外：全量代理拉流（绕过地理限制）
      // 方案 C：直连失败时在 useMusicPlayer 降级到 getNeteaseProxyAudioUrl
      const audioUrl = inChina
        ? getNeteasePlayUrl(String(track.id))
        : getNeteaseProxyAudioUrl(String(track.id))

      return {
        id: track.id.toString(),
        name: track.name,
        artist: artists.map((a: any) => a.name).join(', ') || 'Unknown',
        album: album.name || '',
        cover: album.picUrl || album.blurPicUrl || '',
        url: audioUrl,
        duration: Math.floor(duration / 1000),
        source: 'netease' as MusicSource,
        isVip,
        isTrial,
        trialDuration,
      }
    })

    // 存入缓存
    savePlaylistToCache(cacheKey, songs)

    return songs
  } catch (error) {
    console.error('Error fetching Netease playlist:', error)
    return []
  }
}

/**
 * 获取QQ音乐歌单（带缓存）
 */
export async function getQQPlaylist(playlistId: string): Promise<Song[]> {
  const cacheKey = `qq-${playlistId}`

  // 检查缓存
  const cached = getPlaylistFromCache(cacheKey)
  if (cached) {
    return cached
  }

  try {
    // 通过后端代理访问QQ音乐API
    const response = await fetch(
      `${API_URL}/api/proxy/music/qq/playlist/${playlistId}`,
    )

    if (!response.ok) {
      throw new Error('Failed to fetch playlist')
    }

    const data = await response.json()

    if (!data.cdlist || data.cdlist.length === 0) {
      throw new Error('Invalid playlist response')
    }

    const playlist = data.cdlist[0]
    const songlist = playlist.songlist || []

    const songs = songlist
      .map((song: any) => {
        // QQ音乐返回格式：singer(歌手数组), albumname(专辑名), interval(时长秒)
        const singers = Array.isArray(song.singer) ? song.singer : []
        const songMid = String(song.songmid || song.id || '').trim()
        if (!songMid) {
          return null
        }

        return {
          id: songMid,
          name: song.songname || song.name,
          artist:
            singers.length > 0
              ? singers.map((s: any) => s.name).join(', ')
              : 'Unknown',
          album: song.albumname || song.album?.name || '',
          cover: song.albummid
            ? `https://y.gtimg.cn/music/photo_new/T002R300x300M000${song.albummid}.jpg`
            : '',
          // 经后端 GetEVkey 解析临时播放链；旧 fromtag=46 直连已 403
          url: getQQAudioUrl(songMid),
          duration: song.interval || 0,
          source: 'qq' as MusicSource,
          // 后端会补 isVip；无字段时默认 false
          isVip: Boolean(song.isVip),
        } satisfies Song
      })
      .filter((song: Song | null): song is Song => song !== null)

    // 存入缓存
    savePlaylistToCache(cacheKey, songs)

    return songs
  } catch (error) {
    console.error('Error fetching QQ playlist:', error)
    return []
  }
}

/**
 * 获取网易云音乐歌词
 */
export async function getNeteaseLyrics(songId: string): Promise<LyricLine[]> {
  const cacheKey = `netease-${songId}`

  // 检查缓存
  if (lyricsCache.has(cacheKey)) {
    return lyricsCache.get(cacheKey)!
  }

  try {
    const response = await fetch(
      `${API_URL}/api/proxy/music/netease/lyrics/${songId}`,
    )

    if (!response.ok) {
      throw new Error('Failed to fetch lyrics')
    }

    const data = await response.json()

    if (data.lrc?.lyric) {
      const lyrics = parseLyrics(data.lrc.lyric)
      addToLyricsCache(cacheKey, lyrics)
      return lyrics
    }

    return []
  } catch (error) {
    console.error('Error fetching Netease lyrics:', error)
    return []
  }
}

// 逐字歌词缓存（复用 LRU 大小上限）
const verbatimLyricsCache = new Map<string, VerbatimLyricsResult>()

/**
 * 获取网易云逐字歌词（yrc）+ 逐行兜底
 *
 * 返回 { lines, verbatim }：verbatim 为空时消费方应回退到 lines。
 */
export async function getNeteaseVerbatimLyrics(
  songId: string,
): Promise<VerbatimLyricsResult> {
  const cacheKey = `netease-v1-${songId}`

  const cached = verbatimLyricsCache.get(cacheKey)
  if (cached) return cached

  try {
    const response = await fetch(
      `${API_URL}/api/proxy/music/netease/lyrics-verbatim/${songId}`,
    )

    if (!response.ok) {
      throw new Error('Failed to fetch verbatim lyrics')
    }

    const data = await response.json()

    const lines: LyricLine[] = data.lrc?.lyric ? parseLyrics(data.lrc.lyric) : []
    const verbatim: WordLyricLine[] = data.yrc?.lyric
      ? parseYrc(data.yrc.lyric)
      : []

    // 逐行翻译：ytlrc 与 yrc 同轴（优先），tlyric 与 lrc 同轴（兜底）。
    // 均为标准 LRC 文本；非中文歌通常有，中文歌为空
    const translationRaw = data.ytlrc?.lyric || data.tlyric?.lyric || ''
    const translation: LyricLine[] = translationRaw
      ? parseLyrics(translationRaw)
      : []
    attachLyricTranslation(lines, translation)
    attachLyricTranslation(verbatim, translation)

    const result: VerbatimLyricsResult = { lines, verbatim, translation }

    // LRU：超上限删最旧
    if (verbatimLyricsCache.size >= MAX_LYRICS_CACHE_SIZE) {
      const firstKey = verbatimLyricsCache.keys().next().value
      if (firstKey) verbatimLyricsCache.delete(firstKey)
    }
    verbatimLyricsCache.set(cacheKey, result)

    return result
  } catch (error) {
    console.error('Error fetching Netease verbatim lyrics:', error)
    return { lines: [], verbatim: [], translation: [] }
  }
}

/**
 * 把逐行翻译按时间就近挂到歌词行的 translation 字段
 *
 * 翻译（tlyric）时间戳与 lrc 逐行一致，但展示行可能来自 yrc/KRC（同一行
 * 起始时间有数百毫秒级出入），所以按「最近时间 + 容差」匹配而不按索引对位。
 * 每条翻译只认领一个最近的行（更近者胜出），避免密集行重复同一条翻译。
 */
export function attachLyricTranslation(
  entries: Array<{ time: number; translation?: string }>,
  translation: LyricLine[],
  toleranceSec = 1.0,
): void {
  if (entries.length === 0 || translation.length === 0) return
  const claimed = new Map<number, number>() // 行下标 → 已挂翻译的时间差
  for (const t of translation) {
    let best = -1
    let bestD = toleranceSec
    for (let i = 0; i < entries.length; i++) {
      const d = Math.abs(entries[i].time - t.time)
      if (d < bestD) {
        bestD = d
        best = i
      }
    }
    if (best < 0) continue
    const prev = claimed.get(best)
    if (prev === undefined || bestD < prev) {
      claimed.set(best, bestD)
      entries[best].translation = t.text
    }
  }
}

/**
 * 跨源歌词时间轴校准
 *
 * 酷狗 KRC 按「歌名+时长」匹配，可能命中不同版本（现场/remix/不同剪辑），
 * 时间轴相对当前音频整体偏移甚至结构不符 → 歌词「乱轴」。
 * 用同源可信的逐行时间轴（网易云 lrc）做中位数对齐：
 *  - 常数偏移 → 整体平移校正（含逐字 token）
 *  - 平移后残差仍大（结构不符 = 不同版本/不同歌）→ 返回 null 拒绝
 */
export function alignVerbatimToLines(
  verbatim: WordLyricLine[],
  lines: LyricLine[],
): WordLyricLine[] | null {
  if (verbatim.length < 4 || lines.length < 4) return verbatim // 样本不足，无法校验

  // 每条可信行找最近的 verbatim 行，收集时间差
  const diffs: number[] = []
  for (const ln of lines) {
    let bestDiff = Infinity
    for (const v of verbatim) {
      const d = v.time - ln.time
      if (Math.abs(d) < Math.abs(bestDiff)) bestDiff = d
    }
    if (Number.isFinite(bestDiff)) diffs.push(bestDiff)
  }
  if (diffs.length < 4) return verbatim

  diffs.sort((a, b) => a - b)
  const median = diffs[Math.floor(diffs.length / 2)]
  const residuals = diffs
    .map((d) => Math.abs(d - median))
    .sort((a, b) => a - b)
  const medResidual = residuals[Math.floor(residuals.length / 2)]

  if (medResidual > 1.2) return null // 结构不符：拒绝该源
  if (Math.abs(median) < 0.08) return verbatim // 已对齐

  // 常数偏移：整体平移
  return verbatim.map((v) => ({
    ...v,
    time: Math.max(0, v.time - median),
    words: v.words.map((w) => ({ ...w, time: Math.max(0, w.time - median) })),
  }))
}

// 酷狗逐字歌词缓存（按 关键词|时长秒 缓存）
const kugouVerbatimCache = new Map<string, WordLyricLine[]>()

/**
 * 获取酷狗逐字歌词（KRC）—— 网易云 yrc 缺失时的补充第三方源
 *
 * @param keyword 建议「歌名 歌手」
 * @param durationSec 歌曲时长（秒），用于挑最接近的版本
 */
export async function getKugouVerbatimLyrics(
  keyword: string,
  durationSec = 0,
): Promise<WordLyricLine[]> {
  const kw = (keyword || '').trim()
  if (!kw) return []

  const cacheKey = `${kw}|${Math.round(durationSec)}`
  const cached = kugouVerbatimCache.get(cacheKey)
  if (cached) return cached

  try {
    const params = new URLSearchParams({
      keyword: kw,
      duration: String(Math.round(durationSec * 1000)),
    })
    const response = await fetch(
      `${API_URL}/api/proxy/music/kugou/lyrics-verbatim?${params.toString()}`,
    )
    if (!response.ok) throw new Error('Failed to fetch kugou verbatim lyrics')

    const data = await response.json()
    const verbatim: WordLyricLine[] = data.krc ? parseKrc(data.krc) : []

    if (kugouVerbatimCache.size >= MAX_LYRICS_CACHE_SIZE) {
      const firstKey = kugouVerbatimCache.keys().next().value
      if (firstKey) kugouVerbatimCache.delete(firstKey)
    }
    kugouVerbatimCache.set(cacheKey, verbatim)

    return verbatim
  } catch (error) {
    console.error('Error fetching KuGou verbatim lyrics:', error)
    return []
  }
}

/**
 * 获取QQ音乐歌词
 */
export async function getQQLyrics(songId: string): Promise<LyricLine[]> {
  const cacheKey = `qq-${songId}`

  // 检查缓存
  if (lyricsCache.has(cacheKey)) {
    return lyricsCache.get(cacheKey)!
  }

  try {
    const response = await fetch(
      `${API_URL}/api/proxy/music/qq/lyrics/${songId}`,
    )

    if (!response.ok) {
      throw new Error('Failed to fetch lyrics')
    }

    const data = await response.json()

    if (data.lyric) {
      const lyrics = parseLyrics(data.lyric)
      addToLyricsCache(cacheKey, lyrics)
      return lyrics
    }

    return []
  } catch (error) {
    console.error('Error fetching QQ lyrics:', error)
    return []
  }
}

/**
 * 获取歌曲歌词：逐字优先，多源兜底。
 *
 * 主源：网易云 yrc / QQ 逐行；逐字兜底：酷狗 KRC（按歌名 + 主歌手 + 时长）。
 * 返回的 lines 始终是逐行兜底，verbatim 为空时消费方应退回 lines。
 */
export async function getLyricsWithVerbatim(
  song: Pick<Song, 'id' | 'source' | 'name' | 'artist' | 'duration'>,
): Promise<LyricsWithVerbatimResult> {
  let lines: LyricLine[] = []
  let verbatim: WordLyricLine[] = []
  let verbatimSource: VerbatimLyricsSource = ''
  let translation: LyricLine[] = []

  if (song.source === 'qq') {
    lines = await getQQLyrics(song.id)
  } else {
    const result = await getNeteaseVerbatimLyrics(song.id)
    lines = result.lines
    verbatim = result.verbatim
    translation = result.translation
    if (verbatim.length > 0) {
      verbatimSource = 'netease'
    }
  }

  if (verbatim.length === 0 && song.name) {
    const mainArtist = (song.artist || '').split(/[,/、&×]/)[0].trim()
    const keyword = mainArtist ? `${song.name} ${mainArtist}` : song.name
    const kugou = await getKugouVerbatimLyrics(keyword, song.duration || 0)

    if (kugou.length > 0) {
      const aligned = alignVerbatimToLines(kugou, lines)
      if (aligned) {
        verbatim = aligned
        verbatimSource = 'kugou'
        // 酷狗行已校准到网易时间轴，翻译（网易）可直接就近挂载
        attachLyricTranslation(verbatim, translation)
      } else {
        console.debug(
          '[MusicPlayer] KuGou verbatim rejected: timeline mismatch for',
          keyword,
        )
      }
    }
  }

  if (lines.length === 0 && verbatim.length > 0) {
    lines = verbatim.map((line) => ({
      time: line.time,
      text: line.text,
      translation: line.translation,
    }))
  }

  return {
    lines,
    verbatim,
    translation,
    source: song.source,
    hasVerbatim: verbatim.length > 0,
    verbatimSource,
    hasTranslation: translation.length > 0,
    translationLang: translation.length > 0 ? 'zh' : '',
  }
}

/**
 * 根据当前播放时间获取当前歌词索引
 * 重构版：精确匹配，正确处理所有边界情况
 */
export function getCurrentLyricIndex(
  lyrics: LyricLine[],
  currentTime: number,
): number {
  if (!lyrics || lyrics.length === 0) return -1

  // 如果还没到第一句歌词的时间，返回 -1 表示没有当前歌词
  if (currentTime < lyrics[0].time) {
    return -1
  }

  // 找到当前时间应该显示的歌词索引
  // 规则：显示最后一个时间小于等于当前时间的歌词
  let currentIndex = -1

  for (let i = 0; i < lyrics.length; i++) {
    if (lyrics[i].time <= currentTime) {
      currentIndex = i
    } else {
      // 因为歌词已按时间排序，后面的都不会匹配了
      break
    }
  }

  return currentIndex
}

/**
 * 格式化时间（秒 -> mm:ss）
 */
export function formatTime(seconds: number): string {
  const mins = Math.floor(seconds / 60)
  const secs = Math.floor(seconds % 60)
  return `${mins.toString().padStart(2, '0')}:${secs.toString().padStart(2, '0')}`
}

/**
 * 检查歌曲是否为VIP或试听版本
 */
export function getSongVipStatus(song: {
  isVip?: boolean
  isTrial?: boolean
}): {
  isVip: boolean
  isTrial: boolean
  displayText: string
} {
  const isVip = song.isVip || false
  const isTrial = song.isTrial || false

  // 简化显示：VIP歌曲直接显示VIP标识
  const displayText = isVip ? 'VIP' : ''

  return { isVip, isTrial, displayText }
}

/**
 * 过滤播放列表
 * @param songs 歌曲列表
 * @param query 搜索关键词
 * @param options 过滤选项
 */
export function filterPlaylist(
  songs: Song[],
  query: string,
  options?: {
    hideVip?: boolean // 隐藏VIP歌曲
    hideTrial?: boolean // 隐藏试听歌曲
  },
): Song[] {
  let filtered = songs

  // 根据VIP状态过滤
  if (options?.hideVip) {
    filtered = filtered.filter((song) => !song.isVip)
  }
  if (options?.hideTrial) {
    filtered = filtered.filter((song) => !song.isTrial)
  }

  // 根据搜索关键词过滤
  if (query && query.trim()) {
    const lowerQuery = query.toLowerCase().trim()
    filtered = filtered.filter(
      (song) =>
        song.name.toLowerCase().includes(lowerQuery) ||
        song.artist.toLowerCase().includes(lowerQuery) ||
        song.album.toLowerCase().includes(lowerQuery),
    )
  }

  return filtered
}

/**
 * 高亮搜索关键词
 * @param text 原文本
 * @param query 搜索关键词
 */
export function highlightText(text: string, query: string): string {
  // 先转义 HTML 元字符，防止 XSS
  const escaped = text
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')

  if (!query || !query.trim()) {
    return escaped
  }

  const regex = new RegExp(
    `(${query.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')})`,
    'gi',
  )
  return escaped.replace(regex, '<mark>$1</mark>')
}

/**
 * 移动端是否应走原生 HTMLAudio 输出（不经 Web Audio 图）。
 *
 * createMediaElementSource 会把 <audio> 输出永久劫持到 AudioContext；
 * 页面进后台时系统会 suspend AudioContext → 音乐静音/停止。
 * iOS/Android 后台播放必须保留原生媒体通路 + Media Session。
 */
export function shouldPreserveNativeAudioOutput(): boolean {
  if (typeof navigator === 'undefined' || typeof window === 'undefined') {
    return false
  }
  const ua = navigator.userAgent || ''
  // iPhone / iPod / 旧 iPad
  if (/iPad|iPhone|iPod/.test(ua)) return true
  // iPadOS 13+ 桌面 UA
  if (navigator.platform === 'MacIntel' && navigator.maxTouchPoints > 1) {
    return true
  }
  // 粗指针移动设备（多数 Android 手机）
  try {
    if (window.matchMedia('(hover: none) and (pointer: coarse)').matches) {
      return true
    }
  } catch {
    // matchMedia 不可用时按 UA 兜底
  }
  if (/Android/i.test(ua)) return true
  return false
}

/**
 * 创建适合移动端后台播放的 Audio 元素。
 * - 挂入 DOM（部分 WebKit 对 detached Audio 后台限流更狠）
 * - playsinline，避免被当成需全屏的媒体
 */
export function createPlaybackAudioElement(
  volume: number = 1,
): HTMLAudioElement {
  const audio = new Audio()
  audio.volume = volume
  audio.preload = 'auto'
  audio.setAttribute('playsinline', 'true')
  audio.setAttribute('webkit-playsinline', 'true')
  audio.setAttribute('data-myriad-audio', 'playback')
  // 不可见但保留在文档树中，便于系统识别为页面媒体会话
  Object.assign(audio.style, {
    position: 'fixed',
    width: '0',
    height: '0',
    opacity: '0',
    pointerEvents: 'none',
    zIndex: '-1',
  })
  if (typeof document !== 'undefined' && document.body) {
    document.body.appendChild(audio)
  }
  return audio
}

/** 销毁由 createPlaybackAudioElement 创建的音频元素 */
export function destroyPlaybackAudioElement(
  audio: HTMLAudioElement | null | undefined,
): void {
  if (!audio) return
  try {
    audio.pause()
    audio.removeAttribute('src')
    audio.load()
    audio.remove()
  } catch {
    // 清理失败可忽略
  }
}

/**
 * 全局音频管理器 - 确保同一时间只有一个音频在播放
 * 支持实时音频频谱分析
 */
class GlobalAudioManager {
  private static instance: GlobalAudioManager
  private currentAudio: HTMLAudioElement | null = null
  private currentSong: Song | null = null

  // Web Audio API 相关 - 用于频谱分析
  private audioContext: AudioContext | null = null
  private analyser: AnalyserNode | null = null
  private sourceNode: MediaElementAudioSourceNode | null = null
  private connectedAudio: HTMLAudioElement | null = null // 追踪已连接的音频元素
  private frequencyData: Uint8Array | null = null
  /** 一旦为 true，本会话内不再尝试把媒体元素接入 AudioContext */
  private nativeOutputLocked = false

  private constructor() {}

  static getInstance(): GlobalAudioManager {
    if (!GlobalAudioManager.instance) {
      GlobalAudioManager.instance = new GlobalAudioManager()
    }
    return GlobalAudioManager.instance
  }

  /**
   * 设置当前音频实例
   * 会自动停止并清理之前的音频
   */
  setCurrentAudio(
    audio: HTMLAudioElement | null,
    song: Song | null = null,
  ): void {
    // 如果有之前的音频在播放，先停止并清理
    if (this.currentAudio && this.currentAudio !== audio) {
      this.currentAudio.pause()
      this.currentAudio.src = ''
      this.currentAudio.load() // 重置音频元素
    }

    this.currentAudio = audio
    this.currentSong = song

    // 如果设置了新音频和歌曲信息，更新 Media Session
    if (audio && song) {
      this.updateMediaSession(song)
    }
  }

  /**
   * 获取当前音频实例
   */
  getCurrentAudio(): HTMLAudioElement | null {
    return this.currentAudio
  }

  /**
   * 获取当前歌曲信息
   */
  getCurrentSong(): Song | null {
    return this.currentSong
  }

  /**
   * 停止当前音频
   */
  stopCurrentAudio(): void {
    if (this.currentAudio) {
      this.currentAudio.pause()
      this.currentAudio.src = ''
      this.currentAudio.load()
      this.currentAudio = null
      this.currentSong = null
      this.clearMediaSession()
    }
  }

  /**
   * 更新 Media Session API (移动端系统级媒体控制)
   */
  private updateMediaSession(song: Song): void {
    if ('mediaSession' in navigator) {
      navigator.mediaSession.metadata = new MediaMetadata({
        title: song.name,
        artist: song.artist,
        album: song.album,
        artwork: [
          { src: song.cover, sizes: '96x96', type: 'image/jpeg' },
          { src: song.cover, sizes: '128x128', type: 'image/jpeg' },
          { src: song.cover, sizes: '192x192', type: 'image/jpeg' },
          { src: song.cover, sizes: '256x256', type: 'image/jpeg' },
          { src: song.cover, sizes: '384x384', type: 'image/jpeg' },
          { src: song.cover, sizes: '512x512', type: 'image/jpeg' },
        ],
      })
    }
  }

  /**
   * 清除 Media Session
   */
  private clearMediaSession(): void {
    if ('mediaSession' in navigator) {
      navigator.mediaSession.metadata = null
    }
  }

  /**
   * 设置 Media Session 操作处理器
   */
  setMediaSessionHandlers(handlers: {
    play?: () => void
    pause?: () => void
    previoustrack?: () => void
    nexttrack?: () => void
    seekbackward?: () => void
    seekforward?: () => void
    seekto?: (details: { seekTime: number }) => void
  }): void {
    if ('mediaSession' in navigator) {
      // 设置播放/暂停
      if (handlers.play) {
        try {
          navigator.mediaSession.setActionHandler('play', handlers.play)
        } catch {
          // Media Session action not supported
        }
      }

      if (handlers.pause) {
        try {
          navigator.mediaSession.setActionHandler('pause', handlers.pause)
        } catch {
          // Media Session action not supported
        }
      }

      // 设置上一首/下一首
      if (handlers.previoustrack) {
        try {
          navigator.mediaSession.setActionHandler(
            'previoustrack',
            handlers.previoustrack,
          )
        } catch {
          // Media Session action not supported
        }
      }

      if (handlers.nexttrack) {
        try {
          navigator.mediaSession.setActionHandler(
            'nexttrack',
            handlers.nexttrack,
          )
        } catch {
          // Media Session action not supported
        }
      }

      // 设置快进/快退
      if (handlers.seekbackward) {
        try {
          navigator.mediaSession.setActionHandler(
            'seekbackward',
            handlers.seekbackward,
          )
        } catch {
          // Media Session action not supported
        }
      }

      if (handlers.seekforward) {
        try {
          navigator.mediaSession.setActionHandler(
            'seekforward',
            handlers.seekforward,
          )
        } catch {
          // Media Session action not supported
        }
      }

      // 设置进度跳转
      if (handlers.seekto) {
        try {
          navigator.mediaSession.setActionHandler('seekto', (details) => {
            if (handlers.seekto && details.seekTime !== undefined) {
              handlers.seekto({ seekTime: details.seekTime })
            }
          })
        } catch {
          // Media Session action not supported
        }
      }
    }
  }

  /**
   * 更新播放状态
   */
  setPlaybackState(state: 'none' | 'paused' | 'playing'): void {
    if ('mediaSession' in navigator) {
      navigator.mediaSession.playbackState = state
    }
  }

  /**
   * 更新 Media Session 位置状态（移动端后台播放关键）
   * 需要定期调用以保持系统媒体控制的同步
   */
  updatePositionState(
    duration: number,
    position: number,
    playbackRate: number = 1,
  ): void {
    if (
      'mediaSession' in navigator &&
      navigator.mediaSession.setPositionState
    ) {
      try {
        // 确保参数有效
        if (duration > 0 && position >= 0 && position <= duration) {
          navigator.mediaSession.setPositionState({
            duration,
            playbackRate,
            position,
          })
        }
      } catch {
        // Position state update not supported or invalid parameters
      }
    }
  }

  /**
   * 恢复 AudioContext（移动端后台播放时可能被暂停）
   * 当页面恢复可见时调用
   */
  async resumeAudioContext(): Promise<void> {
    if (this.audioContext && this.audioContext.state === 'suspended') {
      try {
        await this.audioContext.resume()
      } catch {
        // AudioContext resume failed
      }
    }
  }

  /**
   * 初始化 Web Audio API 用于频谱分析
   * 注意：由于 CORS 限制，跨域音频无法进行频谱分析
   */
  private initAudioContext(): boolean {
    if (this.audioContext) return true

    try {
      this.audioContext = new (
        window.AudioContext ||
        (window as unknown as { webkitAudioContext: typeof AudioContext })
          .webkitAudioContext
      )()
      this.analyser = this.audioContext.createAnalyser()

      // 配置分析器 - 使用较小的 FFT 以获得更快的响应
      this.analyser.fftSize = 64 // 32 个频段
      this.analyser.smoothingTimeConstant = 0.6 // 平滑系数，0-1
      this.analyser.minDecibels = -90
      this.analyser.maxDecibels = -10

      // 连接到音频输出
      this.analyser.connect(this.audioContext.destination)

      // 初始化频率数据数组
      this.frequencyData = new Uint8Array(this.analyser.frequencyBinCount)

      return true
    } catch (e) {
      console.warn(
        'Failed to initialize AudioContext for spectrum analysis:',
        e,
      )
      return false
    }
  }

  /**
   * 连接音频元素到分析器。
   *
   * ⚠️ 移动端默认拒绝接入：MediaElementAudioSourceNode 会劫持原生输出，
   * AudioContext 在页面后台被 suspend 后音乐无法继续，表现为「不能后台播放」。
   * 桌面端可安全使用实时频谱；移动端 media.getSpectrum 返回静默（0）。
   */
  connectAudioToAnalyser(audio: HTMLAudioElement): boolean {
    if (this.nativeOutputLocked || shouldPreserveNativeAudioOutput()) {
      this.nativeOutputLocked = true
      return false
    }

    if (!this.initAudioContext() || !this.audioContext || !this.analyser) {
      return false
    }

    // 如果已经连接了相同的音频元素，跳过
    if (this.connectedAudio === audio && this.sourceNode) {
      return true
    }

    try {
      // 创建新的源节点
      // 注意：每个音频元素只能创建一次 MediaElementAudioSourceNode
      this.sourceNode = this.audioContext.createMediaElementSource(audio)
      this.sourceNode.connect(this.analyser)
      this.connectedAudio = audio

      return true
    } catch (e) {
      // 如果音频元素已经被连接过，会抛出错误
      // 这种情况下频谱分析可能仍然可用
      console.warn(
        'Failed to connect audio to analyser (may already be connected):',
        e,
      )
      return false
    }
  }

  /**
   * 获取当前频谱数据
   * 返回一个包含 4 个值的数组，索引对应：[bar1, bar2, bar3, bar4]
   * 优化策略：中间的bar2和bar3显示最高的频率点，形成视觉中心
   * 性能优化：
   *   - 复用所有数组，零GC压力
   *   - 使用选择算法O(n)代替排序O(n log n)
   *   - 避免创建临时对象
   */
  private spectrumResult: number[] = [0, 0, 0, 0] // 复用数组
  private lastSpectrumTime = 0
  private readonly SPECTRUM_THROTTLE = 50 // 节流间隔 ms
  private tempBands: number[] = [0, 0, 0, 0, 0, 0, 0, 0] // 临时存储8个频段数据
  private bandIndices: number[] = [0, 1, 2, 3, 4, 5, 6, 7] // 复用索引数组，避免创建对象

  getSpectrumData(): number[] {
    // 节流：避免过于频繁的计算
    const now = performance.now()
    if (now - this.lastSpectrumTime < this.SPECTRUM_THROTTLE) {
      return this.spectrumResult
    }
    this.lastSpectrumTime = now

    if (!this.analyser || !this.frequencyData) {
      // 移除随机频响后退方案：无分析器时返回静默
      this.spectrumResult.fill(0)
      return this.spectrumResult
    }

    try {
      // 获取频率数据
      this.analyser.getByteFrequencyData(
        this.frequencyData as Uint8Array<ArrayBuffer>,
      )

      // 将32个频段分成8个区域，每个区域4个bin，获得更精细的频率分布
      const binCount = this.frequencyData.length // 32 个频段
      const bandSize = binCount >> 3 // 除以8 = 4个bin per band

      let hasData = false

      // 计算8个频段的平均值
      for (let i = 0; i < 8; i++) {
        let sum = 0
        const start = i * bandSize
        const end = start + bandSize

        for (let j = start; j < end; j++) {
          sum += this.frequencyData[j]
        }

        // 归一化到 0-1 范围，应用 1.8x 增益（提高灵敏度）
        const value = Math.min(1, (sum / bandSize / 255) * 1.8)
        this.tempBands[i] = value
        if (value > 0.01) hasData = true // 降低阈值，检测更细微的声音
      }

      if (!hasData) {
        // 移除随机频响后退方案：无数据时返回静默
        this.spectrumResult.fill(0)
        return this.spectrumResult
      }

      // ⚡ 性能优化：使用选择算法找前4大的值，O(n)时间复杂度
      // 避免完整排序和创建临时对象

      // 使用部分选择排序：只需要找到前4大的值
      // 索引数组按值降序排列前4个元素
      const bands = this.tempBands
      const indices = this.bandIndices

      // 找到最大值的索引（第1大）
      let maxIdx = 0
      for (let i = 1; i < 8; i++) {
        if (bands[indices[i]] > bands[indices[maxIdx]]) {
          maxIdx = i
        }
      }
      // 交换到位置0
      if (maxIdx !== 0) {
        const temp = indices[0]
        indices[0] = indices[maxIdx]
        indices[maxIdx] = temp
      }

      // 找到第二大值的索引
      maxIdx = 1
      for (let i = 2; i < 8; i++) {
        if (bands[indices[i]] > bands[indices[maxIdx]]) {
          maxIdx = i
        }
      }
      // 交换到位置1
      if (maxIdx !== 1) {
        const temp = indices[1]
        indices[1] = indices[maxIdx]
        indices[maxIdx] = temp
      }

      // 找到第三大值的索引
      maxIdx = 2
      for (let i = 3; i < 8; i++) {
        if (bands[indices[i]] > bands[indices[maxIdx]]) {
          maxIdx = i
        }
      }
      // 交换到位置2
      if (maxIdx !== 2) {
        const temp = indices[2]
        indices[2] = indices[maxIdx]
        indices[maxIdx] = temp
      }

      // 找到第四大值的索引
      maxIdx = 3
      for (let i = 4; i < 8; i++) {
        if (bands[indices[i]] > bands[indices[maxIdx]]) {
          maxIdx = i
        }
      }
      // 交换到位置3
      if (maxIdx !== 3) {
        const temp = indices[3]
        indices[3] = indices[maxIdx]
        indices[maxIdx] = temp
      }

      // 分配策略：
      // - bar2 (中间左): 最高频段
      // - bar3 (中间右): 次高频段
      // - bar1 (左边): 第三高频段
      // - bar4 (右边): 第四高频段
      // 形成 "低-高-高-低" 的对称视觉效果

      this.spectrumResult[1] = bands[indices[0]] // bar2: 最高
      this.spectrumResult[2] = bands[indices[1]] // bar3: 次高
      this.spectrumResult[0] = bands[indices[2]] // bar1: 第三
      this.spectrumResult[3] = bands[indices[3]] // bar4: 第四

      return this.spectrumResult
    } catch {
      // 移除随机频响后退方案：异常时返回静默
      this.spectrumResult.fill(0)
      return this.spectrumResult
    }
  }

  /**
   * 获取原始 8 频段数据（bass→high 自然顺序，0-1 归一化）
   * 与 getSpectrumData 不同：后者是为 4 根柱视觉重排过的（低-高-高-低），
   * 不适合做频谱可视化；此方法返回未重排的频段，供 Tapp 可视化使用
   */
  private bandsResult: number[] = [0, 0, 0, 0, 0, 0, 0, 0]

  getSpectrumBands(): number[] {
    // 刷新 tempBands（内部自带 50ms 节流）
    this.getSpectrumData()
    if (!this.analyser || !this.frequencyData) {
      this.bandsResult.fill(0)
      return this.bandsResult
    }
    for (let i = 0; i < 8; i++) {
      this.bandsResult[i] = this.tempBands[i]
    }
    return this.bandsResult
  }

  /**
   * 检查是否支持频谱分析
   */
  isSpectrumSupported(): boolean {
    return !!(
      window.AudioContext ||
      (window as unknown as { webkitAudioContext: typeof AudioContext })
        .webkitAudioContext
    )
  }
}

/**
 * 导出全局音频管理器实例
 */
export const audioManager = GlobalAudioManager.getInstance()
