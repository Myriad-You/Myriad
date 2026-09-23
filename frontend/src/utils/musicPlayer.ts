import type { MotionAudioFeatures } from './audioMotionAnalysis'
import { API_URL } from '../config'
import { currentCopy, formatCurrent } from '../i18n/localeCopy'
import { analyzeMotionAudio } from './audioMotionAnalysis'
import { getCachedIsChinaMainland, isUserInChinaMainland } from './geoLocation'
import { shouldPreserveNativeAudioOutput } from './platformDetect'
import { proxyImageUrlOr } from './proxyImageUrl'

export { shouldPreserveNativeAudioOutput }

/** play-url CDN has no CORS; desktop Web Audio needs the full proxy. */
export function getNeteasePlayUrl(songId: string): string {
  return `${API_URL}/api/proxy/music/netease/play-url/${songId}`
}

export function getNeteaseProxyAudioUrl(songId: string): string {
  return `${API_URL}/api/proxy/music/netease/audio/${songId}`
}

let musicStreamProxyEnabled = true

/** Admin switch: off means play-url everywhere, no full-proxy fallback. */
export function setMusicStreamProxyEnabled(enabled: boolean): void {
  musicStreamProxyEnabled = enabled
}

export function isMusicStreamProxyEnabled(): boolean {
  return musicStreamProxyEnabled
}

export function prefersSameOriginMusicProxy(): boolean {
  return musicStreamProxyEnabled && !shouldPreserveNativeAudioOutput()
}

/** False for /audio/ to avoid fallback loops. */
export function isNeteaseDirectPlayUrl(url: string): boolean {
  if (!url) return false
  if (url.includes('/api/proxy/music/netease/audio/')) return false
  if (url.includes('/api/proxy/music/netease/play-url/')) return true
  // Treat leftover outer/CDN URLs as direct play-url.
  if (
    url.includes('music.126.net') ||
    url.includes('music.163.com/song/media') ||
    url.includes('music.163.com/song/media/outer')
  ) {
    return true
  }
  return false
}

/** play-url/CDN is unsafe for MediaElementAudioSource (CORS). */
export function isWebAudioUnsafeMediaUrl(url: string): boolean {
  if (!url) return false
  return isNeteaseDirectPlayUrl(url) || isQQDirectPlayUrl(url)
}

/** Desktop: upgrade play-url/CDN to same-origin proxy. */
export function ensureSpectrumSafePlaybackUrl(
  song: Pick<Song, 'id' | 'source' | 'url'>,
): string {
  if (!song?.url) return song?.url ?? ''
  if (!prefersSameOriginMusicProxy()) return song.url
  if (song.source === 'netease' && isNeteaseDirectPlayUrl(song.url)) {
    return getNeteaseProxyAudioUrl(song.id)
  }
  if (song.source === 'qq' && isQQDirectPlayUrl(song.url)) {
    return getQQProxyAudioUrl(song.id)
  }
  if (isNeteaseDirectPlayUrl(song.url) && song.id) {
    return getNeteaseProxyAudioUrl(song.id)
  }
  if (isQQDirectPlayUrl(song.url) && song.id) {
    return getQQProxyAudioUrl(song.id)
  }
  return song.url
}

export function withSpectrumSafePlaybackUrl<T extends Song>(song: T): T {
  const url = ensureSpectrumSafePlaybackUrl(song)
  return url === song.url ? song : { ...song, url }
}

/** CN mobile: play-url; desktop/overseas: full proxy (CORS). */
export function getNeteaseGeoPlaybackUrl(
  songId: string,
  inChina: boolean,
): string {
  if (!musicStreamProxyEnabled) return getNeteasePlayUrl(songId)
  if (!inChina) return getNeteaseProxyAudioUrl(songId)
  if (prefersSameOriginMusicProxy()) return getNeteaseProxyAudioUrl(songId)
  return getNeteasePlayUrl(songId)
}

/** Fallback full-proxy URL; null if already proxied. */
export function getNeteaseProxyFallbackUrl(
  song: Pick<Song, 'id' | 'source' | 'url'>,
): string | null {
  if (!musicStreamProxyEnabled) return null
  if (song.source !== 'netease') return null
  if (!isNeteaseDirectPlayUrl(song.url)) return null
  return getNeteaseProxyAudioUrl(song.id)
}

export function getNeteaseAudioUrlImmediate(songId: string): string {
  if (!musicStreamProxyEnabled) return getNeteasePlayUrl(songId)
  // createMediaElementSource cannot use play-url CDN.
  if (prefersSameOriginMusicProxy()) return getNeteaseProxyAudioUrl(songId)

  const cached = getCachedIsChinaMainland()
  if (cached === true) return getNeteasePlayUrl(songId)
  if (cached === false) return getNeteaseProxyAudioUrl(songId)
  // Uncached geo: do not await; full proxy so playback starts.
  void isUserInChinaMainland()
  return getNeteaseProxyAudioUrl(songId)
}

/** QQ CDN has no CORS; desktop needs the full proxy. */
export function getQQPlayUrl(songMid: string): string {
  return `${API_URL}/api/proxy/music/qq/play-url/${songMid}`
}

