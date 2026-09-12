/** 不进口 pageData / manager。 */

import type { ReactNode } from 'react'
import { AnimatePresenceShim as AnimatePresence } from '@lib/motionShim'
import AnimatedView from '../../AnimatedView'
import { Spinner } from '../../Spinner'
import '../../../styles/page-frame.css'

export function BrewPage({
  lock,
  loading,
  children,
}: {
  lock: boolean
  loading?: boolean
  children?: ReactNode
}) {
  return (
    <AnimatedView
      className={`brew-shell ${lock && !loading ? 'h-dvh' : 'min-h-screen'}`}
    >
      <div
        className="brew-shell__inner h-full min-h-0 flex flex-col"
      >
        <div className="brew-shell__stage flex-1 mx-auto w-full flex flex-col relative min-h-0">
          {loading ? (
            <div className="flex flex-1 items-center justify-center">
              <Spinner size="lg" />
            </div>
          ) : (
            children
          )}
        </div>
      </div>
    </AnimatedView>
  )
}

export { AnimatePresence }
