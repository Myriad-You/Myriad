import type { MutableRefObject, RefObject } from 'react'
import type { ColorPalette } from '../../utils/colorExtractor'
import type { LyricLine, Song, WordLyricLine } from '../../utils/musicPlayer'
import type { MusicColors, PlayMode, TempPlayMode } from './types'

import { useCallback, useEffect, useRef, useState } from 'react'
import {
  extractColorsFromImage,
  extractColorsFromLoadedImage,
  getCachedPalette,
  isDefaultPalette,
  setCachedPalette,
} from '../../utils/colorExtractor'
import {
  readLiveAudioProgress,
  resolveMusicPalette,
} from '../../utils/musicPlayerState'
import { getGlobalState, publishMusicPlayerSnapshot } from './globalState'

export function warmCoverImage(
  cover: string,
  fetchPriority?: 'high',
): void {
  try {
    const img = new Image()
    img.decoding = 'async'
    if (fetchPriority === 'high') img.fetchPriority = 'high'
    img.referrerPolicy = 'no-referrer'
    img.src = cover
  } catch {
    /* ignore */
  }
}

export interface CoverColorsApi {
  musicColors: MusicColors | null
  musicColorsRef: MutableRefObject<MusicColors | null>
  colorCacheRef: MutableRefObject<Map<string, MusicColors>>
  peekCoverColors: (cover: string) => MusicColors | null
  isUsableCoverPalette: (colors: MusicColors | null | undefined) => boolean
  rememberCoverColors: (cover: string, colors: MusicColors) => void
  extractCoverColorsForSong: (
    song: Song,
    index: number,
    generation: number,
    settleAttempt?: number,
  ) => void
  prefetchAroundIndex: (center: number) => void
  pushSongTheme: (
    song: Song,
    index: number,
    colors: MusicColors | null,
    playing: boolean,
    options?: { resetProgress?: boolean },
  ) => void
}

