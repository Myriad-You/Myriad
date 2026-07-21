/**
 * 音乐播放器组件
 * 从 GlobalControlPanel 分离出来的音乐播放器 UI
 */

import type { UseMusicPlayerReturn } from '../../hooks/useMusicPlayer'
import {
  LuAlertTriangle,
  LuMusic,
  LuPause,
  LuPlay,
  LuSearchX,
} from '@lib/icons'
import React, { memo, useCallback, useEffect, useMemo, useState } from 'react'
import { useI18n } from '../../contexts/I18nContext'
import {
  formatTime,
  getSongVipStatus,
  highlightText,
} from '../../utils/musicPlayer'
import '../MusicPlayer.css'

interface MusicPlayerProps {
  player: UseMusicPlayerReturn
}

/**
 * 音乐信息视图
 */
const MusicInfoView: React.FC<MusicPlayerProps> = ({ player }) => {
  const { t } = useI18n()
  const {
    currentSong,
    isPlaying,
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
  } = player

  // 获取翻译后的错误消息
  const musicError = musicErrorKey
    ? (t.music as Record<string, string>)[musicErrorKey] || musicErrorKey
    : ''

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

  return (
    <>
      {/* 临时播放模式：右上角关闭按钮 */}
      {isTempPlayMode && (
        <button
          onClick={stopTempPlay}
          className="music-player-temp-close-btn"
          aria-label={t.music.stopTemp}
          title={t.music.stopTempAndRestore}
        >
          <svg width="20" height="20" viewBox="0 0 24 24" fill="currentColor">
            <path d="M19 6.41L17.59 5 12 10.59 6.41 5 5 6.41 10.59 12 5 17.59 6.41 19 12 13.41 17.59 19 19 17.59 13.41 12z" />
          </svg>
        </button>
      )}

      {/* 封面和歌曲信息 + 进度条 */}
      <div className="music-info-main">
        <div className="music-album-cover-large">
          {currentSong.cover ? (
            <img
              key={currentSong.cover}
              src={currentSong.cover}
              alt={currentSong.name}
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
          {isPlaying && (
            <div className="music-playing-indicator">
              <svg
                width="16"
                height="16"
                viewBox="0 0 24 24"
                fill="currentColor"
              >
                <path d="M12 3v10.55c-.59-.34-1.27-.55-2-.55-2.21 0-4 1.79-4 4s1.79 4 4 4 4-1.79 4-4V7h4V3h-6z" />
              </svg>
            </div>
          )}
        </div>

        <div className="music-info-right">
          <div className="music-song-info">
            <div className="music-song-name-row">
              <div className="music-song-name">{currentSong.name}</div>
              {vipStatus.displayText && (
                <span
                  className={`music-vip-badge ${vipStatus.isTrial ? 'trial' : ''}`}
                >
                  {vipStatus.displayText}
                </span>
              )}
            </div>
            <div className="music-song-artist">{currentSong.artist}</div>
          </div>

          <div className="music-progress-container">
            <span className="music-time">{formatTime(currentTime)}</span>
            {/* Wrapper lets the range shrink in flex (input min-content is stubborn) */}
            <div className="music-progress-track">
              <input
                ref={progressBarRef}
                type="range"
                min="0"
                max={audioDuration || currentSong.duration || 0}
                value={currentTime}
                onMouseDown={handleSeekStart}
                onMouseUp={handleSeekEnd}
                onTouchStart={handleSeekStart}
                onTouchEnd={handleSeekEnd}
                onInput={(e) =>
                  handleSeek(Number.parseFloat(e.currentTarget.value))
                }
                className="music-progress-bar"
                aria-label={t.music.progress}
              />
            </div>
            <span className="music-time music-time-remaining">
              {`-${formatTime(
                (audioDuration || currentSong.duration || 0) - currentTime,
              )}`}
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
              onClick={() => setMusicPlayerView('lyrics')}
              className="music-view-switch-btn"
              aria-label={t.music.lyrics}
              title={t.music.lyrics}
            >
              <svg className="w-5 h-5" fill="currentColor" viewBox="0 0 20 20">
                <path d="M18 13V5a2 2 0 00-2-2H4a2 2 0 00-2 2v8a2 2 0 002 2h3l3 3 3-3h3a2 2 0 002-2zM5 7a1 1 0 011-1h8a1 1 0 110 2H6a1 1 0 01-1-1zm1 3a1 1 0 100 2h3a1 1 0 100-2H6z" />
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
              onClick={() => setShowVolumePopup(!showVolumePopup)}
              className="music-volume-btn"
              aria-label={t.music.volume}
            >
              <svg className="w-4 h-4" fill="currentColor" viewBox="0 0 24 24">
                <path d="M3 9v6h4l5 5V4L7 9H3zm13.5 3c0-1.77-1.02-3.29-2.5-4.03v8.05c1.48-.73 2.5-2.25 2.5-4.02z" />
              </svg>
            </button>
            <div
              className={`music-volume-popup ${showVolumePopup ? 'visible' : ''}`}
            >
              <svg className="w-4 h-4" fill="currentColor" viewBox="0 0 24 24">
                <path d="M3 9v6h4l5 5V4L7 9H3zm13.5 3c0-1.77-1.02-3.29-2.5-4.03v8.05c1.48-.73 2.5-2.25 2.5-4.02z" />
              </svg>
              <input
                type="range"
                min="0"
                max="1"
                step="0.01"
                value={volume}
                onChange={(e) =>
                  handleVolumeChange(Number.parseFloat(e.target.value))
                }
                className="music-volume-slider"
                aria-label={t.music.volume}
              />
            </div>
          </div>

          {/* 播放列表按钮 */}
          {playlist.length > 0 && (
            <button
              onClick={() => setMusicPlayerView('playlist')}
              className="music-view-switch-btn"
              aria-label={t.music.playlist}
              title={t.music.playlist}
            >
              <svg className="w-5 h-5" fill="currentColor" viewBox="0 0 24 24">
                <path
                  fillRule="evenodd"
                  d="M2.625 6.75a1.125 1.125 0 112.25 0 1.125 1.125 0 01-2.25 0zm4.875 0A.75.75 0 018.25 6h12a.75.75 0 010 1.5h-12a.75.75 0 01-.75-.75zM2.625 12a1.125 1.125 0 112.25 0 1.125 1.125 0 01-2.25 0zM7.5 12a.75.75 0 01.75-.75h12a.75.75 0 010 1.5h-12A.75.75 0 017.5 12zm-4.875 5.25a1.125 1.125 0 112.25 0 1.125 1.125 0 01-2.25 0zm4.875 0a.75.75 0 01.75-.75h12a.75.75 0 010 1.5h-12a.75.75 0 01-.75-.75z"
                  clipRule="evenodd"
                />
              </svg>
            </button>
          )}
        </div>
      </div>
    </>
  )
}

/**
 * 歌词视图
 */
const MusicLyricsView: React.FC<{ player: UseMusicPlayerReturn }> = ({
  player,
}) => {
  const { t } = useI18n()
  const {
    currentSong,
    lyrics,
    currentLyricIndex,
    setMusicPlayerView,
    lyricsScrollRef,
  } = player

  // 歌词自动滚动
  useEffect(() => {
    if (!lyricsScrollRef.current || lyrics.length === 0) {
      return
    }

    const container = lyricsScrollRef.current

    if (currentLyricIndex < 0) {
      container.scrollTo({
        top: 0,
        behavior: 'smooth',
      })
      return
    }

    const activeElement = container.children[currentLyricIndex] as HTMLElement
    if (!activeElement) return

    const containerHeight = container.clientHeight
    const elementTop = activeElement.offsetTop
    const elementHeight = activeElement.clientHeight
    const scrollTop = elementTop - containerHeight / 2 + elementHeight / 2

    container.scrollTo({
      top: Math.max(0, scrollTop),
      behavior: 'smooth',
    })
  }, [currentLyricIndex, lyrics.length, lyricsScrollRef])

  if (!currentSong || lyrics.length === 0) {
    return null
  }

  const vipStatus = getSongVipStatus(currentSong)

  return (
    <div className="music-view music-view-lyrics">
      <div className="music-lyrics-header">
        <button
          onClick={() => setMusicPlayerView('info')}
          className="music-back-btn"
          aria-label={t.music.back}
        >
          <svg className="w-5 h-5" fill="currentColor" viewBox="0 0 20 20">
            <path
              fillRule="evenodd"
              d="M12.707 5.293a1 1 0 010 1.414L9.414 10l3.293 3.293a1 1 0 01-1.414 1.414l-4-4a1 1 0 010-1.414l4-4a1 1 0 011.414 0z"
              clipRule="evenodd"
            />
          </svg>
        </button>
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

      <div
        className="music-lyrics-scroll"
        ref={lyricsScrollRef}
        data-total-lyrics={lyrics.length}
        data-current-index={currentLyricIndex}
      >
        {lyrics.map((line, index) => (
          <div
            key={`lyric-${index}-${line.time}`}
            className={`music-lyric-line ${
              index === currentLyricIndex ? 'active' : ''
            } ${
              currentLyricIndex >= 0 && index < currentLyricIndex
                ? 'passed'
                : ''
            }`}
            data-time={line.time.toFixed(2)}
            data-index={index}
            data-active={index === currentLyricIndex}
          >
            {line.text}
          </div>
        ))}
      </div>
    </div>
  )
}

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
  searchQuery: string
  onSelect: (song: any, index: number, autoPlay: boolean) => void
  onClose: () => void
}>(
  ({
    song,
    originalIndex,
    isActive,
    isPlaying,
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
            <div
              className="music-playlist-name"
              dangerouslySetInnerHTML={{
                __html: searchQuery
                  ? highlightText(song.name, searchQuery)
                  : song.name,
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
              __html: searchQuery
                ? highlightText(song.artist, searchQuery)
                : song.artist,
            }}
          />
        </div>
        {isActive && (
          <span className="music-playlist-playing">
            {isPlaying ? <LuPlay size={12} /> : <LuPause size={12} />}
          </span>
        )}
      </div>
    )
  },
)

PlaylistItem.displayName = 'PlaylistItem'

/**
 * 播放列表视图
 */
const MusicPlaylistView: React.FC<{ player: UseMusicPlayerReturn }> = ({
  player,
}) => {
  const { t } = useI18n()
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

  // 🔧 监听面板动画状态，动画期间简化渲染
  const [isPanelAnimating, setIsPanelAnimating] = useState(false)

  // 🔧 监听面板动画事件
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

  // 播放列表自动滚动到当前歌曲
  useEffect(() => {
    // 动画期间不滚动
    if (isPanelAnimating) return
    if (!playlistScrollRef.current || playlist.length === 0) {
      return
    }

    const container = playlistScrollRef.current

    if (playlistSearchQuery.trim()) {
      return
    }

    setTimeout(() => {
      const activeElement = container.querySelector(
        '.music-playlist-item.active',
      ) as HTMLElement
      if (!activeElement) return

      const containerHeight = container.clientHeight
      const elementTop = activeElement.offsetTop
      const elementHeight = activeElement.clientHeight
      const scrollTop = elementTop - containerHeight / 2 + elementHeight / 2

      container.scrollTo({
        top: Math.max(0, scrollTop),
        behavior: 'smooth',
      })
    }, 100)
  }, [
    currentSongIndex,
    playlist.length,
    playlistSearchQuery,
    playlistScrollRef,
    isPanelAnimating,
  ])

  // 🔧 预计算歌曲 ID 到索引的映射，避免 O(n²) 查找
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

  // 🔧 动画期间只显示简化视图（当前歌曲附近的几首）
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
    <div className="music-view music-view-playlist">
      <div className="music-playlist-header">
        <button
          onClick={() => {
            setMusicPlayerView('info')
            setPlaylistSearchQuery('')
          }}
          className="music-back-btn"
          aria-label={t.music.back}
        >
          <svg className="w-5 h-5" fill="currentColor" viewBox="0 0 20 20">
            <path
              fillRule="evenodd"
              d="M12.707 5.293a1 1 0 010 1.414L9.414 10l3.293 3.293a1 1 0 01-1.414 1.414l-4-4a1 1 0 010-1.414l4-4a1 1 0 011.414 0z"
              clipRule="evenodd"
            />
          </svg>
        </button>
        <div className="music-playlist-title">
          {t.music.playlistTitle} ({displayPlaylist.length}/{playlist.length})
        </div>

        {/* 排除VIP开关 */}
        <button
          onClick={() => setExcludeVipSongs(!excludeVipSongs)}
          className={`music-vip-filter-toggle ${!excludeVipSongs ? 'active' : ''}`}
          aria-label={
            excludeVipSongs ? t.music.showVipSongs : t.music.hideVipSongs
          }
          title={excludeVipSongs ? t.music.showVipSongs : t.music.hideVipSongs}
        >
          <svg className="w-4 h-4" fill="currentColor" viewBox="0 0 24 24">
            <path d="M5 16L3 7l5.5 4L12 5l3.5 6L21 7l-2 9H5zm0 2h14v2H5v-2z" />
          </svg>
        </button>

        {/* 搜索框 */}
        <div className="music-playlist-search-compact">
          <svg
            className="music-search-icon"
            fill="currentColor"
            viewBox="0 0 20 20"
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
              onClick={() => setPlaylistSearchQuery('')}
              className="music-search-clear"
              aria-label={t.music.clearSearch}
            >
              <svg fill="currentColor" viewBox="0 0 20 20">
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
                isPlaying={isPlaying}
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
export const MusicPlayer: React.FC<MusicPlayerProps> = (props) => {
  const { player } = props
  const {
    musicEnabled,
    musicPlayerView,
    currentSong,
    lyrics,
    musicContainerRef,
  } = player

  // 🔧 视图切换时触发父容器重测高度
  useEffect(() => {
    // 延迟触发，等待 DOM 更新完成
    const timer = setTimeout(() => {
      window.dispatchEvent(new CustomEvent('gcp-remeasure'))
    }, 50)
    return () => clearTimeout(timer)
  }, [musicPlayerView])

  // 歌词视图自动返回：当歌词不存在时延迟检测后返回默认界面
  useEffect(() => {
    if (musicPlayerView !== 'lyrics') {
      return
    }

    if (!currentSong || lyrics.length === 0) {
      const timer = setTimeout(() => {
        if (
          musicPlayerView === 'lyrics' &&
          (!currentSong || lyrics.length === 0)
        ) {
          player.setMusicPlayerView('info')
        }
      }, 800)

      return () => clearTimeout(timer)
    }
  }, [musicPlayerView, currentSong, lyrics.length, player])

  if (!musicEnabled) {
    return null
  }

  return (
    <div ref={musicContainerRef} className="music-player-container">
      {musicPlayerView === 'info' && (
        <div className="music-view music-view-info">
          <MusicInfoView {...props} />
        </div>
      )}

      {musicPlayerView === 'lyrics' && currentSong && lyrics.length > 0 && (
        <MusicLyricsView player={player} />
      )}

      {musicPlayerView === 'playlist' && player.playlist.length > 0 && (
        <MusicPlaylistView player={player} />
      )}
    </div>
  )
}

export default MusicPlayer
