import type { CSSProperties } from 'react'
import { memo, useEffect, useMemo, useRef, useState } from 'react'
import { useMusicLyricsSlice } from '../../contexts/MusicPlayerContext'
import { LyricWaveScroll } from '../shared/LyricWaveScroll'
import { LIBRARY_LIVE_MS } from './libraryLiveMs'

function injectLibraryStyle(id: string, css: string) {
  if (typeof document === 'undefined') return
  let style = document.getElementById(id) as HTMLStyleElement | null
  if (!style) {
    style = document.createElement('style')
    style.id = id
    document.head.appendChild(style)
  }
  style.textContent = css
}
injectLibraryStyle(
  'library-card-lyrics-styles',
  `
        /*
         * 资料库卡片歌词外壳（进出场 + 遮罩）
         * 行级波浪引擎见 shared/LyricWaveScroll
         */
        .library-card-lyrics {
            --music-color: #ef4444;
            position: absolute;
            inset: 0;
            z-index: 2;
            pointer-events: none;
            display: flex;
            align-items: flex-end;
            justify-content: center;
            padding: 0 0.5rem 0.45rem;
            opacity: 0;
            transform: translateY(6px) scale(0.985);
            filter: blur(0);
            transition:
                opacity 0.42s cubic-bezier(0.22, 1, 0.36, 1),
                transform 0.48s cubic-bezier(0.22, 1, 0.36, 1),
                filter 0.4s ease;
        }

        .library-card-lyrics.is-on {
            opacity: 1;
            transform: translateY(0) scale(1);
            filter: blur(0);
        }

        /* 播完/换歌退场：略下沉 + 微缩 + 轻糊，比硬淡出更顺 */
        .library-card-lyrics.is-leaving {
            opacity: 0;
            transform: translateY(8px) scale(0.97);
            filter: blur(1.2px);
            transition:
                opacity 0.48s cubic-bezier(0.33, 1, 0.68, 1),
                transform 0.52s cubic-bezier(0.33, 1, 0.68, 1),
                filter 0.42s cubic-bezier(0.4, 0, 0.2, 1);
        }

        /* 播放/退场中不藏歌词、不抢退场 */
        .group:not(.is-hover-locked):hover .library-card-lyrics.is-on {
            opacity: 0;
            transform: translateY(4px) scale(0.99);
            filter: blur(0.4px);
            transition-duration: 0.22s;
        }

        .library-card-lyrics__mask {
            position: absolute;
            inset: 0;
            border-radius: inherit;
            background:
                linear-gradient(
                    to top,
                    color-mix(
                        in srgb,
                        var(--music-color) 55%,
                        rgb(0 0 0 / 90%)
                    ) 0%,
                    color-mix(
                        in srgb,
                        var(--music-color) 38%,
                        rgb(0 0 0 / 72%)
                    ) 28%,
                    color-mix(
                        in srgb,
                        var(--music-color) 16%,
                        transparent
                    ) 55%,
                    transparent 78%
                );
            opacity: 0.95;
            transition:
                opacity 0.4s ease,
                background 0.45s ease;
        }

        .library-card-lyrics.is-leaving .library-card-lyrics__mask {
            opacity: 0;
            transition-duration: 0.45s;
        }

        @media (prefers-reduced-motion: reduce) {
            .library-card-lyrics,
            .library-card-lyrics.is-on,
            .library-card-lyrics.is-leaving {
                transition: opacity 0.15s ease;
                transform: none;
                filter: none;
            }
        }
  `,
)

/**
 * 父级轻量订阅：仅 songId / isPlaying / musicColor
 * 切句不触发 LibraryGrid 重渲染
 */
interface LibraryMusicIdentity {
  songId: string | null
  isPlaying: boolean
  musicColor: string
}

function readLibraryMusicIdentity(): LibraryMusicIdentity {
  if (typeof window === 'undefined') {
    return { songId: null, isPlaying: false, musicColor: '#ef4444' }
  }
  const g = (window as any).__musicPlayerState
  return {
    songId: g?.currentSong?.id != null ? String(g.currentSong.id) : null,
    isPlaying: Boolean(g?.isPlaying),
    musicColor: String(g?.musicColor || '#ef4444'),
  }
}

