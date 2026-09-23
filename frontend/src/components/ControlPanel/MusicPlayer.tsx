import type { UseMusicPlayerReturn } from '../../hooks/useMusicPlayer'
import {
  LuAlertTriangle,
  LuListMusic,
  LuMusic,
  LuVolume2,
} from '@lib/chromeStrokeIcons'
import React, {
  memo,
  useEffect,
  useRef,
  useState,
} from 'react'
import { useI18n } from '../../contexts/I18nContext'
import { useAnimationLevel } from '../../hooks/useAnimationLevel'
import { emitAppEvent } from '../../utils/appEvents'
import { formatMusicError } from '../../utils/musicError'
import {
  formatTime,
  getSongVipStatus,
} from '../../utils/musicPlayer'
import { PlayingSpectrum } from '../shared/PlayingSpectrum'
import { FitText } from '../widgets/shared/FitText'
import { MusicLyricsView } from './MusicLyricsView'
import { MusicPlaylistView } from './MusicPlaylistView'
import '../MusicPlayer.css'

interface MusicPlayerProps {
  player: UseMusicPlayerReturn
  /**
   * 控制面板是否处于可交互展开态。
   * 收起时停频谱；歌词/列表 keep-alive（is-hidden），info 降频且封面不卸载。
   * 默认 true，便于单独挂载时行为不变。
   */
  panelVisible?: boolean
}

interface MusicInfoViewProps {
  player: UseMusicPlayerReturn
  /** false 时停频谱并跳过进度 tick 重渲染；整块 info DOM 保留（含封面）。 */
  visible: boolean
}

/**
 * 隐藏态相等比较：忽略 currentTime 等进度 tick，只在切歌/结构变化时更新
 */
function musicInfoHiddenEqual(
  prev: MusicInfoViewProps,
  next: MusicInfoViewProps,
): boolean {
  if (next.visible || prev.visible) return false
  const a = prev.player
  const b = next.player
  return (
    a.currentSong?.id === b.currentSong?.id &&
    a.currentSong?.cover === b.currentSong?.cover &&
    a.currentSong?.name === b.currentSong?.name &&
    a.currentSong?.artist === b.currentSong?.artist &&
    a.isPlaying === b.isPlaying &&
    a.isAudioLoading === b.isAudioLoading &&
    a.isTempPlayMode === b.isTempPlayMode &&
    a.playlist.length === b.playlist.length &&
    a.lyrics.length === b.lyrics.length &&
    a.musicErrorKey === b.musicErrorKey &&
    a.musicErrorDetail === b.musicErrorDetail &&
    a.volume === b.volume &&
    a.showVolumePopup === b.showVolumePopup &&
    a.playMode === b.playMode &&
    a.audioDuration === b.audioDuration
  )
}

