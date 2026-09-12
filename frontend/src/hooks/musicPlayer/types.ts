import type { ReactNode, RefObject } from 'react'
import type {
  LyricLine,
  MusicSource,
  Song,
  VerbatimLyricsSource,
  WordLyricLine,
} from '../../utils/musicPlayer'
import type { MusicColorPalette } from '../../utils/musicPlayerState'

export type PlayMode = 'loop' | 'single' | 'shuffle'

export type MusicPlayerView = 'info' | 'lyrics' | 'playlist'

export interface TempPlayMode {
  enabled: boolean
  originalPlaylist: Song[]
  originalIndex: number
  originalSource: MusicSource
  originalPlaylistId: string
}

/** 与取色器 / 快照层 palette 同形，不另维护一份字段。 */
export type MusicColors = MusicColorPalette

export interface UseMusicPlayerReturn {
  playlist: Song[]
  currentSongIndex: number
  currentSong: Song | null
  isPlaying: boolean
  isAudioLoading: boolean
  currentTime: number
  audioDuration: number
  volume: number
  lyrics: LyricLine[]
  verbatimLyrics: WordLyricLine[]
  hasVerbatimLyrics: boolean
  verbatimLyricsSource: VerbatimLyricsSource
  currentLyricIndex: number
  musicEnabled: boolean
  musicSource: MusicSource
  playlistId: string
  musicErrorKey: string
  musicErrorDetail: string
  musicPlayerView: MusicPlayerView
  playMode: PlayMode
  musicColors: MusicColors | null

  playlistSearchQuery: string
  excludeVipSongs: boolean
  filteredPlaylist: Song[]

  isTempPlayMode: boolean

  togglePlay: () => Promise<void>
  playPrevious: () => void
  playNext: () => void
  handleSeek: (time: number) => void
  handleSeekStart: () => void
  handleSeekEnd: () => void
  handleVolumeChange: (volume: number) => void
  togglePlayMode: () => void
  selectSong: (song: Song, index: number, autoPlay?: boolean) => Promise<void>
  playSong: (song: Song) => void
  stopTempPlay: () => Promise<void>
  setMusicPlayerView: (view: MusicPlayerView) => void
  setPlaylistSearchQuery: (query: string) => void
  setExcludeVipSongs: (exclude: boolean) => void
  loadMusicConfig: () => Promise<void>

  audioRef: RefObject<HTMLAudioElement | null>
  playlistScrollRef: RefObject<HTMLDivElement | null>
  progressBarRef: RefObject<HTMLInputElement | null>
  musicContainerRef: RefObject<HTMLDivElement | null>
  volumeControlRef: RefObject<HTMLDivElement | null>

  showVolumePopup: boolean
  setShowVolumePopup: (show: boolean) => void

  getPlayModeInfo: () => {
    icon: ReactNode
    textKey: 'singleRepeat' | 'shuffle' | 'listRepeat'
  }

  /** 进度条不可见时 timeupdate 跳过 setCurrentTime；对外进度同步不受影响。 */
  setProgressUiVisible: (visible: boolean) => void
}