export function useCoverColors(options: {
  musicContainerRef: RefObject<HTMLDivElement | null>
  audioRef: RefObject<HTMLAudioElement | null>
  playlistRef: MutableRefObject<Song[]>
  currentSongRef: MutableRefObject<Song | null>
  currentSongIndexRef: MutableRefObject<number>
  selectGenerationRef: MutableRefObject<number>
  tempPlayModeRef: MutableRefObject<TempPlayMode>
  musicEnabled: boolean
  volume: number
  playMode: PlayMode
}): CoverColorsApi {
  const {
    musicContainerRef,
    audioRef,
    playlistRef,
    currentSongRef,
    currentSongIndexRef,
    selectGenerationRef,
    tempPlayModeRef,
    musicEnabled,
    volume,
    playMode,
  } = options

  const [musicColors, setMusicColors] = useState<MusicColors | null>(null)
  const musicColorsRef = useRef<MusicColors | null>(null)
  musicColorsRef.current = musicColors
  const colorCacheRef = useRef<Map<string, MusicColors>>(new Map())

  const normalizeColor = useCallback((color: string): string => {
    const cleaned = color.trim().replaceAll(/\s+/g, '')
    if (/^#([0-9A-F]{3}){1,2}$/i.test(cleaned)) {
      return cleaned.toLowerCase()
    }
    console.warn(`Invalid color format: "${color}", using fallback`)
    return '#999999'
  }, [])

  /** 同步写 --music-*（取色完成当帧生效）。useEffect 仍作严格模式双写兜底。 */
  const applyMusicCssVars = useCallback(
    (colors: MusicColors) => {
      const root = document.documentElement
      root.style.setProperty('--music-primary', normalizeColor(colors.primary))
      root.style.setProperty(
        '--music-secondary',
        normalizeColor(colors.secondary),
      )
      root.style.setProperty('--music-accent', normalizeColor(colors.accent))
      root.style.setProperty('--music-light', normalizeColor(colors.light))
      root.style.setProperty('--music-dark', normalizeColor(colors.dark))
    },
    [normalizeColor],
  )

  useEffect(() => {
    if (!musicColors) return
    applyMusicCssVars(musicColors)
  }, [musicColors, applyMusicCssVars])

  /** 立即写入全局态并推给 Tapp。colors=null 保留上一首主题色，不刷默认红/灰。 */
  const pushSongTheme = useCallback(
    (
      song: Song,
      index: number,
      colors: MusicColors | null,
      playing: boolean,
      themeOptions?: { resetProgress?: boolean },
    ) => {
      const resetProgress = themeOptions?.resetProgress ?? false

      const g = getGlobalState()
      const prevColors =
        (g?.musicColors as MusicColors | null | undefined) ??
        musicColorsRef.current ??
        null
      const resolvedColors = resolveMusicPalette(colors, prevColors)

      if (colors) {
        applyMusicCssVars(colors)
        setMusicColors(colors)
        musicColorsRef.current = colors
      }

      const live = resetProgress
        ? { currentTime: 0, audioDuration: 0 }
        : readLiveAudioProgress(audioRef.current)

      publishMusicPlayerSnapshot({
        song,
        index,
        colors: resolvedColors,
        isPlaying: playing,
        isEnabled: musicEnabled,
        volume,
        playMode,
        playlist: playlistRef.current,
        isTempPlay: tempPlayModeRef.current.enabled,
        resetProgress,
        liveCurrentTime: live.currentTime,
        liveDuration: live.audioDuration,
        lyrics: resetProgress ? [] : ((g?.lyrics as LyricLine[]) ?? []),
        verbatimLyrics: resetProgress
          ? []
          : ((g?.verbatimLyrics as WordLyricLine[]) ?? []),
        hasVerbatimLyrics: resetProgress
          ? false
          : Boolean(g?.hasVerbatimLyrics),
        verbatimLyricsSource: resetProgress
          ? ''
          : String(g?.verbatimLyricsSource || ''),
        currentLyricIndex: resetProgress
          ? -1
          : typeof g?.currentLyricIndex === 'number'
            ? g.currentLyricIndex
            : -1,
        generation: selectGenerationRef.current,
        isLoading: resetProgress
          ? true
          : Boolean(
              (g as { isAudioLoading?: boolean } | undefined)?.isAudioLoading,
            ),
      })
    },
    [
      musicEnabled,
      volume,
      playMode,
      applyMusicCssVars,
      audioRef,
      playlistRef,
      tempPlayModeRef,
      selectGenerationRef,
    ],
  )

  const peekCoverColors = useCallback((cover: string): MusicColors | null => {
    return (
      colorCacheRef.current.get(cover) ??
      (getCachedPalette(cover) as MusicColors | null)
    )
  }, [])

  const isUsableCoverPalette = useCallback(
    (colors: MusicColors | null | undefined) =>
      Boolean(colors && !isDefaultPalette(colors)),
    [],
  )

  const rememberCoverColors = useCallback(
    (cover: string, colors: MusicColors) => {
      if (isDefaultPalette(colors)) return
      if (colorCacheRef.current.size >= 50) {
        const firstKey = colorCacheRef.current.keys().next().value
        if (firstKey !== undefined) colorCacheRef.current.delete(firstKey)
      }
      colorCacheRef.current.set(cover, colors)
      setCachedPalette(cover, colors)
    },
    [],
  )

  /** 从已渲染封面 <img> 同步取色（零网络）；小尺寸 URL / 二次请求失败时的主兜底。 */
  const tryExtractFromDomCover = useCallback(
    (cover: string): MusicColors | null => {
      const root = musicContainerRef.current
      if (!root || !cover) return null
      const img = root.querySelector(
        '.music-album-cover-large img',
      ) as HTMLImageElement | null
      if (!img?.complete) return null
      if ((img.naturalWidth || 0) <= 2 || (img.naturalHeight || 0) <= 2) {
        return null
      }
      const src = img.currentSrc || img.src || ''
      if (!src) return null
      try {
        const resolvedCover = new URL(cover, window.location.href).href
        const resolvedSrc = new URL(src, window.location.href).href
        if (resolvedCover !== resolvedSrc) return null
      } catch {
        if (src !== cover && !src.includes(cover) && !cover.includes(src)) {
          return null
        }
      }
      const palette = extractColorsFromLoadedImage(img)
      if (isDefaultPalette(palette)) return null
      return palette as MusicColors
    },
    [musicContainerRef],
  )

  /** generation 过期或曲目已变则放弃。 */
  const extractCoverColorsForSong = useCallback(
    (
      song: Song,
      index: number,
      generation: number,
      settleAttempt: number = 0,
    ) => {
      if (!song.cover) return
      const cover = song.cover
      const isCurrent = () =>
        selectGenerationRef.current === generation &&
        currentSongRef.current?.id === song.id

      const applyIfCurrent = (colors: MusicColors) => {
        if (!isCurrent() || isDefaultPalette(colors)) return false
        rememberCoverColors(cover, colors)
        pushSongTheme(
          song,
          index,
          colors,
          !!(audioRef.current && !audioRef.current.paused),
          { resetProgress: false },
        )
        return true
      }

      const cached = peekCoverColors(cover)
      if (cached && !isDefaultPalette(cached)) {
        applyIfCurrent(cached)
        return
      }

      const fromDom = tryExtractFromDomCover(cover)
      if (fromDom) {
        applyIfCurrent(fromDom)
        return
      }

      const musicContainer = musicContainerRef.current
      if (settleAttempt === 0 && musicContainer) {
        musicContainer.classList.add('color-transitioning')
      }

      void extractColorsFromImage(cover, {
        context: 'music',
        priority: 'high',
        forceRefresh: settleAttempt > 0,
      })
        .then((palette: ColorPalette) => {
          if (!isCurrent()) return
          let colors = palette as MusicColors
          if (isDefaultPalette(colors)) {
            const domRetry = tryExtractFromDomCover(cover)
            if (domRetry) colors = domRetry
          }
          if (isDefaultPalette(colors)) {
            if (settleAttempt < 3) {
              const delay = 200 * 2 ** settleAttempt
              window.setTimeout(() => {
                if (!isCurrent()) return
                extractCoverColorsForSong(
                  song,
                  index,
                  generation,
                  settleAttempt + 1,
                )
              }, delay)
            }
            if (musicContainer) {
              musicContainer.classList.remove('color-transitioning')
            }
            return
          }
          applyIfCurrent(colors)
          if (musicContainer) {
            musicContainer.classList.remove('color-transitioning')
          }
        })
        .catch((error) => {
          if (!isCurrent()) return
          const msg = error instanceof Error ? error.message : String(error)
          const aborted =
            msg.includes('cancel') ||
            msg.includes('Abort') ||
            msg.includes('aborted')
          if (aborted) {
            if (musicContainer) {
              musicContainer.classList.remove('color-transitioning')
            }
            return
          }
          const domRetry = tryExtractFromDomCover(cover)
          if (domRetry && applyIfCurrent(domRetry)) {
            if (musicContainer) {
              musicContainer.classList.remove('color-transitioning')
            }
            return
          }
          if (settleAttempt < 3) {
            const delay = 200 * 2 ** settleAttempt
            window.setTimeout(() => {
              if (!isCurrent()) return
              extractCoverColorsForSong(
                song,
                index,
                generation,
                settleAttempt + 1,
              )
            }, delay)
          } else {
            console.warn('Failed to extract colors from cover:', error)
          }
          if (musicContainer) {
            musicContainer.classList.remove('color-transitioning')
          }
        })
    },
    [
      peekCoverColors,
      pushSongTheme,
      rememberCoverColors,
      tryExtractFromDomCover,
      audioRef,
      currentSongRef,
      musicContainerRef,
      selectGenerationRef,
    ],
  )

  const prefetchAroundIndex = useCallback(
    (center: number) => {
      const list = playlistRef.current
      if (!list.length) return
      const targets = [center - 1, center + 1, center + 2]
      for (const raw of targets) {
        if (raw < 0 || raw >= list.length || raw === center) continue
        const s = list[raw]
        if (!s?.cover) continue
        if (colorCacheRef.current.has(s.cover) || getCachedPalette(s.cover)) {
          continue
        }
        warmCoverImage(s.cover)
        void extractColorsFromImage(s.cover, {
          context: 'music',
          priority: 'low',
        })
          .then((palette) => {
            rememberCoverColors(s.cover, palette as MusicColors)
          })
          .catch(() => {})
      }
    },
    [playlistRef, rememberCoverColors],
  )

  useEffect(() => {
    const handleCoverLoaded = (e: Event) => {
      const detail = (e as CustomEvent<{ songId?: string; cover?: string }>)
        .detail
      const song = currentSongRef.current
      if (!song?.cover || !detail?.cover) return
      if (song.id !== detail.songId && song.cover !== detail.cover) return
      const hit =
        colorCacheRef.current.get(song.cover) ?? getCachedPalette(song.cover)
      if (hit && !isDefaultPalette(hit)) return
      extractCoverColorsForSong(
        song,
        currentSongIndexRef.current,
        selectGenerationRef.current,
        0,
      )
    }
    window.addEventListener('music-cover-loaded', handleCoverLoaded)
    return () => {
      window.removeEventListener('music-cover-loaded', handleCoverLoaded)
    }
  }, [extractCoverColorsForSong, currentSongRef, currentSongIndexRef, selectGenerationRef])

  useEffect(
    () => () => {
      colorCacheRef.current.clear()
    },
    [],
  )

  return {
    musicColors,
    musicColorsRef,
    colorCacheRef,
    peekCoverColors,
    isUsableCoverPalette,
    rememberCoverColors,
    extractCoverColorsForSong,
    prefetchAroundIndex,
    pushSongTheme,
  }
}