export function getQQProxyAudioUrl(songMid: string): string {
  return `${API_URL}/api/proxy/music/qq/audio/${songMid}`
}

export function isQQDirectPlayUrl(url: string): boolean {
  if (!url) return false
  if (url.includes('/api/proxy/music/qq/audio/')) return false
  if (url.includes('/api/proxy/music/qq/play-url/')) return true
  if (
    url.includes('stream.qqmusic.qq.com') ||
    url.includes('dl.stream.qqmusic.qq.com') ||
    url.includes('qqmusic.qq.com/')
  ) {
    return true
  }
  return false
}

export function getQQGeoPlaybackUrl(songMid: string, inChina: boolean): string {
  if (!musicStreamProxyEnabled) return getQQPlayUrl(songMid)
  if (!inChina) return getQQProxyAudioUrl(songMid)
  if (prefersSameOriginMusicProxy()) return getQQProxyAudioUrl(songMid)
  return getQQPlayUrl(songMid)
}

export function getQQProxyFallbackUrl(
  song: Pick<Song, 'id' | 'source' | 'url'>,
): string | null {
  if (!musicStreamProxyEnabled) return null
  if (song.source !== 'qq') return null
  if (!isQQDirectPlayUrl(song.url)) return null
  return getQQProxyAudioUrl(song.id)
}

export function getMusicProxyFallbackUrl(
  song: Pick<Song, 'id' | 'source' | 'url'>,
): string | null {
  if (song.source === 'netease') return getNeteaseProxyFallbackUrl(song)
  if (song.source === 'qq') return getQQProxyFallbackUrl(song)
  return null
}

