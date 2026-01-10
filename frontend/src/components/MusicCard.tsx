import React from 'react'
import PlatformIcon from './PlatformIcon'

interface MusicCardProps {
  item: {
    id: string
    title: string
    cover?: string
    platform: string
    metadata: {
      id?: string
      artist?: string
      ar?: Array<{ name: string }>
      isVip?: boolean
      fee?: number
    }
  }
  layout: {
    left: number
    top: number
    width: number
    height: number
  }
  platformColor: string
  isVip: boolean
  isCurrentSong: boolean
  isPlaying: boolean
  musicColor: string
  animationDelay: number
  onPlay: () => void
}

/**
 * ✅ 音乐卡片组件 - 已优化性能
 * - 使用 React.memo 避免不必要的重新渲染
 * - 仅在 props 变化时重新渲染
 */
const MusicCard = React.memo<MusicCardProps>(({
  item,
  layout,
  platformColor,
  isVip,
  isCurrentSong,
  isPlaying,
  musicColor,
  animationDelay,
  onPlay,
}) => {
  // 提取艺术家信息
  const getArtist = () => {
    const artists = item.metadata.ar
    if (Array.isArray(artists) && artists.length > 0) {
      return artists.map((a: any) => a.name || a).join(', ')
    }
    if (item.metadata.artist) {
      return item.metadata.artist
    }
    return null
  }

  const artist = getArtist()

  return (
    <div
      key={item.id}
      className="absolute group library-card-container"
      style={{
        'left': `${layout.left}px`,
        'top': `${layout.top}px`,
        'width': `${layout.width}px`,
        'height': `${layout.height}px`,
        '--platform-color': platformColor,
        'animationDelay': `${animationDelay}s`,
      } as React.CSSProperties}
    >
      {/* 音乐卡片：正方形专辑封面 */}
      <div className="relative bg-white rounded-xl shadow-md hover:shadow-2xl transition-all duration-300 transform hover:-translate-y-1 hover:scale-[1.02] overflow-hidden h-full">
        <div
          className="block w-full h-full relative cursor-pointer"
          onClick={(e) => {
            e.preventDefault()
            e.stopPropagation()
            onPlay()
          }}
        >
          {item.cover
            ? (
                <img
                  src={item.cover}
                  alt={item.title}
                  className="w-full h-full object-cover transition-all duration-500 group-hover:scale-110"
                  loading="lazy"
                  decoding="async"
                  onError={(e) => {
                    (e.target as HTMLImageElement).src = `https://ui-avatars.com/api/?name=${encodeURIComponent(item.title)}&size=400&background=random`
                  }}
                />
              )
            : (
                <div className="w-full h-full bg-gradient-to-br from-pink-400 to-purple-500 flex items-center justify-center">
                  <span className="text-white text-4xl font-bold">
                    {item.title.charAt(0).toUpperCase()}
                  </span>
                </div>
              )}

          {/* VIP 标识 */}
          {isVip && (
            <div className="absolute top-2 left-2 bg-gradient-to-r from-yellow-400 to-orange-500 text-white text-xs font-bold px-2 py-1 rounded-full shadow-lg z-10">
              VIP
            </div>
          )}

          {/* 播放中指示器 */}
          {isPlaying && (
            <div
              className="playing-indicator"
              style={{
                '--music-color': musicColor,
                '--platform-color': musicColor,
              } as React.CSSProperties}
            >
              <PlatformIcon platform={item.platform} className="w-8 h-8" />
            </div>
          )}

          {/* 平台图标 */}
          <div className="absolute top-2 right-2 z-10">
            <PlatformIcon
              platform={item.platform}
              className="w-6 h-6 opacity-90 drop-shadow-lg"
            />
          </div>

          {/* 悬停遮罩 */}
          <div className="absolute inset-0 bg-black bg-opacity-0 group-hover:bg-opacity-30 transition-all duration-300 pointer-events-none" />

          {/* 歌曲信息 */}
          <div className="absolute bottom-0 left-0 right-0 bg-gradient-to-t from-black/80 via-black/50 to-transparent p-3 transform translate-y-full group-hover:translate-y-0 transition-transform duration-300">
            <h3 className="text-white font-semibold text-sm truncate mb-1">
              {item.title}
            </h3>
            {artist && (
              <p className="text-white/80 text-xs truncate">
                {artist}
              </p>
            )}
          </div>
        </div>
      </div>
    </div>
  )
})

MusicCard.displayName = 'MusicCard'

export default MusicCard