export function useLibraryMusicIdentity(): LibraryMusicIdentity {
  const [snap, setSnap] = useState(readLibraryMusicIdentity)
  useEffect(() => {
    // 事件只作通知：一律读 __musicPlayerState（宿主完整合并后的真相）。
    // 禁止从 detail 重建——embed 等路径会发 partial（仅 currentSong），
    // 缺字段会被当成 null/false/默认红，卡片「正在播」状态会假掉。
    const applyFromGlobal = () => {
      const next = readLibraryMusicIdentity()
      setSnap((prev) =>
        prev.songId === next.songId &&
        prev.isPlaying === next.isPlaying &&
        prev.musicColor === next.musicColor
          ? prev
          : next,
      )
    }
    window.addEventListener('music-player-state-change', applyFromGlobal)
    applyFromGlobal()
    return () =>
      window.removeEventListener('music-player-state-change', applyFromGlobal)
  }, [])
  return snap
}

/**
 * 资料库卡片歌词外壳：进出场 + 封面色遮罩；
 * 切换引擎见共享 LyricWaveScroll（与控制面板同一套）
 *
 * 退场时冻结歌词快照：换歌会 resetLyrics，不能靠 live hasLyrics 决定是否卸载，
 * 否则 is-leaving 会被短路硬切。
 */
export const LibraryCardLyrics = memo(
  ({
    active,
    musicColor,
  }: {
    /** true=当前曲（含暂停）；false=换歌离场 */
    active: boolean
    musicColor: string
  }) => {
    const { lyrics: liveLyrics, currentLyricIndex: liveIndex } =
      useMusicLyricsSlice()
    const hasLiveLyrics = useMemo(
      () => liveLyrics.some((l) => (l.text || '').trim()),
      [liveLyrics],
    )

    const [mounted, setMounted] = useState(false)
    const [visible, setVisible] = useState(false)
    /** 展示用（active 时跟随 live；leave 期间冻结） */
    const [displayLyrics, setDisplayLyrics] = useState(liveLyrics)
    const [displayIndex, setDisplayIndex] = useState(liveIndex)
    const leaveTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
    /** 是否曾成功进场；从未进场则不走 leave 计时（避免首帧无词误卸） */
    const everShownRef = useRef(false)

    // active 且有词：同步展示；leave 时不写，保留上一曲快照
    useEffect(() => {
      if (active && hasLiveLyrics) {
        setDisplayLyrics(liveLyrics)
        setDisplayIndex(liveIndex)
      }
    }, [active, hasLiveLyrics, liveLyrics, liveIndex])

    useEffect(() => {
      if (leaveTimerRef.current) {
        clearTimeout(leaveTimerRef.current)
        leaveTimerRef.current = null
      }
      if (active && hasLiveLyrics) {
        everShownRef.current = true
        setMounted(true)
        const raf = requestAnimationFrame(() => setVisible(true))
        return () => cancelAnimationFrame(raf)
      }
      // 仍是当前曲但歌词暂空（切歌 reset / 二次加载中）：只等词，绝不 leave
      // 否则 everShown 时会误走退场，把已挂载歌词卸掉，且二次加载失败时永久空白
      if (active) {
        if (!everShownRef.current) {
          setMounted(false)
          setVisible(false)
        }
        return
      }
      // 从未进场：保持未挂载
      if (!everShownRef.current) {
        setMounted(false)
        setVisible(false)
        return
      }
      // 非当前曲退场：冻结 display 快照；时长对齐 CSS is-leaving
      setVisible(false)
      leaveTimerRef.current = setTimeout(() => {
        setMounted(false)
        everShownRef.current = false
        leaveTimerRef.current = null
      }, LIBRARY_LIVE_MS.lyricsUnmount)
      return () => {
        if (leaveTimerRef.current) {
          clearTimeout(leaveTimerRef.current)
          leaveTimerRef.current = null
        }
      }
    }, [active, hasLiveLyrics])

    const hasDisplayLyrics = displayLyrics.some((l) => (l.text || '').trim())
    if (!mounted || !hasDisplayLyrics) return null

    return (
      <div
        className={`library-card-lyrics${visible ? ' is-on' : ' is-leaving'}`}
        style={{ '--music-color': musicColor } as CSSProperties}
        aria-hidden
      >
        <div className="library-card-lyrics__mask" />
        <LyricWaveScroll
          variant="card"
          lyrics={displayLyrics}
          currentLyricIndex={displayIndex}
          musicColor={musicColor}
        />
      </div>
    )
  },
)