/** 常驻 DOM；不可见时降频 */
const MusicInfoView = memo(({
  player,
  visible,
}: MusicInfoViewProps) => {
  const { t } = useI18n()
  const anim = useAnimationLevel()
  // 不可见时彻底关掉实时频谱 rAF
  const useSpectrum = visible && anim.level === 'standard'
  const {
    currentSong,
    isPlaying,
    isAudioLoading,
    currentTime,
    audioDuration,
    volume,
    lyrics,
    isTempPlayMode,
    playlist,
    togglePlay,
    playPrevious,
    playNext,
    handleSeek,
    handleSeekStart,
    handleSeekEnd,
    handleVolumeChange,
    togglePlayMode,
    stopTempPlay,
    setMusicPlayerView,
    getPlayModeInfo,
    progressBarRef,
    volumeControlRef,
    showVolumePopup,
    setShowVolumePopup,
    musicErrorKey,
    musicErrorDetail,
  } = player

  const musicError = musicErrorKey
    ? formatMusicError(
        (t.music as Record<string, string>)[musicErrorKey] || musicErrorKey,
        musicErrorDetail,
      )
    : ''

  const volumeBtnRef = useRef<HTMLButtonElement>(null)
  const volumeSliderRef = useRef<HTMLInputElement>(null)

  // 点击外部关闭音量弹层（逻辑必须挂在 MusicPlayer：state/ref 都在 player 里）
  useEffect(() => {
    if (!showVolumePopup) return
    const handlePointerDown = (event: MouseEvent | TouchEvent) => {
      const el = volumeControlRef.current
      const target = event.target as Node | null
      if (el && target && !el.contains(target)) {
        setShowVolumePopup(false)
      }
    }
    document.addEventListener('mousedown', handlePointerDown)
    document.addEventListener('touchstart', handlePointerDown, {
      passive: true,
    })
    return () => {
      document.removeEventListener('mousedown', handlePointerDown)
      document.removeEventListener('touchstart', handlePointerDown)
    }
  }, [showVolumePopup, setShowVolumePopup, volumeControlRef])

  useEffect(() => {
    if (!showVolumePopup) return
    const t = window.setTimeout(() => {
      volumeSliderRef.current?.focus({ preventScroll: true })
    }, 0)
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return
      event.preventDefault()
      event.stopPropagation()
      setShowVolumePopup(false)
      volumeBtnRef.current?.focus({ preventScroll: true })
    }
    document.addEventListener('keydown', handleKeyDown)
    return () => {
      window.clearTimeout(t)
      document.removeEventListener('keydown', handleKeyDown)
    }
  }, [showVolumePopup, setShowVolumePopup])

  // 离开默认页 / 面板不可见时收起弹层，避免残留
  useEffect(() => {
    if (!visible && showVolumePopup) {
      setShowVolumePopup(false)
    }
  }, [visible, showVolumePopup, setShowVolumePopup])

  // 加载圆点：active → settle(停呼吸) → exiting → hidden
  const [loadDotPhase, setLoadDotPhase] = useState<
    'hidden' | 'active' | 'settle' | 'exiting'
  >(() => (isAudioLoading ? 'active' : 'hidden'))
  /** settle/exit 锁定 left%，避免进度 tick 带动漂移 */
  const loadDotAtRef = useRef(0)
  const loadDotExitTimerRef = useRef<ReturnType<typeof setTimeout> | null>(
    null,
  )
  const loadDotRafRef = useRef(0)

  useEffect(() => {
    if (isAudioLoading) {
      if (loadDotExitTimerRef.current) {
        clearTimeout(loadDotExitTimerRef.current)
        loadDotExitTimerRef.current = null
      }
      if (loadDotRafRef.current) {
        cancelAnimationFrame(loadDotRafRef.current)
        loadDotRafRef.current = 0
      }
      setLoadDotPhase('active')
      return
    }
    setLoadDotPhase((prev) => {
      if (prev === 'hidden') return 'hidden'
      if (prev === 'exiting' || prev === 'settle') return prev
      return 'settle'
    })
  }, [isAudioLoading])

  // settle：停动画并冻结几何 → 下一帧再开 transition 融进
  useEffect(() => {
    if (loadDotPhase !== 'settle') return
    let raf2 = 0
    const raf1 = requestAnimationFrame(() => {
      raf2 = requestAnimationFrame(() => {
        setLoadDotPhase('exiting')
        loadDotRafRef.current = 0
      })
      loadDotRafRef.current = raf2
    })
    loadDotRafRef.current = raf1
    return () => {
      cancelAnimationFrame(raf1)
      if (raf2) cancelAnimationFrame(raf2)
    }
  }, [loadDotPhase])

  useEffect(() => {
    if (loadDotPhase !== 'exiting') return
    loadDotExitTimerRef.current = setTimeout(() => {
      setLoadDotPhase('hidden')
      loadDotExitTimerRef.current = null
    }, 400)
    return () => {
      if (loadDotExitTimerRef.current) {
        clearTimeout(loadDotExitTimerRef.current)
        loadDotExitTimerRef.current = null
      }
    }
  }, [loadDotPhase])

  if (!currentSong) {
    return (
      <div className="music-no-song">
        <div className="music-no-song-icon">
          {musicError ? <LuAlertTriangle size={20} /> : <LuMusic size={20} />}
        </div>
        <div className={`music-no-song-text ${musicError ? 'error' : ''}`}>
          {musicError ||
            (playlist.length === 0 ? t.music.noPlaylist : t.music.noPlaying)}
        </div>
      </div>
    )
  }

  const vipStatus = getSongVipStatus(currentSong)
  const playModeInfo = getPlayModeInfo()
  const totalDuration = audioDuration || currentSong.duration || 0
  // 未就绪时 max=1 避免 0；有时长时用真实值（短于 1s 的曲不能抬到 1）
  const rangeMax = totalDuration > 0 ? totalDuration : 1
  const progressPercent =
    totalDuration > 0
      ? Math.min(100, Math.max(0, (currentTime / totalDuration) * 100))
      : 0
  const spectrumLive = visible && isPlaying

  if (loadDotPhase === 'active') {
    loadDotAtRef.current = progressPercent
  }
  const loadDotLeft =
    loadDotPhase === 'active' ? progressPercent : loadDotAtRef.current

  return (
    <>
      <div
        className={`music-info-spectrum${spectrumLive ? ' is-playing' : ''}`}
        aria-hidden
      >
        <PlayingSpectrum
          themeColor="var(--music-base-primary, #ec4899)"
          scale={0.88}
          isPlaying={spectrumLive}
          useSpectrum={useSpectrum}
          variant="center"
        />
      </div>

      <div className="music-info-main">
        <div className="music-album-cover-large">
          {currentSong.cover ? (
            <img
              key={`${currentSong.id}:${currentSong.cover}`}
              src={currentSong.cover}
              alt={currentSong.name}
              decoding="async"
              referrerPolicy="no-referrer"
              draggable={false}
              onLoad={(e) => {
                // 切歌成功加载时恢复显示（避免上一次 onError 的 display:none 残留）
                e.currentTarget.style.display = ''
                e.currentTarget.nextElementSibling?.classList.add('hidden')
                // 通知 hook：显示图已解码，可走 DOM 同步取色（比二次请求稳）
                if (currentSong.cover) {
                  emitAppEvent('music-cover-loaded', {
                        songId: currentSong.id,
                        cover: currentSong.cover,
                      })
                }
              }}
              onError={(e) => {
                e.currentTarget.style.display = 'none'
                e.currentTarget.nextElementSibling?.classList.remove('hidden')
              }}
            />
          ) : null}
          <div
            className={`music-cover-placeholder ${currentSong.cover ? 'hidden' : ''}`}
          >
            <svg width="24" height="24" fill="currentColor" viewBox="0 0 24 24">
              <path d="M12 3v10.55c-.59-.34-1.27-.55-2-.55-2.21 0-4 1.79-4 4s1.79 4 4 4 4-1.79 4-4V7h4V3h-6z" />
            </svg>
          </div>
        </div>

        <div className="music-info-right">
          <div className="music-song-info">
            <div className="music-song-name-row">
              <FitText
                as="div"
                className="music-song-name"
                max={14}
                min={14}
                maxLines={1}
                marquee
                enabled={visible}
                title={currentSong.name}
              >
                {currentSong.name}
              </FitText>
              {vipStatus.displayText && (
                <span
                  className={`music-vip-badge ${vipStatus.isTrial ? 'trial' : ''}`}
                >
                  {vipStatus.displayText}
                </span>
              )}
            </div>
            <div className="music-song-artist">{currentSong.artist}</div>
            {musicError ? (
              <div className="music-song-error" role="alert">
                {musicError}
              </div>
            ) : null}
          </div>

          <div className="music-progress-container">
            <span className="music-time">{formatTime(currentTime)}</span>
            {/* 自绘轨道保证已播/未播同高同轴；range 仅负责交互与圆点 */}
            <div
              className={`music-progress-track${
                loadDotPhase !== 'hidden' ? ' is-loading' : ''
              }`}
            >
              <div className="music-progress-rail" aria-hidden>
                <div
                  className="music-progress-fill"
                  style={{ width: `${progressPercent}%` }}
                />
              </div>
              {/* 相对 track 定位：left% 与 fill 宽度同系，圆心对准进度端点 */}
              {loadDotPhase !== 'hidden' && (
                <span
                  className={`music-progress-loading-dot${
                    loadDotPhase === 'settle'
                      ? ' is-settle'
                      : loadDotPhase === 'exiting'
                        ? ' is-exiting'
                        : ''
                  }`}
                  style={
                    {
                      '--load-at': `${loadDotLeft}%`,
                    } as React.CSSProperties
                  }
                  aria-hidden
                />
              )}
              <input
                ref={progressBarRef}
                type="range"
                min="0"
                max={rangeMax}
                value={Math.min(currentTime, rangeMax)}
                onMouseDown={handleSeekStart}
                onMouseUp={handleSeekEnd}
                onTouchStart={handleSeekStart}
                onTouchEnd={handleSeekEnd}
                onInput={(e) =>
                  handleSeek(Number.parseFloat(e.currentTarget.value))
                }
                className="music-progress-bar"
                aria-label={t.music.progress}
                aria-valuemin={0}
                aria-valuemax={rangeMax}
                aria-valuenow={Math.min(currentTime, rangeMax)}
                aria-busy={isAudioLoading || undefined}
              />
            </div>
            <span className="music-time music-time-remaining">
              {`-${formatTime(Math.max(0, totalDuration - currentTime))}`}
            </span>
          </div>
        </div>
      </div>

      <div className="music-control-row">
        <div className="music-view-switcher">
          {lyrics.length > 0 && (
            <button
              type="button"
              onClick={() => setMusicPlayerView('lyrics')}
              className="music-view-switch-btn"
              aria-label={t.music.lyrics}
              title={t.music.lyrics}
            >
              <svg
                className="music-ctrl-icon"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                strokeWidth={2}
                strokeLinecap="round"
                strokeLinejoin="round"
                aria-hidden
              >
                <path d="M22 17a2 2 0 0 1-2 2H6.828a2 2 0 0 0-1.414.586l-2.202 2.202A.71.71 0 0 1 2 21.286V5a2 2 0 0 1 2-2h16a2 2 0 0 1 2 2z" />
                <path d="M7 9h10" />
                <path d="M7 13h6" />
              </svg>
            </button>
          )}
          {!isTempPlayMode && (
            <button
              onClick={togglePlayMode}
              className="music-view-switch-btn"
              aria-label={t.music[playModeInfo.textKey]}
              title={t.music[playModeInfo.textKey]}
            >
              {playModeInfo.icon}
            </button>
          )}
        </div>

        <div className="music-control-buttons">
          <button
            onClick={playPrevious}
            className="music-control-btn"
            aria-label={t.music.previous}
          >
            <svg className="w-5 h-5" fill="currentColor" viewBox="0 0 24 24">
              <path d="M6 6h2v12H6zm3.5 6l8.5 6V6z" />
            </svg>
          </button>

          <button
            onClick={togglePlay}
            className="music-play-btn"
            aria-label={isPlaying ? t.music.pause : t.music.play}
          >
            {isPlaying ? (
              <svg className="w-6 h-6" fill="currentColor" viewBox="0 0 24 24">
                <path d="M6 4h4v16H6V4zm8 0h4v16h-4V4z" />
              </svg>
            ) : (
              <svg className="w-6 h-6" fill="currentColor" viewBox="0 0 24 24">
                <path d="M8 5v14l11-7z" />
              </svg>
            )}
          </button>

          <button
            onClick={playNext}
            className="music-control-btn"
            aria-label={t.music.next}
          >
            <svg className="w-5 h-5" fill="currentColor" viewBox="0 0 24 24">
              <path d="M6 18l8.5-6L6 6v12zM16 6v12h2V6h-2z" />
            </svg>
          </button>
        </div>

        <div className="music-view-switcher music-view-switcher-right">
          <div className="music-volume-control" ref={volumeControlRef}>
            <button
              ref={volumeBtnRef}
              type="button"
              onClick={() => setShowVolumePopup(!showVolumePopup)}
              className={`music-view-switch-btn music-volume-btn${
                showVolumePopup ? ' is-active' : ''
              }`}
              aria-label={t.music.volume}
              title={t.music.volume}
              aria-expanded={showVolumePopup}
              aria-haspopup="dialog"
            >
              <LuVolume2 className="music-ctrl-icon" strokeWidth={2} aria-hidden />
            </button>
            <div
              className={`music-volume-popup ${showVolumePopup ? 'visible' : ''}`}
              role="dialog"
              aria-label={t.music.volume}
              aria-hidden={!showVolumePopup}
            >
              <LuVolume2
                className="music-ctrl-icon music-volume-popup-icon"
                strokeWidth={2}
                aria-hidden
              />
              <div className="music-progress-track music-volume-track">
                <div className="music-progress-rail" aria-hidden>
                  <div
                    className="music-progress-fill"
                    style={{
                      width: `${Math.min(100, Math.max(0, volume * 100))}%`,
                    }}
                  />
                </div>
                <input
                  ref={volumeSliderRef}
                  type="range"
                  min="0"
                  max="1"
                  step="0.01"
                  value={volume}
                  onChange={(e) =>
                    handleVolumeChange(Number.parseFloat(e.target.value))
                  }
                  className="music-progress-bar music-volume-slider"
                  aria-label={t.music.volume}
                />
              </div>
            </div>
          </div>

          {isTempPlayMode ? (
            <button
              type="button"
              onClick={stopTempPlay}
              className="music-view-switch-btn music-temp-stop-btn"
              aria-label={t.music.stopTemp}
              title={t.music.stopTempAndRestore}
            >
              <svg className="w-5 h-5" fill="currentColor" viewBox="0 0 24 24">
                <path d="M19 6.41L17.59 5 12 10.59 6.41 5 5 6.41 10.59 12 5 17.59 6.41 19 12 13.41 17.59 19 19 17.59 13.41 12z" />
              </svg>
            </button>
          ) : (
            playlist.length > 0 && (
              <button
                type="button"
                onClick={() => setMusicPlayerView('playlist')}
                className="music-view-switch-btn"
                aria-label={t.music.playlist}
                title={t.music.playlist}
              >
                <LuListMusic
                  className="music-ctrl-icon"
                  strokeWidth={2}
                  aria-hidden
                />
              </button>
            )
          )}
        </div>
      </div>
    </>
  )
}, musicInfoHiddenEqual)

