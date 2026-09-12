import type { LibraryCanvasTransform } from '../../utils/libraryCanvas'

import { useCallback, useEffect, useState, useSyncExternalStore } from 'react'
import { createPortal } from 'react-dom'
import {
  getNavLayoutSnapshot,
  getServerNavLayoutSnapshot,
  subscribeNavLayout,
} from '../../utils/navLayout'

function injectLibraryStyle(id: string, css: string) {
  if (typeof document === 'undefined') return
  let style = document.getElementById(id) as HTMLStyleElement | null
  if (!style) {
    style = document.createElement('style')
    style.id = id
    document.head.appendChild(style)
  }
  style.textContent = css
}
injectLibraryStyle(
  'library-canvas-chrome-styles',
  `
        /* 画布虚拟化会按视口装卸卡片；外层禁用 fadeInUp（焦点 scale 占用 transform）。 */
        .library-canvas-world .library-card-container {
            animation: none;
            transition: transform 120ms ease-out;
            /* 静止不占合成层；拖拽时再开 will-change，避免几十张卡常驻 GPU 内存 */
        }

        /*
         * 首次揭示入场挂在内层 shell（不碰外层 focus scale）。
         * 回扫已见 id / 拖拽中静默挂载，避免虚拟化重播与平移时弹入。
         */
        @keyframes library-canvas-shell-enter {
            from {
                opacity: 0;
                transform: scale(0.96);
            }
            to {
                opacity: 1;
                transform: scale(1);
            }
        }

        .library-canvas-world .library-card-shell[data-canvas-enter='1'] {
            animation: library-canvas-shell-enter 0.62s cubic-bezier(0.22, 1, 0.36, 1) both;
        }

        @media (prefers-reduced-motion: reduce) {
            .library-canvas-world .library-card-shell[data-canvas-enter='1'] {
                animation: none;
                opacity: 1;
                transform: none;
            }
        }

        [data-library-canvas-surface='true'][data-dragging='true'] {
            cursor: grabbing;
        }

        [data-library-canvas-surface='true'][data-dragging='true'] .library-canvas-world {
            will-change: transform;
        }

        [data-library-canvas-surface='true'][data-dragging='true'] .library-card-container {
            will-change: transform;
            transition: none;
        }

        [data-library-canvas-surface='true'],
        [data-library-canvas-surface='true'] .library-card-container {
            -webkit-user-select: none;
            user-select: none;
        }

        [data-library-canvas-surface='true'] .library-card-container img {
            -webkit-user-drag: none;
            user-select: none;
        }

        /*
         * 无限画布本身已响应指针；背景强制静止，避免双重位移与点击涟漪干扰。
         * transform 过渡由 useEvocativeWallpaper soft-lock 写入（需能先 transition:none
         * 缓存 from 帧再缓入 identity；此处勿 !important，否则 from 帧插值被掐断）。
         * 退出画布后 soft-restore 缓回 scale。
         */
        html[data-library-canvas='active'] #wallpaper {
            animation: none !important;
        }

        html[data-library-canvas='active'] #wallpaper-ripple-canvas {
            /* 长时画布会话用 display 省合成；恢复时 canvas 会重建并从 opacity 0 淡入 */
            display: none !important;
        }

        html[data-library-canvas='active'] #wallpaper-awaiting-fx,
        html[data-library-canvas='active'] #wallpaper-awaiting-fx::after {
            animation: none !important;
        }
  `,
)

const CANVAS_HINT_SESSION_KEY = 'library-canvas-hint-dismissed'

interface LibraryCanvasChromeProps {
  ariaLabel: string
  atMaxZoom: boolean
  atMinZoom: boolean
  dismissHintLabel: string
  hint: string
  mobileHint?: string
  isDefault: boolean
  onReset: () => void
  onZoom: (factor: number) => void
  resetLabel: string
  zoomInLabel: string
  zoomOutLabel: string
  zoomPercent: number
}

