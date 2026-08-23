/**
 * 音乐播放器组件
 * 从 GlobalControlPanel 分离出来的音乐播放器 UI
 */

import type { UseMusicPlayerReturn } from '../../hooks/useMusicPlayer'
import {
  LuAlertTriangle,
  LuListMusic,
  LuMusic,
  LuSearchX,
  LuVolume2,
} from '@lib/icons'
import React, {
  memo,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from 'react'
import { useI18n } from '../../contexts/I18nContext'
import { formatMusicError } from '../../utils/musicError'
import { useAnimationLevel } from '../../hooks/useAnimationLevel'
import {
  formatTime,
  getSongVipStatus,
  highlightText,
} from '../../utils/musicPlayer'
import { LyricWaveScroll } from '../shared/LyricWaveScroll'
import { PlayingSpectrum } from '../shared/PlayingSpectrum'
import { FitText } from '../widgets/shared/FitText'
import '../MusicPlayer.css'

interface MusicPlayerProps {
  player: UseMusicPlayerReturn
  /**
   * 控制面板是否处于可交互展开态。
   * 收起时停频谱 / 卸载歌词与列表引擎，仅保留 info DOM（封面缓存）。
   * 默认 true，便于单独挂载时行为不变。
   */
  panelVisible?: boolean
}

interface MusicInfoViewProps {
  player: UseMusicPlayerReturn
  /** 是否为当前展示的视图；false 时停频谱/高频重渲染，仅保留封面 DOM */
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

/**
 * 音乐信息视图（常驻 DOM；不可见时降频）
 */
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

  // 获取翻译后的错误消息
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

  // Escape 关闭 + 打开时焦点进滑块，关闭后回到音量按钮
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

  // 加载中跟随进度；settle/exit 用冻结坐标
  if (loadDotPhase === 'active') {
    loadDotAtRef.current = progressPercent
  }
  const loadDotLeft =
    loadDotPhase === 'active' ? progressPercent : loadDotAtRef.current

  return (
    <>
      {/* 默认页右上角：中线上下扩展频谱（隐藏时不驱动） */}
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

      {/* 封面和歌曲信息 + 进度条 */}
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
                  window.dispatchEvent(
                    new CustomEvent('music-cover-loaded', {
                      detail: {
                        songId: currentSong.id,
                        cover: currentSong.cover,
                      },
                    }),
                  )
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

      {/* 播放控制按钮 + 音量 + 视图切换 */}
      <div className="music-control-row">
        {/* 左侧：歌词按钮和播放顺序按钮 */}
        <div className="music-view-switcher">
          {lyrics.length > 0 && (
            <button
              type="button"
              onClick={() => setMusicPlayerView('lyrics')}
              className="music-view-switch-btn"
              aria-label={t.music.lyrics}
              title={t.music.lyrics}
            >
              {/* 线框气泡 + 两行（对齐原实心 chat-alt，比 MessageSquareText 三行更干净） */}
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
          {/* 播放顺序按钮 - 临时播放模式下隐藏 */}
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

        {/* 中间：核心控制按钮 */}
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

        {/* 右侧：音量和播放列表 */}
        <div className="music-view-switcher music-view-switcher-right">
          {/* 音量控制（弹出式） */}
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
              {/* 与播放进度同一套自绘轨道 + hover 圆点 */}
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

          {/* 临时播放：关闭按钮替换列表；正常模式：播放列表 */}
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

/**
 * 歌词页小封面：占位垫底 + 失败回退 + 缓存命中 complete 校正。
 * 避免条件挂载重进时偶发空白（load 事件已过 / 代理失败无回退）。
 */
const MusicLyricsCover = memo(({
  songId,
  cover,
}: {
  songId: string
  cover?: string | null
}) => {
  const coverUrl = (cover || '').trim()
  const coverKey = `${songId}:${coverUrl}`
  /** 仅当 failKey 对应当前 cover 时视为失败，切歌自动失效 */
  const [failKey, setFailKey] = useState<string | null>(null)
  const failed = Boolean(coverUrl) && failKey === coverKey
  const imgRef = useRef<HTMLImageElement>(null)

  // 磁盘/内存缓存命中时 load 可能已结束，onLoad 听不到 → 校正
  useLayoutEffect(() => {
    if (!coverUrl || failed) return
    const img = imgRef.current
    if (!img) return
    if (img.complete) {
      if (img.naturalWidth > 0) {
        img.style.display = ''
      } else {
        setFailKey(coverKey)
      }
    }
  }, [coverKey, coverUrl, failed])

  const showImg = Boolean(coverUrl) && !failed

  return (
    <div className="music-lyrics-cover" aria-hidden>
      {showImg && (
        <img
          ref={imgRef}
          key={coverKey}
          src={coverUrl}
          alt=""
          decoding="async"
          referrerPolicy="no-referrer"
          draggable={false}
          onLoad={(e) => {
            e.currentTarget.style.display = ''
            setFailKey((k) => (k === coverKey ? null : k))
          }}
          onError={() => setFailKey(coverKey)}
        />
      )}
      <div
        className={`music-lyrics-cover__placeholder${showImg ? ' is-behind' : ''}`}
      >
        <LuMusic className="music-lyrics-cover__icon" aria-hidden />
      </div>
    </div>
  )
})

/**
 * 歌词视图 — 切换逻辑与资料库卡片共用 LyricWaveScroll（Tapp 波浪）
 * visible=false 时 is-hidden 保 DOM（封面不卸载），波浪引擎 paused
 */
const MusicLyricsView = memo(({
  player,
  visible,
}: {
  player: UseMusicPlayerReturn
  visible: boolean
}) => {
  const { t } = useI18n()
  const {
    currentSong,
    lyrics,
    currentLyricIndex,
    setMusicPlayerView,
    musicColors,
  } = player

  if (!currentSong || lyrics.length === 0) {
    return null
  }

  const vipStatus = getSongVipStatus(currentSong)
  const musicColor = musicColors?.primary || '#ef4444'

  return (
    <div
      className={`music-view music-view-lyrics${visible ? '' : ' is-hidden'}`}
      aria-hidden={!visible}
      inert={!visible ? true : undefined}
    >
      <div className="music-lyrics-header">
        <button
          type="button"
          onClick={() => setMusicPlayerView('info')}
          className="music-back-btn music-lyrics-back-btn"
          aria-label={t.music.back}
        >
          <svg
            className="music-lyrics-back-btn__icon"
            fill="currentColor"
            viewBox="0 0 20 20"
            aria-hidden
          >
            <path
              fillRule="evenodd"
              d="M12.707 5.293a1 1 0 010 1.414L9.414 10l3.293 3.293a1 1 0 01-1.414 1.414l-4-4a1 1 0 010-1.414l4-4a1 1 0 011.414 0z"
              clipRule="evenodd"
            />
          </svg>
          <span className="music-lyrics-back-btn__label">{t.music.back}</span>
        </button>
        <div className="music-lyrics-meta">
          <MusicLyricsCover songId={currentSong.id} cover={currentSong.cover} />
          <div className="music-lyrics-title">
            <div className="music-lyrics-song-name-row">
              <div className="music-lyrics-song-name">{currentSong.name}</div>
              {vipStatus.displayText && (
                <span
                  className={`music-vip-badge ${vipStatus.isTrial ? 'trial' : ''}`}
                >
                  {vipStatus.displayText}
                </span>
              )}
            </div>
            <div className="music-lyrics-artist">{currentSong.artist}</div>
          </div>
        </div>
      </div>

      <LyricWaveScroll
        variant="panel"
        lyrics={lyrics}
        currentLyricIndex={currentLyricIndex}
        musicColor={musicColor}
        paused={!visible}
      />
    </div>
  )
}, (prev, next) => {
  // player 每帧新对象：只比歌词页关心的字段，避免进度 tick 空转
  if (prev.visible !== next.visible) return false
  const a = prev.player
  const b = next.player
  return (
    a.currentSong?.id === b.currentSong?.id &&
    a.currentSong?.cover === b.currentSong?.cover &&
    a.currentSong?.name === b.currentSong?.name &&
    a.currentSong?.artist === b.currentSong?.artist &&
    a.lyrics === b.lyrics &&
    a.currentLyricIndex === b.currentLyricIndex &&
    a.musicColors?.primary === b.musicColors?.primary &&
    a.setMusicPlayerView === b.setMusicPlayerView
  )
})

/**
 * 单个播放列表项 - 使用 memo 避免不必要的重渲染
 */
const PlaylistItem = memo<{
  song: {
    id: string
    name: string
    artist: string
    isVip?: boolean
    vipType?: string
  }
  originalIndex: number
  isActive: boolean
  isPlaying: boolean
  /** 是否启用实时频谱（由父级 animation level 决定） */
  useSpectrum: boolean
  searchQuery: string
  onSelect: (song: any, index: number, autoPlay: boolean) => void
  onClose: () => void
}>(
  ({
    song,
    originalIndex,
    isActive,
    isPlaying,
    useSpectrum,
    searchQuery,
    onSelect,
    onClose,
  }) => {
    const vipStatus = getSongVipStatus(song)

    const handleClick = useCallback(() => {
      onSelect(song, originalIndex, true)
      onClose()
    }, [song, originalIndex, onSelect, onClose])

    return (
      <div
        onClick={handleClick}
        className={`music-playlist-item ${isActive ? 'active' : ''}`}
      >
        <span className="music-playlist-index">{originalIndex + 1}</span>
        <div className="music-playlist-info">
          <div className="music-playlist-name-row">
            {/* 曲名/艺人名来自远端音乐 API，属于不可信数据。highlightText 内部
                会转义 HTML 元字符；无搜索词时也必须走转义，不能直接把原值塞进
                innerHTML —— 那条分支曾经是一个可执行的 XSS。 */}
            <div
              className="music-playlist-name"
              dangerouslySetInnerHTML={{
                __html: highlightText(song.name, searchQuery),
              }}
            />
            {vipStatus.displayText && (
              <span
                className={`music-vip-badge ${vipStatus.isTrial ? 'trial' : ''}`}
              >
                {vipStatus.displayText}
              </span>
            )}
          </div>
          <div
            className="music-playlist-artist"
            dangerouslySetInnerHTML={{
              __html: highlightText(song.artist, searchQuery),
            }}
          />
        </div>
        {isActive && (
          <span
            className={`music-playlist-playing${isPlaying ? ' is-playing' : ''}`}
            aria-hidden
          >
            <PlayingSpectrum
              themeColor="var(--music-base-primary, #ec4899)"
              scale={0.7}
              isPlaying={isPlaying}
              useSpectrum={useSpectrum}
            />
          </span>
        )}
      </div>
    )
  },
)

PlaylistItem.displayName = 'PlaylistItem'

/**
 * 播放列表视图
 * visible=false 时 is-hidden 保 DOM（滚动位置/列表不卸载）
 */
const MusicPlaylistView: React.FC<{
  player: UseMusicPlayerReturn
  visible: boolean
}> = ({ player, visible }) => {
  const { t } = useI18n()
  const anim = useAnimationLevel()
  // 与小组件一致：仅 standard 级走实时频谱；隐藏时关掉
  const useSpectrum = visible && anim.level === 'standard'
  const {
    playlist,
    currentSongIndex,
    isPlaying,
    playlistSearchQuery,
    excludeVipSongs,
    setMusicPlayerView,
    setPlaylistSearchQuery,
    setExcludeVipSongs,
    selectSong,
    playlistScrollRef,
  } = player

  // 监听面板动画状态，动画期间简化渲染
  const [isPanelAnimating, setIsPanelAnimating] = useState(false)

  // 监听面板动画事件
  useEffect(() => {
    const handleAnimationStart = () => setIsPanelAnimating(true)
    const handleAnimationEnd = () => setIsPanelAnimating(false)

    window.addEventListener('gcp-animation-start', handleAnimationStart)
    window.addEventListener('gcp-animation-end', handleAnimationEnd)

    return () => {
      window.removeEventListener('gcp-animation-start', handleAnimationStart)
      window.removeEventListener('gcp-animation-end', handleAnimationEnd)
    }
  }, [])

  // 播放列表自动锁定：打开列表 / 切歌时把当前曲滚到视口中部
  // 两列 grid 用相对容器坐标，避免 offsetTop 偏差；布局未就绪时重试
  useEffect(() => {
    if (!visible) return
    if (isPanelAnimating) return
    if (playlist.length === 0) return
    if (playlistSearchQuery.trim()) return

    let cancelled = false
    let attempt = 0
    let rafId = 0
    let timerId = 0
    const maxAttempts = 10

    const scrollToActive = (): boolean => {
      const scroller = playlistScrollRef.current
      if (!scroller) return false
      const activeElement = scroller.querySelector(
        '.music-playlist-item.active',
      ) as HTMLElement | null
      if (!activeElement) return false
      // 高度尚未展开（父容器 remeasure 前）则失败重试
      if (scroller.clientHeight < 16) return false

      const scrollerRect = scroller.getBoundingClientRect()
      const itemRect = activeElement.getBoundingClientRect()
      const elementTop =
        itemRect.top - scrollerRect.top + scroller.scrollTop
      const elementHeight = itemRect.height || 1
      const containerHeight = scroller.clientHeight
      const maxScroll = Math.max(0, scroller.scrollHeight - containerHeight)
      const target = elementTop - containerHeight / 2 + elementHeight / 2

      // auto：打开即锁定，避免 smooth 与多次重试互相打断
      scroller.scrollTo({
        top: Math.max(0, Math.min(target, maxScroll)),
        behavior: 'auto',
      })
      return true
    }

    const tryScroll = () => {
      if (cancelled) return
      if (scrollToActive()) return
      if (attempt++ >= maxAttempts) return
      rafId = requestAnimationFrame(tryScroll)
    }

    // 等 mode 内边距 / grid 首帧布局，再开锁定；仍失败则 rAF 重试
    timerId = window.setTimeout(() => {
      rafId = requestAnimationFrame(tryScroll)
    }, 40)

    // 面板高度重测后再锁一次（MusicPlayer 会 dispatch gcp-remeasure）
    const onRemeasure = () => {
      if (cancelled) return
      window.setTimeout(() => {
        if (!cancelled) scrollToActive()
      }, 30)
    }
    window.addEventListener('gcp-remeasure', onRemeasure)

    return () => {
      cancelled = true
      window.clearTimeout(timerId)
      if (rafId) cancelAnimationFrame(rafId)
      window.removeEventListener('gcp-remeasure', onRemeasure)
    }
  }, [
    visible,
    currentSongIndex,
    playlist.length,
    playlistSearchQuery,
    playlistScrollRef,
    isPanelAnimating,
  ])

  // 预计算歌曲 ID 到索引的映射，避免 O(n²) 查找
  const songIdToIndex = useMemo(() => {
    const map = new Map<string, number>()
    playlist.forEach((song, index) => {
      map.set(song.id, index)
    })
    return map
  }, [playlist])

  // 关闭播放列表的回调
  const handleClosePlaylist = useCallback(() => {
    setMusicPlayerView('info')
    setPlaylistSearchQuery('')
  }, [setMusicPlayerView, setPlaylistSearchQuery])

  if (playlist.length === 0) {
    return null
  }

  // 过滤播放列表
  const displayPlaylist = playlistSearchQuery.trim()
    ? playlist.filter((song) => {
        const query = playlistSearchQuery.toLowerCase()
        return (
          song.name.toLowerCase().includes(query) ||
          song.artist.toLowerCase().includes(query)
        )
      })
    : playlist

  // 动画期间只显示简化视图（当前歌曲附近的几首）
  const visiblePlaylist =
    isPanelAnimating && displayPlaylist.length > 20
      ? displayPlaylist.slice(
          Math.max(0, currentSongIndex - 3),
          Math.min(displayPlaylist.length, currentSongIndex + 7),
        )
      : displayPlaylist

  // 计算动画期间的偏移索引
  const indexOffset =
    isPanelAnimating && displayPlaylist.length > 20
      ? Math.max(0, currentSongIndex - 3)
      : 0

  return (
    <div
      className={`music-view music-view-playlist${visible ? '' : ' is-hidden'}`}
      aria-hidden={!visible}
      inert={!visible ? true : undefined}
    >
      <div className="music-playlist-header">
        <button
          type="button"
          onClick={() => {
            setMusicPlayerView('info')
            setPlaylistSearchQuery('')
          }}
          className="music-back-btn music-lyrics-back-btn"
          aria-label={t.music.back}
        >
          <svg
            className="music-lyrics-back-btn__icon"
            fill="currentColor"
            viewBox="0 0 20 20"
            aria-hidden
          >
            <path
              fillRule="evenodd"
              d="M12.707 5.293a1 1 0 010 1.414L9.414 10l3.293 3.293a1 1 0 01-1.414 1.414l-4-4a1 1 0 010-1.414l4-4a1 1 0 011.414 0z"
              clipRule="evenodd"
            />
          </svg>
          <span className="music-lyrics-back-btn__label">{t.music.back}</span>
        </button>
        <div className="music-playlist-title">
          <span className="music-playlist-title-text">
            {t.music.playlistTitle}
          </span>
          <span className="music-playlist-count">
            {displayPlaylist.length}/{playlist.length}
          </span>
        </div>

        {/* 排除VIP开关 */}
        <button
          type="button"
          onClick={() => setExcludeVipSongs(!excludeVipSongs)}
          className={`music-vip-filter-toggle ${!excludeVipSongs ? 'active' : ''}`}
          aria-label={
            excludeVipSongs ? t.music.showVipSongs : t.music.hideVipSongs
          }
          title={excludeVipSongs ? t.music.showVipSongs : t.music.hideVipSongs}
        >
          <svg fill="currentColor" viewBox="0 0 24 24" aria-hidden>
            <path d="M5 16L3 7l5.5 4L12 5l3.5 6L21 7l-2 9H5zm0 2h14v2H5v-2z" />
          </svg>
        </button>

        {/* 搜索框 */}
        <div className="music-playlist-search-compact">
          <svg
            className="music-search-icon"
            fill="currentColor"
            viewBox="0 0 20 20"
            aria-hidden
          >
            <path
              fillRule="evenodd"
              d="M8 4a4 4 0 100 8 4 4 0 000-8zM2 8a6 6 0 1110.89 3.476l4.817 4.817a1 1 0 01-1.414 1.414l-4.816-4.816A6 6 0 012 8z"
              clipRule="evenodd"
            />
          </svg>
          <input
            type="text"
            placeholder={t.music.searchPlaceholder}
            value={playlistSearchQuery}
            onChange={(e) => setPlaylistSearchQuery(e.target.value)}
            className="music-search-input"
          />
          {playlistSearchQuery && (
            <button
              type="button"
              onClick={() => setPlaylistSearchQuery('')}
              className="music-search-clear"
              aria-label={t.music.clearSearch}
            >
              <svg fill="currentColor" viewBox="0 0 20 20" aria-hidden>
                <path
                  fillRule="evenodd"
                  d="M4.293 4.293a1 1 0 011.414 0L10 8.586l4.293-4.293a1 1 0 111.414 1.414L11.414 10l4.293 4.293a1 1 0 01-1.414 1.414L10 11.414l-4.293 4.293a1 1 0 01-1.414-1.414L8.586 10 4.293 5.707a1 1 0 010-1.414z"
                  clipRule="evenodd"
                />
              </svg>
            </button>
          )}
        </div>
      </div>

      <div
        className={`music-playlist-scroll ${isPanelAnimating ? 'animating' : ''}`}
        ref={playlistScrollRef}
      >
        {visiblePlaylist.length > 0 ? (
          visiblePlaylist.map((song, idx) => {
            // 使用 Map 查找，O(1) 复杂度
            const originalIndex =
              songIdToIndex.get(song.id) ?? indexOffset + idx

            return (
              <PlaylistItem
                key={song.id}
                song={song}
                originalIndex={originalIndex}
                isActive={currentSongIndex === originalIndex}
                isPlaying={
                  visible && currentSongIndex === originalIndex
                    ? isPlaying
                    : false
                }
                useSpectrum={useSpectrum}
                searchQuery={playlistSearchQuery}
                onSelect={selectSong}
                onClose={handleClosePlaylist}
              />
            )
          })
        ) : (
          <div className="music-no-results">
            <div className="music-no-results-icon">
              <LuSearchX size={20} />
            </div>
            <div className="music-no-results-text">{t.music.noMatching}</div>
          </div>
        )}
      </div>
    </div>
  )
}

/**
 * 主音乐播放器组件
 */
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

  // 视图切换时触发父容器重测高度（仅面板展开时有意义）
  useEffect(() => {
    if (!panelVisible) return
    // 延迟触发，等待 DOM 更新完成
    const timer = setTimeout(() => {
      window.dispatchEvent(new CustomEvent('gcp-remeasure'))
    }, 50)
    return () => clearTimeout(timer)
  }, [musicPlayerView, panelVisible])

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
        隐藏时 MusicInfoView 停频谱 / 跳过进度 tick 重渲染，只保留封面 DOM
      */}
      <div
        className={`music-view music-view-info${showInfo ? '' : ' is-hidden'}`}
        aria-hidden={!infoUiActive}
        inert={!infoUiActive ? true : undefined}
      >
        <MusicInfoView player={player} visible={infoUiActive} />
      </div>

      {lyricsWanted && (
        <MusicLyricsView player={player} visible={lyricsVisible} />
      )}

      {playlistWanted && (
        <MusicPlaylistView player={player} visible={playlistVisible} />
      )}
    </div>
  )
}

export default MusicPlayer