export const MusicPlayer: React.FC<MusicPlayerProps> = ({
  player,
  panelVisible = true,
}) => {
  const {
    musicEnabled,
    musicPlayerView,
    currentSong,
    lyrics,
    musicContainerRef,
    setMusicPlayerView,
  } = player
  const songId = currentSong?.id ?? null
  const lyricsLen = lyrics.length
  // 自动回退 timer 回调内读最新态，避免闭包过期
  const musicPlayerViewRef = useRef(musicPlayerView)
  const songIdRef = useRef(songId)
  const lyricsLenRef = useRef(lyricsLen)
  musicPlayerViewRef.current = musicPlayerView
  songIdRef.current = songId
  lyricsLenRef.current = lyricsLen

  // 歌词视图自动返回：无词时延迟回 info。
  // 勿依赖整个 player 对象（每帧新引用会重置 timer，永远不回退）
  useEffect(() => {
    if (!panelVisible) return
    if (musicPlayerView !== 'lyrics') return
    if (songId && lyricsLen > 0) return

    const timer = setTimeout(() => {
      // 到期再确认：期间若已有词 / 已离开歌词页则不踢回
      if (musicPlayerViewRef.current !== 'lyrics') return
      if (songIdRef.current && lyricsLenRef.current > 0) return
      setMusicPlayerView('info')
    }, 800)

    return () => clearTimeout(timer)
  }, [
    musicPlayerView,
    songId,
    lyricsLen,
    setMusicPlayerView,
    panelVisible,
  ])

  if (!musicEnabled) {
    return null
  }

  // 仅常驻 info 视图（避免切页卸载导致封面 img 重建/闪白）
  // 歌词/列表：对应 view 时 keep-alive（面板收起 is-hidden）
  // 切歌 resetLyrics 后 lyrics 短暂为空：回落 info，避免整块空白壳
  // 面板收起时 info 降频、歌词 paused、列表停频谱与自动滚
  const lyricsWanted =
    musicPlayerView === 'lyrics' && !!currentSong && lyrics.length > 0
  const lyricsVisible = panelVisible && lyricsWanted
  const playlistWanted =
    musicPlayerView === 'playlist' && player.playlist.length > 0
  const playlistVisible = panelVisible && playlistWanted
  const showInfo =
    musicPlayerView === 'info' ||
    (musicPlayerView === 'lyrics' && !lyricsWanted) ||
    (musicPlayerView === 'playlist' && !playlistWanted)
  const infoUiActive = panelVisible && showInfo
  // mode 类跟「想在哪」走，keep-alive 隐藏时也保留内边距契约
  const viewMode = lyricsWanted
    ? 'lyrics'
    : playlistWanted
      ? 'playlist'
      : 'info'

  return (
    <div
      ref={musicContainerRef}
      className={`music-player-container music-view-mode-${viewMode}`}
    >
      {/*
        用 is-hidden + display:none !important，而不是 HTML hidden：
        .music-view { display:flex } 会盖掉 UA 的 [hidden]{display:none}
        隐藏时停频谱并跳过进度 tick 重渲染；整块 info DOM 保留（含封面）。
      */}
      <div
        className={`music-view music-view-info${showInfo ? '' : ' is-hidden'}`}
        aria-hidden={!infoUiActive}
        inert={!infoUiActive ? true : undefined}
      >
        <MusicInfoView player={player} visible={infoUiActive} />
      </div>

      {lyricsWanted && (
        <MusicLyricsView
          currentSong={currentSong}
          lyrics={lyrics}
          currentLyricIndex={player.currentLyricIndex}
          musicColor={player.musicColors?.primary || '#ef4444'}
          setMusicPlayerView={setMusicPlayerView}
          visible={lyricsVisible}
        />
      )}

      {playlistWanted && (
        <MusicPlaylistView
          visible={playlistVisible}
          playlist={player.playlist}
          currentSongIndex={player.currentSongIndex}
          isPlaying={player.isPlaying}
          playlistSearchQuery={player.playlistSearchQuery}
          excludeVipSongs={player.excludeVipSongs}
          setMusicPlayerView={player.setMusicPlayerView}
          setPlaylistSearchQuery={player.setPlaylistSearchQuery}
          setExcludeVipSongs={player.setExcludeVipSongs}
          selectSong={player.selectSong}
          playlistScrollRef={player.playlistScrollRef}
        />
      )}
    </div>
  )
}

export default MusicPlayer
