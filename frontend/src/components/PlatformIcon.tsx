import {
  BangumiIcon,
  FaGithub,
  FaSteam,
  FaXbox,
  FaXTwitter,
  SiBilibili,
  SiDiscord,
  SiMyanimelist,
  SiNeteasecloudmusic,
  SiPlaystation,
  SiYoutube,
} from '@lib/platformBrandIcons'

import React, { useMemo } from 'react'
import { getPlatformBrandColor } from '../utils/platformBrand'
import { useThemeMode } from '../utils/themeSubscriber'

interface PlatformIconProps {
  platform: string
  className?: string
  style?: React.CSSProperties
  brandColor?: boolean
}

const PlatformIcon: React.FC<PlatformIconProps> = React.memo(
  ({ platform, className = 'w-6 h-6', style, brandColor = true }) => {
    const isDark = useThemeMode()
    const mergedStyle = useMemo(() => {
      if (!brandColor) return style
      if (style?.color) return style
      return {
        ...style,
        color: getPlatformBrandColor(platform, isDark),
      }
    }, [brandColor, isDark, platform, style])

    switch (platform.toLowerCase()) {
      case 'github':
        return <FaGithub className={className} style={mergedStyle} />
      case 'bilibili':
        return <SiBilibili className={className} style={mergedStyle} />
      case 'steam':
        return <FaSteam className={className} style={mergedStyle} />
      case 'youtube':
      case 'yt':
        return <SiYoutube className={className} style={mergedStyle} />
      case 'netease music':
      case 'netease':
      case '网易云音乐':
        return <SiNeteasecloudmusic className={className} style={mergedStyle} />
      case 'bangumi':
        return <BangumiIcon className={className} style={mergedStyle} />
      case 'mal':
      case 'myanimelist':
        return <SiMyanimelist className={className} style={mergedStyle} />
      case 'x':
      case 'twitter':
      case 'x (twitter)':
        return <FaXTwitter className={className} style={mergedStyle} />
      case 'discord':
        return <SiDiscord className={className} style={mergedStyle} />
      case 'xbox':
        return <FaXbox className={className} style={mergedStyle} />
      case 'psn':
      case 'playstation':
        return <SiPlaystation className={className} style={mergedStyle} />
      default:
        return (
          <span className={className} style={mergedStyle}>
            ?
          </span>
        )
    }
  },
)

PlatformIcon.displayName = 'PlatformIcon'

export default PlatformIcon
