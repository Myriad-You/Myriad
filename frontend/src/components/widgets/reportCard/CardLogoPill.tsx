/**
 * Bottom-left platform logo pill; expands with detail-face titles from the face.
 */
import {
  AnimatePresenceShim as AnimatePresence,
  motionShim as motion,
} from '@lib/motionShim'
import { PLATFORM_CONFIG } from './platformConfig'

export type CardContent = {
  title: string
  type?: string
  titles?: string[]
} | null

interface CardLogoPillProps {
  platformId: string
  cardContent: CardContent
}

export function CardLogoPill({ platformId, cardContent }: CardLogoPillProps) {
  const platformConfig =
    PLATFORM_CONFIG[platformId] || PLATFORM_CONFIG.bilibili

  return (
    <motion.div
      className="absolute bottom-3 left-3 z-20"
      initial={false}
      animate={{ width: cardContent ? 'auto' : '32px' }}
      transition={{ duration: 0.3, ease: 'easeOut' }}
    >
      <div
        className={`rounded-lg flex items-center gap-2 ${platformConfig.textColor} shadow-lg transition-all overflow-hidden ${
          cardContent ? 'bg-white/95 dark:bg-black/95' : ''
        }`}
        style={{
          background: cardContent ? undefined : platformConfig.bgColor,
          border: `1px solid ${platformConfig.borderColor}`,
          padding: cardContent?.titles ? '4px 8px' : '0 8px',
          height: cardContent?.titles ? 'auto' : '32px',
        }}
      >
        <div className={`text-base shrink-0 ${platformConfig.textColor}`}>
          {platformConfig.icon}
        </div>
        <AnimatePresence>
          {cardContent && (
            <motion.div
              initial={{ opacity: 0, width: 0 }}
              animate={{ opacity: 1, width: 'auto' }}
              exit={{ opacity: 0, width: 0 }}
              transition={{ duration: 0.3 }}
              className="flex items-center gap-2 whitespace-nowrap overflow-hidden"
            >
              {cardContent.titles ? (
                <div className="flex flex-col gap-0.5">
                  {cardContent.titles.map((title: string, idx: number) => (
                    <div
                      key={idx}
                      className="text-[10px] font-bold text-gray-900 dark:text-gray-100 max-w-30 truncate leading-tight"
                    >
                      {title}
                    </div>
                  ))}
                </div>
              ) : (
                <span className="text-[11px] font-bold text-gray-900 dark:text-gray-100 max-w-30 truncate">
                  {cardContent.title}
                </span>
              )}
            </motion.div>
          )}
        </AnimatePresence>
      </div>
    </motion.div>
  )
}
