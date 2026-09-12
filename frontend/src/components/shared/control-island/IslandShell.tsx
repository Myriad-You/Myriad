import { motionShim as motion } from '@lib/motionShim'

import { ISLAND_GLASS, ISLAND_GLASS_EDIT, SPRING_SNAPPY } from './constants'

export interface IslandShellProps {
  variant: 'mobile' | 'desktop'
  editStyle?: boolean
  className?: string
  motionKey?: string
  children: React.ReactNode
}

export function IslandShell({
  variant,
  editStyle = false,
  className = '',
  motionKey,
  children,
}: IslandShellProps) {
  const isMobile = variant === 'mobile'
  const glass = editStyle ? ISLAND_GLASS_EDIT : ISLAND_GLASS

  return (
    <motion.div
      key={motionKey}
      initial={{
        opacity: 0,
        y: isMobile ? -8 : 8,
        scale: 0.97,
      }}
      animate={{ opacity: 1, y: 0, scale: 1 }}
      exit={{
        opacity: 0,
        y: isMobile ? -8 : 8,
        scale: 0.97,
      }}
      transition={SPRING_SNAPPY}
      className={`flex items-center gap-2 px-2.5 py-2 ${glass} ${className}`}
    >
      {children}
    </motion.div>
  )
}
