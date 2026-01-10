/**
 * 平台卡片小组件 - 显示单个平台的快捷入口
 */

import type { WidgetComponentProps } from '../WidgetGrid'
import { FaGithub, FaSteam, SiBilibili, SiNeteasecloudmusic } from '@lib/icons'
import { motionShim as motion } from '@lib/motionShim'
import { useMemo } from 'react'
import { useNavigate } from 'react-router-dom'
import { useI18n } from '../../contexts/I18nContext'

const PLATFORMS = [
  {
    id: 'bilibili',
    name: 'Bilibili',
    icon: <SiBilibili />,
    color: 'from-blue-400 to-cyan-500',
    emoji: '📺',
  },
  {
    id: 'steam',
    name: 'Steam',
    icon: <FaSteam />,
    color: 'from-gray-700 to-gray-800',
    emoji: '🎮',
  },
  {
    id: 'github',
    name: 'GitHub',
    icon: <FaGithub />,
    color: 'from-gray-700 to-gray-900',
    emoji: '💻',
  },
  {
    id: 'netease',
    name: 'NetEase',
    icon: <SiNeteasecloudmusic />,
    color: 'from-red-500 to-red-600',
    emoji: '🎵',
  },
]

export function PlatformCardWidget({ config, isEditMode }: WidgetComponentProps) {
  const navigate = useNavigate()
  const { t } = useI18n()

  // 从配置中获取平台ID，默认为bilibili
  const platformId = config.config?.platformId || 'bilibili'
  const platform = PLATFORMS.find(p => p.id === platformId) || PLATFORMS[0]

  // 翻译的平台名称
  const displayName = useMemo(() => {
    if (platformId === 'netease') {
      return t.platformCard.neteaseMusic
    }
    return platform.name
  }, [platformId, platform.name, t])

  const handleClick = () => {
    if (!isEditMode) {
      navigate('/reports')
    }
  }

  return (
    <motion.div
      className="h-full w-full rounded-xl relative overflow-hidden cursor-pointer"
      whileHover={!isEditMode ? { scale: 1.02 } : {}}
      whileTap={!isEditMode ? { scale: 0.98 } : {}}
      onClick={handleClick}
    >
      {/* 渐变背景 */}
      <div className={`absolute inset-0 bg-gradient-to-br ${platform.color} opacity-90`} />

      {/* 装饰圆 */}
      <div className="absolute -right-10 -top-10 w-40 h-40 rounded-full bg-white/10 blur-2xl" />
      <div className="absolute -left-10 -bottom-10 w-32 h-32 rounded-full bg-black/10 blur-xl" />

      {/* 内容 */}
      <div className="relative z-10 h-full flex flex-col justify-between p-4">
        <div className="flex items-center gap-2">
          <div className="text-white text-2xl">
            {platform.icon}
          </div>
          <span className="text-white font-bold text-lg">
            {displayName}
          </span>
        </div>

        <div className="text-right">
          <div className="text-5xl opacity-20">
            {platform.emoji}
          </div>
        </div>
      </div>

      {isEditMode && (
        <div className="absolute inset-0 border-2 border-dashed border-white/50 rounded-xl pointer-events-none" />
      )}
    </motion.div>
  )
}
