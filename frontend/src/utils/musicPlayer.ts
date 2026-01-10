/**
 * 音乐播放器 - 支持网易云音乐和QQ音乐歌单播放
 */

import { API_URL } from '../config'
import { isUserInChinaMainland } from './geoLocation'

/**
 * 获取网易云音乐音频URL
 * 根据用户地理位置决定是使用代理还是直连
 *
 * @param songId 歌曲ID
 * @param useProxy 是否强制使用代理（覆盖自动检测）
 * @returns 音频URL
 */
export async function getNeteaseAudioUrl(songId: string, useProxy?: boolean): Promise<string> {
  // 如果显式指定了是否使用代理
  if (useProxy !== undefined) {
    if (useProxy) {
      return `${API_URL}/api/proxy/music/netease/audio/${songId}`
    }
    else {
      // 直连网易云音乐API获取音频URL
      return `https://music.163.com/song/media/outer/url?id=${songId}.mp3`
    }
  }

  // 自动检测是否需要代理
  const inChina = await isUserInChinaMainland()

  if (inChina) {
    // 中国大陆用户：直连网易云音乐
    return `https://music.163.com/song/media/outer/url?id=${songId}.mp3`
  }
  else {
    // 海外用户：通过后端代理
    return `${API_URL}/api/proxy/music/netease/audio/${songId}`
  }
}

/**
 * 节流函数 - 限制函数执行频率
 * @param func 要节流的函数
 * @param wait 等待时间（毫秒）
 */
export function throttle<T extends (...args: any[]) => any>(
  func: T,
  wait: number,
): (...args: Parameters<T>) => void {
  let timeout: NodeJS.Timeout | null = null
  let previous = 0

  return function (this: any, ...args: Parameters<T>) {
    const now = Date.now()
    const remaining = wait - (now - previous)

    if (remaining <= 0 || remaining > wait) {
      if (timeout) {
        clearTimeout(timeout)
        timeout = null
      }
      previous = now
      func.apply(this, args)
    }
    else if (!timeout) {
      timeout = setTimeout(() => {
        previous = Date.now()
        timeout = null
        func.apply(this, args)
      }, remaining)
    }
  }
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
const PLAYLIST_STORAGE_KEY = 'myriad_playlist_cache'

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
      const milliseconds = match[3] ? Number.parseInt(match[3].padEnd(3, '0'), 10) : 0
      const text = match[4].trim()

      if (text) {
        lyrics.push({
          time: minutes * 60 + seconds + milliseconds / 1000,
          text,
        })
      }
    }
  }

  // 按时间排序
  return lyrics.sort((a, b) => a.time - b.time)
}

/**
 * 从缓存获取歌单
 */
function getPlaylistFromCache(cacheKey: string): Song[] | null {
  // 1. 先检查内存缓存
  const memoryCache = playlistMemoryCache.get(cacheKey)
  if (memoryCache && Date.now() - memoryCache.timestamp < PLAYLIST_CACHE_DURATION) {
    return memoryCache.data
  }

  // 2. 检查 SessionStorage
  try {
    const storageData = sessionStorage.getItem(PLAYLIST_STORAGE_KEY)
    if (storageData) {
      const allCache = JSON.parse(storageData) as Record<string, PlaylistCacheEntry>
      const cached = allCache[cacheKey]

      if (cached && Date.now() - cached.timestamp < PLAYLIST_CACHE_DURATION) {
        // 恢复到内存缓存
        playlistMemoryCache.set(cacheKey, cached)
        return cached.data
      }
    }
  }
  catch (error) {
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
  playlistMemoryCache.set(cacheKey, entry)

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

    // 限制缓存数量（最多5个歌单）
    const keys = Object.keys(allCache)
    if (keys.length > 5) {
      // 删除最旧的
      const oldestKey = keys.reduce((oldest, key) => {
        return allCache[key].timestamp < allCache[oldest].timestamp ? key : oldest
      }, keys[0])
      delete allCache[oldestKey]
    }

    sessionStorage.setItem(PLAYLIST_STORAGE_KEY, JSON.stringify(allCache))
  }
  catch (error) {
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
  }
  catch (error) {
    // 静默处理
  }
}

/**
 * 清空歌词缓存
 */
export function clearLyricsCache(): void {
  lyricsCache.clear()
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
    const response = await fetch(`${API_URL}/api/proxy/music/netease/playlist/${playlistId}`)

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
      }
      else if (data.code === -460 || data.code === -462) {
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
    console.log(`[MusicPlayer] 歌单加载完成，用户在中国大陆: ${inChina}，${inChina ? '使用直连' : '使用代理'}`)

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

      // 根据用户地理位置决定音频URL
      // 中国大陆用户：直连网易云（更快，无需代理）
      // 海外用户：通过后端代理（绕过地理限制）
      const audioUrl = inChina
        ? `https://music.163.com/song/media/outer/url?id=${track.id}.mp3`
        : `${API_URL}/api/proxy/music/netease/audio/${track.id}`

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
  }
  catch (error) {
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
    const response = await fetch(`${API_URL}/api/proxy/music/qq/playlist/${playlistId}`)

    if (!response.ok) {
      throw new Error('Failed to fetch playlist')
    }

    const data = await response.json()

    if (!data.cdlist || data.cdlist.length === 0) {
      throw new Error('Invalid playlist response')
    }

    const playlist = data.cdlist[0]
    const songlist = playlist.songlist || []

    const songs = songlist.map((song: any) => {
      // QQ音乐返回格式：singer(歌手数组), albumname(专辑名), interval(时长秒)
      const singers = Array.isArray(song.singer) ? song.singer : []

      return {
        id: song.songmid || song.id?.toString() || '',
        name: song.songname || song.name,
        artist: singers.length > 0 ? singers.map((s: any) => s.name).join(', ') : 'Unknown',
        album: song.albumname || song.album?.name || '',
        cover: song.albummid ? `https://y.gtimg.cn/music/photo_new/T002R300x300M000${song.albummid}.jpg` : '',
        url: `https://ws.stream.qqmusic.qq.com/${song.songmid}.m4a?fromtag=46`,
        duration: song.interval || 0,
        source: 'qq' as MusicSource,
      }
    })

    // 存入缓存
    savePlaylistToCache(cacheKey, songs)

    return songs
  }
  catch (error) {
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
    const response = await fetch(`${API_URL}/api/proxy/music/netease/lyrics/${songId}`)

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
  }
  catch (error) {
    console.error('Error fetching Netease lyrics:', error)
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
    const response = await fetch(`${API_URL}/api/proxy/music/qq/lyrics/${songId}`)

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
  }
  catch (error) {
    console.error('Error fetching QQ lyrics:', error)
    return []
  }
}