export function LibraryCanvasChrome({
  ariaLabel,
  atMaxZoom,
  atMinZoom,
  dismissHintLabel,
  hint,
  mobileHint,
  isDefault,
  onReset,
  onZoom,
  resetLabel,
  zoomInLabel,
  zoomOutLabel,
  zoomPercent,
}: LibraryCanvasChromeProps) {
  const [showHint, setShowHint] = useState<boolean | null>(null)
  const navLayout = useSyncExternalStore(
    subscribeNavLayout,
    getNavLayoutSnapshot,
    getServerNavLayoutSnapshot,
  )
  const isMobile = navLayout === 'mobile'

  useEffect(() => {
    // 移动端不要显示顶部平移/缩放提示。
    if (isMobile) {
      setShowHint(false)
      return
    }
    try {
      setShowHint(
        window.sessionStorage.getItem(CANVAS_HINT_SESSION_KEY) !== '1',
      )
    } catch {
      setShowHint(true)
    }
  }, [isMobile])

  const dismissHint = useCallback(() => {
    setShowHint(false)
    try {
      window.sessionStorage.setItem(CANVAS_HINT_SESSION_KEY, '1')
    } catch {
    }
  }, [])

  if (typeof document === 'undefined') return null

  const hintText =
    isMobile && mobileHint && mobileHint.trim().length > 0 ? mobileHint : hint

  return createPortal(
    <>
      {showHint === true && !isMobile && (
        <div
          className="pointer-events-none fixed inset-x-0 z-40 flex justify-center px-3 sm:px-16 md:px-24"
          style={{
            top: 'max(0.75rem, env(safe-area-inset-top, 0px))',
          }}
        >
          <div className="pointer-events-auto glass flex max-w-[min(100%,28rem)] items-center gap-1 rounded-full py-1 pr-1 pl-3 text-[10px] leading-snug text-gray-600 shadow-sm dark:text-gray-300 sm:max-w-none sm:text-[11px]">
            <span className="min-w-0">{hintText}</span>
            <button
              type="button"
              className="flex h-6 w-6 shrink-0 items-center justify-center rounded-full text-gray-500 hover:bg-black/5 hover:text-gray-900 dark:text-gray-400 dark:hover:bg-white/10 dark:hover:text-white"
              onClick={dismissHint}
              aria-label={dismissHintLabel}
              title={dismissHintLabel}
            >
              <svg
                className="h-3.5 w-3.5"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                aria-hidden="true"
              >
                <path
                  d="m7 7 10 10M17 7 7 17"
                  strokeWidth="2"
                  strokeLinecap="round"
                />
              </svg>
            </button>
          </div>
        </div>
      )}
      <div
        data-tour="library-canvas"
        className={
          isMobile
            ? 'fixed z-40 flex flex-col items-center gap-0.5 rounded-xl border border-white/35 glass p-1 shadow-xl dark:border-white/10'
            : 'fixed left-1/2 z-40 flex -translate-x-1/2 items-center gap-0.5 rounded-xl border border-white/35 glass p-1 shadow-xl dark:border-white/10'
        }
        style={
          isMobile
            ? {
                top: 'calc((100dvh - env(safe-area-inset-bottom, 0px) - 5.75rem + env(safe-area-inset-top, 0px)) / 2)',
                right: 'max(0.75rem, env(safe-area-inset-right, 0px))',
                transform: 'translateY(-50%)',
              }
            : {
                bottom: 'calc(env(safe-area-inset-bottom, 0px) + 1.5rem)',
              }
        }
        role="toolbar"
        aria-label={ariaLabel}
      >
        <button
          type="button"
          data-library-canvas-zoom-out=""
          className="flex h-8 w-8 items-center justify-center rounded-lg text-gray-600 transition-colors hover:bg-white/65 hover:text-gray-950 active:bg-white/80 disabled:cursor-not-allowed disabled:opacity-35 dark:text-gray-300 dark:hover:bg-white/10 dark:hover:text-white"
          onClick={() => onZoom(1 / 1.16)}
          disabled={atMinZoom}
          title={`${zoomOutLabel} (-)`}
          aria-label={zoomOutLabel}
        >
          <svg
            className="h-4 w-4"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            aria-hidden="true"
          >
            <path d="M5 12h14" strokeWidth="2" strokeLinecap="round" />
          </svg>
        </button>
        <output
          data-library-canvas-zoom-percent=""
          className={
            isMobile
              ? 'min-w-8 select-none py-0.5 text-center text-[10px] font-semibold tabular-nums text-gray-700 dark:text-gray-200'
              : 'min-w-11 select-none text-center text-[11px] font-semibold tabular-nums text-gray-700 dark:text-gray-200'
          }
          aria-live="polite"
        >
          {zoomPercent}%
        </output>
        <button
          type="button"
          data-library-canvas-zoom-in=""
          className="flex h-8 w-8 items-center justify-center rounded-lg text-gray-600 transition-colors hover:bg-white/65 hover:text-gray-950 active:bg-white/80 disabled:cursor-not-allowed disabled:opacity-35 dark:text-gray-300 dark:hover:bg-white/10 dark:hover:text-white"
          onClick={() => onZoom(1.16)}
          disabled={atMaxZoom}
          title={`${zoomInLabel} (+)`}
          aria-label={zoomInLabel}
        >
          <svg
            className="h-4 w-4"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            aria-hidden="true"
          >
            <path d="M12 5v14M5 12h14" strokeWidth="2" strokeLinecap="round" />
          </svg>
        </button>
        <span
          className={
            isMobile
              ? 'mx-0 h-px w-4 bg-gray-900/10 dark:bg-white/15'
              : 'mx-0.5 h-4 w-px bg-gray-900/10 dark:bg-white/15'
          }
          aria-hidden="true"
        />
        <button
          type="button"
          data-library-canvas-reset=""
          className={
            isMobile
              ? 'flex h-8 w-8 items-center justify-center rounded-lg text-gray-600 transition-colors hover:bg-white/65 hover:text-gray-950 active:bg-white/80 disabled:cursor-default disabled:opacity-35 dark:text-gray-300 dark:hover:bg-white/10 dark:hover:text-white'
              : 'flex h-8 items-center justify-center gap-1.5 rounded-lg px-2 text-[11px] font-medium text-gray-600 transition-colors hover:bg-white/65 hover:text-gray-950 active:bg-white/80 disabled:cursor-default disabled:opacity-35 dark:text-gray-300 dark:hover:bg-white/10 dark:hover:text-white'
          }
          onClick={onReset}
          disabled={isDefault}
          title={`${resetLabel} (0)`}
          aria-label={resetLabel}
        >
          <svg
            className="h-4 w-4"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            aria-hidden="true"
          >
            <path
              d="M8 3H5a2 2 0 0 0-2 2v3m13-5h3a2 2 0 0 1 2 2v3M8 21H5a2 2 0 0 1-2-2v-3m13 5h3a2 2 0 0 0 2-2v-3M12 8v8m-4-4h8"
              strokeWidth="1.8"
              strokeLinecap="round"
              strokeLinejoin="round"
            />
          </svg>
          {!isMobile && <span>{resetLabel}</span>}
        </button>
      </div>
    </>,
    document.body,
  )
}

