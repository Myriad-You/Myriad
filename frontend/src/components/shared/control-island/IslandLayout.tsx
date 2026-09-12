import { AnimatePresenceShim as AnimatePresence } from '@lib/motionShim'

export interface IslandLayoutProps {
  children: (variant: 'mobile' | 'desktop') => React.ReactNode
}

export function IslandLayout({ children }: IslandLayoutProps) {
  return (
    <>
      <div className="sm:hidden w-full mb-4 touch-pan-y relative z-30">
        <AnimatePresence mode="wait">{children('mobile')}</AnimatePresence>
      </div>

      <div className="hidden sm:block fixed bottom-8 left-1/2 -translate-x-1/2 z-40">
        <AnimatePresence mode="wait">{children('desktop')}</AnimatePresence>
      </div>
    </>
  )
}