/**
 * 根据当前播放时间获取当前歌词索引
 * 重构版：精确匹配，正确处理所有边界情况
 */
export function getCurrentLyricIndex(lyrics: LyricLine[], currentTime: number): number {
  if (!lyrics || lyrics.length === 0)
    return -1

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
    }
    else {
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
export function getSongVipStatus(song: { isVip?: boolean, isTrial?: boolean }): {
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
    filtered = filtered.filter(song => !song.isVip)
  }
  if (options?.hideTrial) {
    filtered = filtered.filter(song => !song.isTrial)
  }

  // 根据搜索关键词过滤
  if (query && query.trim()) {
    const lowerQuery = query.toLowerCase().trim()
    filtered = filtered.filter(song =>
      song.name.toLowerCase().includes(lowerQuery)
      || song.artist.toLowerCase().includes(lowerQuery)
      || song.album.toLowerCase().includes(lowerQuery),
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
  if (!query || !query.trim()) {
    return text
  }

  const regex = new RegExp(`(${query.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')})`, 'gi')
  return text.replace(regex, '<mark>$1</mark>')
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
  setCurrentAudio(audio: HTMLAudioElement | null, song: Song | null = null): void {
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
        }
        catch {
          // Media Session action not supported
        }
      }

      if (handlers.pause) {
        try {
          navigator.mediaSession.setActionHandler('pause', handlers.pause)
        }
        catch {
          // Media Session action not supported
        }
      }

      // 设置上一首/下一首
      if (handlers.previoustrack) {
        try {
          navigator.mediaSession.setActionHandler('previoustrack', handlers.previoustrack)
        }
        catch {
          // Media Session action not supported
        }
      }

      if (handlers.nexttrack) {
        try {
          navigator.mediaSession.setActionHandler('nexttrack', handlers.nexttrack)
        }
        catch {
          // Media Session action not supported
        }
      }

      // 设置快进/快退
      if (handlers.seekbackward) {
        try {
          navigator.mediaSession.setActionHandler('seekbackward', handlers.seekbackward)
        }
        catch {
          // Media Session action not supported
        }
      }

      if (handlers.seekforward) {
        try {
          navigator.mediaSession.setActionHandler('seekforward', handlers.seekforward)
        }
        catch {
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
        }
        catch {
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
  updatePositionState(duration: number, position: number, playbackRate: number = 1): void {
    if ('mediaSession' in navigator && navigator.mediaSession.setPositionState) {
      try {
        // 确保参数有效
        if (duration > 0 && position >= 0 && position <= duration) {
          navigator.mediaSession.setPositionState({
            duration,
            playbackRate,
            position,
          })
        }
      }
      catch {
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
      }
      catch {
        // AudioContext resume failed
      }
    }
  }

  /**
   * 初始化 Web Audio API 用于频谱分析
   * 注意：由于 CORS 限制，跨域音频无法进行频谱分析
   */
  private initAudioContext(): boolean {
    if (this.audioContext)
      return true

    try {
      this.audioContext = new (window.AudioContext || (window as unknown as { webkitAudioContext: typeof AudioContext }).webkitAudioContext)()
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
    }
    catch (e) {
      console.warn('Failed to initialize AudioContext for spectrum analysis:', e)
      return false
    }
  }

  /**
   * 连接音频元素到分析器
   */
  connectAudioToAnalyser(audio: HTMLAudioElement): boolean {
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
    }
    catch (e) {
      // 如果音频元素已经被连接过，会抛出错误
      // 这种情况下频谱分析可能仍然可用
      console.warn('Failed to connect audio to analyser (may already be connected):', e)
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
      this.analyser.getByteFrequencyData(this.frequencyData as Uint8Array<ArrayBuffer>)

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
        if (value > 0.01)
          hasData = true // 降低阈值，检测更细微的声音
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
    }
    catch {
      // 移除随机频响后退方案：异常时返回静默
      this.spectrumResult.fill(0)
      return this.spectrumResult
    }
  }

  /**
   * 检查是否支持频谱分析
   */
  isSpectrumSupported(): boolean {
    return !!(window.AudioContext || (window as unknown as { webkitAudioContext: typeof AudioContext }).webkitAudioContext)
  }
}

/**
 * 导出全局音频管理器实例
 */
export const audioManager = GlobalAudioManager.getInstance()
