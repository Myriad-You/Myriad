import type { AnimationEvent, CSSProperties, ReactNode } from 'react'
import { FaArrowLeft } from '@lib/icons'
import { motionShim as motion } from '@lib/motionShim'
import { createPortal } from 'react-dom'
import { isExlight, useAnimationLevel } from '../../hooks/useAnimationLevel'
import { isWebKit } from '../../utils/platformDetect'
import {
  TAPP_SHELL_CHROME_PAD_CLASS,
  TAPP_SHELL_CHROME_PB_DESKTOP,
  TAPP_SHELL_CHROME_PB_MOBILE,
  tappShellContentClass,
} from '../hooks/tappShellChrome'

export interface TappAppShellPresence {
  shellClassName: string
  scrimClassName: string
  shellStyle: CSSProperties | undefined
  onShellAnimationEnd: (event: AnimationEvent<HTMLElement>) => void
  isExiting: boolean
}

export interface TappAppShellProps {
  shellAttr: 'data-tapp-run-shell' | 'data-tapp-store-shell'
  isMobile: boolean
  isFullscreen: boolean
  presence: TappAppShellPresence
  fullscreenToolbar: ReactNode
  headerLeading: ReactNode
  headerActions: ReactNode
  onBack: () => void
  backTitle: string
  backAriaLabel: string
  children: ReactNode
  contentClassName?: string
  contentStyle?: CSSProperties
}

export function TappAppShell({
  shellAttr,
  isMobile,
  isFullscreen,
  presence,
  fullscreenToolbar,
  headerLeading,
  headerActions,
  onBack,
  backTitle,
  backAriaLabel,
  children,
  contentClassName,
  contentStyle,
}: TappAppShellProps) {
  const animConfig = useAnimationLevel()
  const noAnimation = isExlight(animConfig)
  const {
    shellClassName,
    scrimClassName,
    shellStyle,
    onShellAnimationEnd,
    isExiting,
  } = presence

  const toolbar = isWebKit
    ? createPortal(fullscreenToolbar, document.body)
    : fullscreenToolbar

  const rootProps =
    shellAttr === 'data-tapp-run-shell'
      ? { 'data-tapp-run-shell': '' as const }
      : { 'data-tapp-store-shell': '' as const }

  return (
    <div
      {...rootProps}
      style={{
        position: 'fixed',
        top: 0,
        right: 0,
        bottom: 0,
        left: 0,
        overflow: 'hidden',
        zIndex: 40,
        pointerEvents: isExiting ? 'none' : undefined,
      }}
    >
      {toolbar}

      {scrimClassName ? (
        <div className={scrimClassName} style={shellStyle} aria-hidden />
      ) : null}

      <div className="pointer-events-none absolute inset-0 z-[1] flex flex-col overflow-hidden">
        <div
          className={`shrink-0 transition-[height,opacity] duration-200 ease-out ${
            isFullscreen ? 'h-0 opacity-0' : 'h-20 opacity-100'
          }`}
          aria-hidden
        />

        <div
          className={`flex min-h-0 flex-1 flex-col ${TAPP_SHELL_CHROME_PAD_CLASS}`}
          style={{
            paddingBottom: isFullscreen
              ? 0
              : isMobile
                ? TAPP_SHELL_CHROME_PB_MOBILE
                : TAPP_SHELL_CHROME_PB_DESKTOP,
          }}
        >
          <div
            className={`mx-auto flex min-h-0 w-full max-w-6xl flex-1 flex-col ${shellClassName}`}
            style={shellStyle}
            onAnimationEnd={onShellAnimationEnd}
          >
            <div
              className={`glass pointer-events-auto flex shrink-0 items-center justify-between gap-2 overflow-hidden rounded-t-xl px-3 shadow-sm transition-[opacity,height,padding,min-height] duration-200 ease-out ${
                isFullscreen
                  ? 'pointer-events-none h-0 min-h-0 py-0 opacity-0'
                  : 'h-auto min-h-11 py-2 opacity-100'
              }`}
              aria-hidden={isFullscreen}
            >
              <div className="flex min-w-0 items-center gap-2">
                <motion.button
                  onClick={onBack}
                  className="shrink-0 rounded-lg p-1.5 text-gray-500 transition-colors hover:bg-gray-100 hover:text-gray-700 dark:hover:bg-neutral-700 dark:hover:text-gray-300"
                  title={backTitle}
                  aria-label={backAriaLabel}
                  whileHover={noAnimation ? undefined : { scale: 1.1, x: -2 }}
                  whileTap={noAnimation ? undefined : { scale: 0.9 }}
                >
                  <FaArrowLeft className="h-4 w-4" />
                </motion.button>
                {headerLeading}
              </div>
              {headerActions}
            </div>

            <div className="relative min-h-0 flex-1">
              <div
                className={tappShellContentClass(
                  isFullscreen,
                  contentClassName,
                )}
                style={contentStyle}
              >
                {children}
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  )
}