interface LibraryCanvasChromePaint {
  zoomLabel: HTMLElement
  zoomOut: HTMLButtonElement | null
  zoomIn: HTMLButtonElement | null
  resetBtn: HTMLButtonElement | null
}

let chromePaint: LibraryCanvasChromePaint | null = null

function readLibraryCanvasChromePaint(): LibraryCanvasChromePaint | null {
  if (chromePaint?.zoomLabel.isConnected) return chromePaint
  const zoomLabel = document.querySelector<HTMLElement>(
    '[data-library-canvas-zoom-percent]',
  )
  if (!zoomLabel) {
    chromePaint = null
    return null
  }
  chromePaint = {
    zoomLabel,
    zoomOut: document.querySelector<HTMLButtonElement>(
      '[data-library-canvas-zoom-out]',
    ),
    zoomIn: document.querySelector<HTMLButtonElement>(
      '[data-library-canvas-zoom-in]',
    ),
    resetBtn: document.querySelector<HTMLButtonElement>(
      '[data-library-canvas-reset]',
    ),
  }
  return chromePaint
}

export function syncLibraryCanvasChrome(
  t: LibraryCanvasTransform,
  opts: {
    minScale: number
    maxScale: number
    defaultScale: number
  },
) {
  const chrome = readLibraryCanvasChromePaint()
  if (!chrome) return
  const label = `${Math.round(t.scale * 100)}%`
  if (chrome.zoomLabel.textContent !== label) {
    chrome.zoomLabel.textContent = label
  }
  const outDisabled = t.scale <= opts.minScale + 0.001
  if (chrome.zoomOut && chrome.zoomOut.disabled !== outDisabled) {
    chrome.zoomOut.disabled = outDisabled
  }
  const inDisabled = t.scale >= opts.maxScale - 0.001
  if (chrome.zoomIn && chrome.zoomIn.disabled !== inDisabled) {
    chrome.zoomIn.disabled = inDisabled
  }
  if (chrome.resetBtn) {
    const isDefault =
      Math.abs(t.x) < 0.5 &&
      Math.abs(t.y) < 0.5 &&
      Math.abs(t.scale - opts.defaultScale) < 0.001
    if (chrome.resetBtn.disabled !== isDefault) {
      chrome.resetBtn.disabled = isDefault
    }
  }
}