export function getQQAudioUrlImmediate(songMid: string): string {
  if (!musicStreamProxyEnabled) return getQQPlayUrl(songMid)
  if (prefersSameOriginMusicProxy()) return getQQProxyAudioUrl(songMid)

  const cached = getCachedIsChinaMainland()
  if (cached === true) return getQQPlayUrl(songMid)
  if (cached === false) return getQQProxyAudioUrl(songMid)
  void isUserInChinaMainland()
  return getQQProxyAudioUrl(songMid)
}

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
      func.call(this, ...args)
    } else if (!timeout) {
      timeout = setTimeout(() => {
        previous = Date.now()
        timeout = null
        func.call(this, ...args)
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

export type MusicSource = 'netease' | 'qq'

export interface Song {
  id: string
  name: string
  artist: string
  album: string
  cover: string
  url: string
  duration: number
  source: MusicSource
  isVip?: boolean
  isTrial?: boolean
  trialDuration?: number
}

export interface LyricLine {
  time: number
  text: string
  translation?: string
}

export interface WordLyricToken {
  time: number
  duration: number
  text: string
}

export interface WordLyricLine {
  time: number
  duration: number
  text: string
  words: WordLyricToken[]
  translation?: string
}

export interface VerbatimLyricsResult {
  lines: LyricLine[]
  verbatim: WordLyricLine[]
  translation: LyricLine[]
}

export type VerbatimLyricsSource = 'netease' | 'kugou' | ''

export interface LyricsWithVerbatimResult extends VerbatimLyricsResult {
  source: MusicSource
  hasVerbatim: boolean
  verbatimSource: VerbatimLyricsSource
  hasTranslation: boolean
  translationLang: 'zh' | ''
}

// Lyrics LRU cap 100.
const lyricsCache = new Map<string, LyricLine[]>()
const MAX_LYRICS_CACHE_SIZE = 100

function addToLyricsCache(key: string, lyrics: LyricLine[]): void {
  if (lyricsCache.has(key)) {
    lyricsCache.delete(key)
  }

  if (lyricsCache.size >= MAX_LYRICS_CACHE_SIZE) {
    const firstKey = lyricsCache.keys().next().value
    if (firstKey) {
      lyricsCache.delete(firstKey)
    }
  }

  lyricsCache.set(key, lyrics)
}

/** Session cache stores the player view without geo-specific `url`. */
type CachedPlayerSong = Omit<Song, 'url'>

export interface PlayerPlaylistSong {
  id: string
  name: string
  artist: string
  album: string
  cover: string
  duration: number
  isVip?: boolean
}

export interface PlayerPlaylistPayload {
  code: number
  source: MusicSource
  playlistId: string
  songs: PlayerPlaylistSong[]
}

interface PlaylistCacheEntry {
  data: CachedPlayerSong[]
  timestamp: number
}

const playlistMemoryCache = new Map<string, PlaylistCacheEntry>()
const PLAYLIST_CACHE_DURATION = 7 * 24 * 60 * 60 * 1000
// v4: metadata only; hydrate url on read.
const PLAYLIST_STORAGE_KEY = 'myriad_playlist_cache_v4'
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

const CREDIT_LINE_RE =
  /^(制作人|出品|监制|作词|作曲|编曲|歌词|翻译|混音|母带|录音|和声|吉他|贝斯|键盘|弦乐|[鼓词曲]|企划|统筹|发行|OP|SP|Produce[rd]?|Lyric(?:s|ist)?|Compose[rd]?|Arrange[rd]?|Mix(?:ing)?|Master(?:ing)?)\s*[:：]/i

export function parseLyrics(lrcText: string): LyricLine[] {
  const lines = lrcText.split('\n')
  const lyrics: LyricLine[] = []

  for (const line of lines) {
    const match = line.match(/\[(\d{2}):(\d{2})(?:\.(\d{2,3}))?\](.*)/)
    if (match) {
      const minutes = Number.parseInt(match[1], 10)
      const seconds = Number.parseInt(match[2], 10)
      const milliseconds = match[3]
        ? Number.parseInt(match[3].padEnd(3, '0'), 10)
        : 0
      const text = match[4].trim()
      const time = minutes * 60 + seconds + milliseconds / 1000

      if (text && !(time < 15 && CREDIT_LINE_RE.test(text))) {
        lyrics.push({ time, text })
      }
    }
  }

  return lyrics.toSorted((a, b) => a.time - b.time)
}

export function parseYrc(yrcText: string): WordLyricLine[] {
  if (!yrcText) return []

  const rawLines = yrcText.split('\n')
  const result: WordLyricLine[] = []
  const headerRe = /^\[(\d+),(\d+)\]/
  const wordRe = /\((\d+),(\d+),\d+\)([^(]*)/g

  for (const raw of rawLines) {
    const line = raw.trim()
    if (!line || line.startsWith('{')) continue

    const header = headerRe.exec(line)
    if (!header) continue

    const lineStart = Number(header[1]) / 1000
    const lineDuration = Number(header[2]) / 1000

    const words: WordLyricToken[] = []
    let text = ''
    wordRe.lastIndex = 0
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

  return result.toSorted((a, b) => a.time - b.time)
}

export function parseKrc(krcText: string): WordLyricLine[] {
  if (!krcText) return []

  const result: WordLyricLine[] = []
  const headerRe = /^\[(\d+),(\d+)\]/
  const wordRe = /<(\d+),(\d+),\d+>([^<]*)/g

  for (const raw of krcText.split('\n')) {
    const line = raw.trim()
    if (!line || !line.startsWith('[')) continue

    const header = headerRe.exec(line)
    if (!header) continue

    const lineStart = Number(header[1]) / 1000
    const lineDuration = Number(header[2]) / 1000

    const words: WordLyricToken[] = []
    let text = ''
    wordRe.lastIndex = 0
    let m: RegExpExecArray | null = wordRe.exec(line)
    while (m !== null) {
      const wordText = m[3]
      words.push({
        time: lineStart + Number(m[1]) / 1000,
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

  return result.toSorted((a, b) => a.time - b.time)
}

function playbackUrlForSong(
  source: MusicSource,
  id: string,
  inChina: boolean | null,
): string {
  if (source === 'netease') {
    if (inChina === null) return getNeteaseAudioUrlImmediate(id)
    return getNeteaseGeoPlaybackUrl(id, inChina)
  }
  if (inChina === null) return getQQAudioUrlImmediate(id)
  return getQQGeoPlaybackUrl(id, inChina)
}

function hydrateCachedSongs(
  songs: CachedPlayerSong[],
  inChina: boolean | null,
): Song[] {
  return songs.map((song) => ({
    ...song,
    cover: proxyImageUrlOr(song.cover),
    url: playbackUrlForSong(song.source, song.id, inChina),
  }))
}

export function songsFromPlayerPlaylist(
  data: PlayerPlaylistPayload,
  inChina: boolean,
): Song[] {
  const source = data.source
  return data.songs.map((song) => {
    const id = String(song.id)
    return {
      id,
      name: song.name || '',
      artist: song.artist || 'Unknown',
      album: song.album || '',
      cover: proxyImageUrlOr(song.cover || ''),
      url: playbackUrlForSong(source, id, inChina),
      duration: song.duration || 0,
      source,
      isVip: Boolean(song.isVip),
    } satisfies Song
  })
}

function stripPlaybackUrl(song: Song): CachedPlayerSong {
  const { url: _url, ...rest } = song
  return rest
}

function isPlayerPlaylistPayload(
  data: unknown,
  source: MusicSource,
): data is PlayerPlaylistPayload {
  if (!data || typeof data !== 'object') return false
  const payload = data as Record<string, unknown>
  return payload.source === source && Array.isArray(payload.songs)
}

function getPlaylistFromCache(cacheKey: string): Song[] | null {
  const memoryCache = playlistMemoryCache.get(cacheKey)
  if (
    memoryCache &&
    Date.now() - memoryCache.timestamp < PLAYLIST_CACHE_DURATION
  ) {
    setPlaylistMemoryCache(cacheKey, memoryCache)
    return hydrateCachedSongs(memoryCache.data, getCachedIsChinaMainland())
  }
  if (memoryCache) playlistMemoryCache.delete(cacheKey)

  try {
    const storageData = sessionStorage.getItem(PLAYLIST_STORAGE_KEY)
    if (storageData) {
      const allCache = JSON.parse(storageData) as Record<
        string,
        PlaylistCacheEntry
      >
      const cached = allCache[cacheKey]

      if (cached && Date.now() - cached.timestamp < PLAYLIST_CACHE_DURATION) {
        setPlaylistMemoryCache(cacheKey, cached)
        return hydrateCachedSongs(cached.data, getCachedIsChinaMainland())
      }
    }
  } catch {
  }

  return null
}

function savePlaylistToCache(cacheKey: string, songs: Song[]): void {
  const entry: PlaylistCacheEntry = {
    data: songs.map(stripPlaybackUrl),
    timestamp: Date.now(),
  }

  setPlaylistMemoryCache(cacheKey, entry)

  try {
    const storageData = sessionStorage.getItem(PLAYLIST_STORAGE_KEY)
    const allCache: Record<string, PlaylistCacheEntry> = storageData
      ? JSON.parse(storageData)
      : {}

    Object.keys(allCache).forEach((key) => {
      if (Date.now() - allCache[key].timestamp > PLAYLIST_CACHE_DURATION) {
        delete allCache[key]
      }
    })

    allCache[cacheKey] = entry

    const keys = Object.keys(allCache)
    if (keys.length > MAX_PLAYLIST_CACHE_SIZE) {
      const oldestKey = keys.reduce((oldest, key) => {
        return allCache[key].timestamp < allCache[oldest].timestamp
          ? key
          : oldest
      }, keys[0])
      delete allCache[oldestKey]
    }

    sessionStorage.setItem(PLAYLIST_STORAGE_KEY, JSON.stringify(allCache))
  } catch (error) {
    console.warn('Failed to save playlist to SessionStorage:', error)
  }
}

export function clearPlaylistCache(): void {
  playlistMemoryCache.clear()
  try {
    sessionStorage.removeItem(PLAYLIST_STORAGE_KEY)
  } catch {
  }
}

export function clearLyricsCache(): void {
  lyricsCache.clear()
  verbatimLyricsCache.clear()
  kugouVerbatimCache.clear()
}

async function fetchPlayerPlaylist(
  source: MusicSource,
  playlistId: string,
): Promise<PlayerPlaylistPayload> {
  const path =
    source === 'netease'
      ? `${API_URL}/api/proxy/music/netease/playlist/${playlistId}`
      : `${API_URL}/api/proxy/music/qq/playlist/${playlistId}`
  const response = await fetch(path)

  if (response.status === 429) {
    const body = await response.json().catch(() => null)
    const { notifyHttpRateLimit } = await import('./httpRateLimitToast')
    notifyHttpRateLimit(response, body)
    throw new Error('RATE_LIMITED')
  }

  if (!response.ok) {
    throw new Error('FETCH_FAILED')
  }

  const data: unknown = await response.json()
  if (!isPlayerPlaylistPayload(data, source)) {
    throw new Error('FETCH_FAILED')
  }
  return data
}

async function loadPlayerPlaylist(
  source: MusicSource,
  playlistId: string,
): Promise<Song[]> {
  const cacheKey = `${source}-${playlistId}`
  const cached = getPlaylistFromCache(cacheKey)
  if (cached) {
    return cached
  }

  const geoPromise = isUserInChinaMainland()
  const data = await fetchPlayerPlaylist(source, playlistId)
  if (data.songs.length === 0) {
    throw new Error('PLAYLIST_EMPTY')
  }

  const inChina = await geoPromise
  const useSameOriginProxy = prefersSameOriginMusicProxy()
  console.log(
    `[MusicPlayer] 歌单加载完成，用户在中国大陆: ${inChina}，${
      !isMusicStreamProxyEnabled()
        ? 'play-url 直连 CDN（代理已关闭）'
        : !inChina || useSameOriginProxy
        ? `全量代理${useSameOriginProxy && inChina ? '（桌面频谱 CORS）' : ''}`
        : 'play-url 直连 CDN'
    }`,
  )

  const songs = songsFromPlayerPlaylist(data, inChina)
  savePlaylistToCache(cacheKey, songs)
  return songs
}

export async function getNeteasePlaylist(playlistId: string): Promise<Song[]> {
  return loadPlayerPlaylist('netease', playlistId)
}

export async function getQQPlaylist(playlistId: string): Promise<Song[]> {
  return loadPlayerPlaylist('qq', playlistId)
}

const verbatimLyricsCache = new Map<string, VerbatimLyricsResult>()

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

    if (response.status === 429) {
      const { notifyHttpRateLimit } = await import('./httpRateLimitToast')
      notifyHttpRateLimit(response)
      return { lines: [], verbatim: [], translation: [] }
    }

    if (!response.ok) {
      throw new Error(
        formatCurrent(currentCopy().errors.lyricsFailed, {
          status: response.status,
        }),
      )
    }

    const data = await response.json()

    const lines: LyricLine[] = data.lrc?.lyric
      ? parseLyrics(data.lrc.lyric)
      : []
    const verbatim: WordLyricLine[] = data.yrc?.lyric
      ? parseYrc(data.yrc.lyric)
      : []

    const translationRaw = data.ytlrc?.lyric || data.tlyric?.lyric || ''
    const translation: LyricLine[] = translationRaw
      ? parseLyrics(translationRaw)
      : []
    attachLyricTranslation(lines, translation)
    attachLyricTranslation(verbatim, translation)

    const result: VerbatimLyricsResult = { lines, verbatim, translation }

    // LRU drop oldest; keep translation.
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

/** Match translation by nearest time, not index (ms skew). */
export function attachLyricTranslation(
  entries: Array<{ time: number; translation?: string }>,
  translation: LyricLine[],
  toleranceSec = 1.0,
): void {
  if (entries.length === 0 || translation.length === 0) return
  const claimed = new Map<number, number>()
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

export function alignVerbatimToLines(
  verbatim: WordLyricLine[],
  lines: LyricLine[],
): WordLyricLine[] | null {
  if (verbatim.length < 4 || lines.length < 4) return verbatim

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

  const sortedDiffs = diffs.toSorted((a, b) => a - b)
  const median = sortedDiffs[Math.floor(sortedDiffs.length / 2)]
  const residuals = sortedDiffs
    .map((d) => Math.abs(d - median))
    .toSorted((a, b) => a - b)
  const medResidual = residuals[Math.floor(residuals.length / 2)]

  if (medResidual > 1.2) return null
  if (Math.abs(median) < 0.08) return verbatim

  return verbatim.map((v) => ({
    ...v,
    time: Math.max(0, v.time - median),
    words: v.words.map((w) => ({ ...w, time: Math.max(0, w.time - median) })),
  }))
}

// Kugou cache key: keyword|durationSeconds.
const kugouVerbatimCache = new Map<string, WordLyricLine[]>()

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
    if (response.status === 429) {
      const { notifyHttpRateLimit } = await import('./httpRateLimitToast')
      notifyHttpRateLimit(response)
      return []
    }
    if (!response.ok) {
      throw new Error(
        formatCurrent(currentCopy().errors.lyricsFailed, {
          status: response.status,
        }),
      )
    }

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

export async function getQQLyricsWithTranslation(
  songId: string,
): Promise<{ lines: LyricLine[]; translation: LyricLine[] }> {
  const cacheKey = `qq-${songId}`

  if (lyricsCache.has(cacheKey)) {
    const lines = lyricsCache.get(cacheKey)!
    const translation: LyricLine[] = lines
      .filter((l) => typeof l.translation === 'string' && l.translation)
      .map((l) => ({ time: l.time, text: l.translation as string }))
    return { lines, translation }
  }

  try {
    const response = await fetch(
      `${API_URL}/api/proxy/music/qq/lyrics/${songId}`,
    )

    if (response.status === 429) {
      const { notifyHttpRateLimit } = await import('./httpRateLimitToast')
      notifyHttpRateLimit(response)
      return { lines: [], translation: [] }
    }

    if (!response.ok) {
      try {
        const errBody = await response.json()
        if (typeof errBody?.retcode === 'number' && errBody.retcode !== 0) {
          return { lines: [], translation: [] }
        }
      } catch {
        /* ignore body parse */
      }
      throw new Error(
        formatCurrent(currentCopy().errors.lyricsFailed, {
          status: response.status,
        }),
      )
    }

    const data = await response.json()

    const retcode =
      typeof data.retcode === 'number'
        ? data.retcode
        : typeof data.code === 'number'
          ? data.code
          : -1
    if (retcode !== 0 || !data.lyric) {
      return { lines: [], translation: [] }
    }

    const lyricText = unescapeQQLyricText(String(data.lyric))
    const transText = data.trans ? unescapeQQLyricText(String(data.trans)) : ''

    const lines = parseLyrics(lyricText)
    const translation = transText ? parseLyrics(transText) : []
    if (translation.length > 0) {
      attachLyricTranslation(lines, translation)
    }

    // Cache hits must keep translation.
    addToLyricsCache(cacheKey, lines)
    return { lines, translation }
  } catch (error) {
    console.error('Error fetching QQ lyrics:', error)
    return { lines: [], translation: [] }
  }
}

export function unescapeQQLyricText(s: string): string {
  return s
    .replaceAll('&apos;', "'")
    .replaceAll('&#39;', "'")
    .replaceAll('&quot;', '"')
    .replaceAll('&#34;', '"')
    .replaceAll('&amp;', '&')
    .replaceAll('&lt;', '<')
    .replaceAll('&gt;', '>')
    .replaceAll('&#10;', '\n')
    .replaceAll('&#13;', '\r')
}

export async function getLyricsWithVerbatim(
  song: Pick<Song, 'id' | 'source' | 'name' | 'artist' | 'duration'>,
): Promise<LyricsWithVerbatimResult> {
  let lines: LyricLine[] = []
  let verbatim: WordLyricLine[] = []
  let verbatimSource: VerbatimLyricsSource = ''
  let translation: LyricLine[] = []

  if (song.source === 'qq') {
    const qq = await getQQLyricsWithTranslation(song.id)
    lines = qq.lines
    translation = qq.translation
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

export function getCurrentLyricIndex(
  lyrics: LyricLine[],
  currentTime: number,
): number {
  if (!lyrics || lyrics.length === 0) return -1

  if (currentTime < lyrics[0].time) {
    return -1
  }

  let currentIndex = -1

  for (let i = 0; i < lyrics.length; i++) {
    if (lyrics[i].time <= currentTime) {
      currentIndex = i
    } else {
      break
    }
  }

  return currentIndex
}

export function formatTime(seconds: number): string {
  const mins = Math.floor(seconds / 60)
  const secs = Math.floor(seconds % 60)
  return `${mins.toString().padStart(2, '0')}:${secs.toString().padStart(2, '0')}`
}

export function isNeteaseVipFromMeta(meta: unknown): boolean {
  if (!meta || typeof meta !== 'object') return false
  const m = meta as Record<string, unknown>
  if (m.isVip === true || m.is_vip === true) return true
  const privilege =
    m.privilege && typeof m.privilege === 'object'
      ? (m.privilege as Record<string, unknown>)
      : null
  const feeRaw = m.fee ?? privilege?.fee
  const fee =
    typeof feeRaw === 'number'
      ? feeRaw
      : typeof feeRaw === 'string'
        ? Number(feeRaw)
        : NaN
  return fee === 1 || fee === 4
}

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

  const displayText = isVip ? 'VIP' : ''

  return { isVip, isTrial, displayText }
}

export function filterPlaylist(
  songs: Song[],
  query: string,
  options?: {
    hideVip?: boolean
    hideTrial?: boolean
  },
): Song[] {
  let filtered = songs

  if (options?.hideVip) {
    filtered = filtered.filter((song) => !song.isVip)
  }
  if (options?.hideTrial) {
    filtered = filtered.filter((song) => !song.isTrial)
  }

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

export function pickShuffleIndex(
  playlist: ReadonlyArray<Pick<Song, 'isVip'>>,
  currentIndex: number,
  excludeVip: boolean,
): number {
  if (playlist.length <= 1) return -1

  const availableSongs = playlist
    .map((song, idx) => ({ song, idx }))
    .filter((item) => (excludeVip ? !item.song.isVip : true))

  if (availableSongs.length === 0) return -1

  const availableOptions = availableSongs.filter(
    (item) => item.idx !== currentIndex,
  )
  if (availableOptions.length === 0) return availableSongs[0].idx

  const randomItem =
    availableOptions[Math.floor(Math.random() * availableOptions.length)]
  return randomItem.idx
}

export function pickAdjacentIndex(
  playlist: ReadonlyArray<Pick<Song, 'isVip'>>,
  fromIndex: number,
  direction: 1 | -1,
  excludeVip: boolean,
): number | null {
  const n = playlist.length
  if (n === 0) return null

  let newIndex =
    direction === 1
      ? (fromIndex + 1) % n
      : fromIndex === 0
        ? n - 1
        : fromIndex - 1

  if (!excludeVip) return newIndex

  let attempts = 0
  while (playlist[newIndex]?.isVip && attempts < n) {
    newIndex =
      direction === 1
        ? (newIndex + 1) % n
        : newIndex === 0
          ? n - 1
          : newIndex - 1
    attempts++
  }

  if (attempts >= n) return null
  return newIndex
}

export function clampSeekTime(time: number, duration: number): number {
  let safeTime = Math.max(0, time)
  if (duration > 0) {
    const maxSeekTime =
      duration > 1 ? duration - 1 : Math.max(0, duration * 0.95)
    safeTime = Math.min(safeTime, maxSeekTime)
  }
  return safeTime
}

export function escapeHtmlText(text: string): string {
  return text
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;')
    .replaceAll("'", '&#39;')
}

export function highlightText(text: string, query: string): string {
  // Escape before innerHTML.
  const escaped = escapeHtmlText(text)

  if (!query || !query.trim()) {
    return escaped
  }

  const regex = new RegExp(`(${RegExp.escape(query)})`, 'gi')
  return escaped.replaceAll(regex, '<mark>$1</mark>')
}

export function createPlaybackAudioElement(
  volume: number = 1,
): HTMLAudioElement {
  const audio = new Audio()
  audio.volume = volume
  audio.preload = 'auto'
  audio.setAttribute('playsinline', 'true')
  audio.setAttribute('webkit-playsinline', 'true')
  audio.setAttribute('data-myriad-audio', 'playback')
  // Keep in the document (hidden) for Media Session.
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

/** Preload Audio: no DOM; buffer next only. */
export function createPreloadAudioElement(
  volume: number = 1,
): HTMLAudioElement {
  const audio = new Audio()
  audio.preload = 'auto'
  audio.volume = volume
  return audio
}

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
  }
}

class GlobalAudioManager {
  private static instance: GlobalAudioManager
  private currentAudio: HTMLAudioElement | null = null
  private currentSong: Song | null = null

  private audioContext: AudioContext | null = null
  private analyser: AnalyserNode | null = null
  private sourceNode: MediaElementAudioSourceNode | null = null
  private connectedAudio: HTMLAudioElement | null = null
  private frequencyData: Uint8Array | null = null
  private motionAnalyser: AnalyserNode | null = null
  private motionWaveform = new Float32Array(2048)
  private motionSpectrum = new Float32Array(1024)
  private readonly motionFeatures: MotionAudioFeatures = {
    energy: 0,
    bass: 0,
    pulse: 0,
    presence: 0,
  }

  /** Do not reconnect AudioContext this session. */
  private nativeOutputLocked = false

  private constructor() {}

  static getInstance(): GlobalAudioManager {
    if (!GlobalAudioManager.instance) {
      GlobalAudioManager.instance = new GlobalAudioManager()
    }
    return GlobalAudioManager.instance
  }

  setCurrentAudio(
    audio: HTMLAudioElement | null,
    song: Song | null = null,
  ): void {
    if (this.currentAudio && this.currentAudio !== audio) {
      this.currentAudio.pause()
      this.currentAudio.src = ''
      this.currentAudio.load()
    }

    this.currentAudio = audio
    this.currentSong = song

    if (audio && song) {
      this.updateMediaSession(song)
    }
  }

  getCurrentAudio(): HTMLAudioElement | null {
    return this.currentAudio
  }

  getCurrentSong(): Song | null {
    return this.currentSong
  }

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

  private clearMediaSession(): void {
    if ('mediaSession' in navigator) {
      navigator.mediaSession.metadata = null
    }
  }

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
      if (handlers.play) {
        try {
          navigator.mediaSession.setActionHandler('play', handlers.play)
        } catch {
        }
      }

      if (handlers.pause) {
        try {
          navigator.mediaSession.setActionHandler('pause', handlers.pause)
        } catch {
        }
      }

      if (handlers.previoustrack) {
        try {
          navigator.mediaSession.setActionHandler(
            'previoustrack',
            handlers.previoustrack,
          )
        } catch {
        }
      }

      if (handlers.nexttrack) {
        try {
          navigator.mediaSession.setActionHandler(
            'nexttrack',
            handlers.nexttrack,
          )
        } catch {
        }
      }

      if (handlers.seekbackward) {
        try {
          navigator.mediaSession.setActionHandler(
            'seekbackward',
            handlers.seekbackward,
          )
        } catch {
        }
      }

      if (handlers.seekforward) {
        try {
          navigator.mediaSession.setActionHandler(
            'seekforward',
            handlers.seekforward,
          )
        } catch {
        }
      }

      if (handlers.seekto) {
        try {
          navigator.mediaSession.setActionHandler('seekto', (details) => {
            if (handlers.seekto && details.seekTime !== undefined) {
              handlers.seekto({ seekTime: details.seekTime })
            }
          })
        } catch {
        }
      }
    }
  }

  setPlaybackState(state: 'none' | 'paused' | 'playing'): void {
    if ('mediaSession' in navigator) {
      navigator.mediaSession.playbackState = state
    }
  }

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
        if (duration > 0 && position >= 0 && position <= duration) {
          navigator.mediaSession.setPositionState({
            duration,
            playbackRate,
            position,
          })
        }
      } catch {
      }
    }
  }

  async resumeAudioContext(): Promise<void> {
    if (this.audioContext?.state === 'suspended') {
      try {
        await this.audioContext.resume()
      } catch {
      }
    }
  }

  private initAudioContext(): boolean {
    if (this.audioContext) return true

    try {
      this.audioContext = new (
        window.AudioContext ||
        (window as unknown as { webkitAudioContext: typeof AudioContext })
          .webkitAudioContext
      )()
      this.analyser = this.audioContext.createAnalyser()

      this.analyser.fftSize = 64
      this.analyser.smoothingTimeConstant = 0.6
      this.analyser.minDecibels = -90
      this.analyser.maxDecibels = -10

      this.analyser.connect(this.audioContext.destination)

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

  /** Mobile: do not attach MediaElementAudioSource (breaks background audio). */
  connectAudioToAnalyser(audio: HTMLAudioElement): boolean {
    if (this.nativeOutputLocked || shouldPreserveNativeAudioOutput()) {
      this.nativeOutputLocked = true
      return false
    }

    // play-url/CDN has no CORS; connecting mutes or zeros the analyser.
    const mediaUrl = audio.currentSrc || audio.src || ''
    if (isWebAudioUnsafeMediaUrl(mediaUrl)) {
      return false
    }

    if (!this.initAudioContext() || !this.audioContext || !this.analyser) {
      return false
    }

    if (this.connectedAudio === audio && this.sourceNode) {
      return true
    }

    try {
      // One MediaElementAudioSourceNode per element.
      this.sourceNode = this.audioContext.createMediaElementSource(audio)
      this.sourceNode.connect(this.analyser)
      if (this.motionAnalyser) this.sourceNode.connect(this.motionAnalyser)
      this.connectedAudio = audio

      return true
    } catch (e) {
      console.warn(
        'Failed to connect audio to analyser (may already be connected):',
        e,
      )
      return false
    }
  }

  private spectrumResult: number[] = [0, 0, 0, 0]
  private lastSpectrumTime = 0
  private readonly SPECTRUM_THROTTLE = 50 // ms
  private tempBands: number[] = [0, 0, 0, 0, 0, 0, 0, 0]
  private bandIndices: number[] = [0, 1, 2, 3, 4, 5, 6, 7]

  getSpectrumData(): number[] {
    const now = performance.now()
    if (now - this.lastSpectrumTime < this.SPECTRUM_THROTTLE) {
      return this.spectrumResult
    }
    this.lastSpectrumTime = now

    // Resume AudioContext if Chromium suspends it.
    if (this.audioContext?.state === 'suspended') {
      void this.audioContext.resume().catch(() => {})
    }

    if (!this.analyser || !this.frequencyData) {
      // No analyser: silence; no fake spectrum.
      this.spectrumResult.fill(0)
      return this.spectrumResult
    }

    try {
      this.analyser.getByteFrequencyData(
        this.frequencyData as Uint8Array<ArrayBuffer>,
      )

      const binCount = this.frequencyData.length
      const bandSize = binCount >> 3

      let hasData = false

      for (let i = 0; i < 8; i++) {
        let sum = 0
        const start = i * bandSize
        const end = start + bandSize

        for (let j = start; j < end; j++) {
          sum += this.frequencyData[j]
        }

        const value = Math.min(1, (sum / bandSize / 255) * 1.8)
        this.tempBands[i] = value
        if (value > 0.01) hasData = true
      }

      if (!hasData) {
        // No data: silence; no fake spectrum.
        this.spectrumResult.fill(0)
        return this.spectrumResult
      }

      const bands = this.tempBands
      const indices = this.bandIndices

      let maxIdx = 0
      for (let i = 1; i < 8; i++) {
        if (bands[indices[i]] > bands[indices[maxIdx]]) {
          maxIdx = i
        }
      }
      if (maxIdx !== 0) {
        const temp = indices[0]
        indices[0] = indices[maxIdx]
        indices[maxIdx] = temp
      }

      maxIdx = 1
      for (let i = 2; i < 8; i++) {
        if (bands[indices[i]] > bands[indices[maxIdx]]) {
          maxIdx = i
        }
      }
      if (maxIdx !== 1) {
        const temp = indices[1]
        indices[1] = indices[maxIdx]
        indices[maxIdx] = temp
      }

      maxIdx = 2
      for (let i = 3; i < 8; i++) {
        if (bands[indices[i]] > bands[indices[maxIdx]]) {
          maxIdx = i
        }
      }
      if (maxIdx !== 2) {
        const temp = indices[2]
        indices[2] = indices[maxIdx]
        indices[maxIdx] = temp
      }

      maxIdx = 3
      for (let i = 4; i < 8; i++) {
        if (bands[indices[i]] > bands[indices[maxIdx]]) {
          maxIdx = i
        }
      }
      if (maxIdx !== 3) {
        const temp = indices[3]
        indices[3] = indices[maxIdx]
        indices[maxIdx] = temp
      }

      this.spectrumResult[1] = bands[indices[0]]
      this.spectrumResult[2] = bands[indices[1]]
      this.spectrumResult[0] = bands[indices[2]]
      this.spectrumResult[3] = bands[indices[3]]

      return this.spectrumResult
    } catch {
      // On error: silence; no fake spectrum.
      this.spectrumResult.fill(0)
      return this.spectrumResult
    }
  }

  private bandsResult: number[] = [0, 0, 0, 0, 0, 0, 0, 0]

  getSpectrumBands(): number[] {
    // 50ms throttle.
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

  /** Null = unavailable (CORS/native/suspended), not silence. Do not reuse visualizer bars. */
  getMotionAudioFeatures(
    audio: HTMLAudioElement,
  ): Readonly<MotionAudioFeatures> | null {
    if (
      this.connectedAudio !== audio ||
      !this.sourceNode ||
      !this.audioContext ||
      this.audioContext.state !== 'running' ||
      audio.muted ||
      audio.volume < 0.0001
    ) {
      return null
    }
    if (!this.motionAnalyser) {
      this.motionAnalyser = this.audioContext.createAnalyser()
      this.motionAnalyser.fftSize = 2048
      this.motionAnalyser.smoothingTimeConstant = 0
      this.sourceNode.connect(this.motionAnalyser)
    }
    this.motionAnalyser.getFloatTimeDomainData(this.motionWaveform)
    this.motionAnalyser.getFloatFrequencyData(this.motionSpectrum)
    return analyzeMotionAudio(
      this.motionWaveform,
      this.motionSpectrum,
      this.audioContext.sampleRate,
      this.motionFeatures,
      audio.volume,
    )
  }

  isSpectrumSupported(): boolean {
    return !!(
      window.AudioContext ||
      (window as unknown as { webkitAudioContext: typeof AudioContext })
        .webkitAudioContext
    )
  }
}

export const audioManager = GlobalAudioManager.getInstance()
