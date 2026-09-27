import type { WidgetConfig } from './widgetGridTypes'
import {
  AnimatePresenceShim as AnimatePresence,
  motionShim as motion,
} from '@lib/motionShim'
import { memo, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'
import { useI18n } from '../contexts/I18nContext'
import { isExlight } from '../hooks/useAnimationLevel'

import { emitAppEvent } from '../utils/appEvents'
import { ReportCardWidget } from './widgets/ReportCardWidget'

const DARK_ORIGINAL_BG =
  'linear-gradient(to bottom, transparent 0%, transparent 35%, rgba(10, 10, 10, 0.3) 45%, rgba(10, 10, 10, 0.5) 55%, rgba(10, 10, 10, 0.75) 70%, rgba(10, 10, 10, 0.9) 85%, rgba(10, 10, 10, 0.95) 100%)'
const LIGHT_ORIGINAL_BG =
  'linear-gradient(to bottom, transparent 0%, transparent 35%, rgba(255, 255, 255, 0.4) 55%, rgba(255, 255, 255, 0.9) 85%, rgba(255, 255, 255, 0.9) 100%)'
const FEATHER_RANGE = 120

function getOriginalBackground(isDark: boolean) {
  return isDark ? DARK_ORIGINAL_BG : LIGHT_ORIGINAL_BG
}

function buildGradientLayer(
  isDark: boolean,
  easeProgress: number,
  isEnteringPhase: boolean,
) {
  const baseColorStops = isDark
    ? [
        'rgba(10, 10, 10, 0.98)',
        'rgba(10, 10, 10, 0.9)',
        'rgba(10, 10, 10, 0.6)',
        'rgba(10, 10, 10, 0.2)',
        'transparent',
      ]
    : [
        'rgba(255, 255, 255, 0.98)',
        'rgba(255, 255, 255, 0.9)',
        'rgba(255, 255, 255, 0.6)',
        'rgba(255, 255, 255, 0.2)',
        'transparent',
      ]

  const reverseStops = isDark
    ? [
        'transparent',
        'rgba(10, 10, 10, 0.2)',
        'rgba(10, 10, 10, 0.6)',
        'rgba(10, 10, 10, 0.9)',
        'rgba(10, 10, 10, 0.98)',
      ]
    : [
        'transparent',
        'rgba(255, 255, 255, 0.2)',
        'rgba(255, 255, 255, 0.6)',
        'rgba(255, 255, 255, 0.9)',
        'rgba(255, 255, 255, 0.98)',
      ]

  const pos = easeProgress * (100 + FEATHER_RANGE) - FEATHER_RANGE

  if (isEnteringPhase) {
    return `linear-gradient(to top, ${baseColorStops[0]} ${pos}%, ${baseColorStops[1]} ${pos + FEATHER_RANGE * 0.2}%, ${baseColorStops[2]} ${pos + FEATHER_RANGE * 0.5}%, ${baseColorStops[3]} ${pos + FEATHER_RANGE * 0.8}%, ${baseColorStops[4]} ${pos + FEATHER_RANGE}%)`
  }

  return `linear-gradient(to bottom, ${reverseStops[0]} ${pos}%, ${reverseStops[1]} ${pos + FEATHER_RANGE * 0.2}%, ${reverseStops[2]} ${pos + FEATHER_RANGE * 0.5}%, ${reverseStops[3]} ${pos + FEATHER_RANGE * 0.8}%, ${reverseStops[4]} ${pos + FEATHER_RANGE}%)`
}

function applyStageGradient(
  element: HTMLElement,
  isDark: boolean,
  easeProgress: number,
  isEnteringPhase: boolean,
) {
  const gradientLayer = buildGradientLayer(
    isDark,
    easeProgress,
    isEnteringPhase,
  )
  const originalBg = getOriginalBackground(isDark)
  element.style.setProperty(
    'background-image',
    `${gradientLayer}, ${originalBg}`,
    'important',
  )
}

function getBlurAmount(easeProgress: number, isEnteringPhase: boolean) {
  return isEnteringPhase ? easeProgress * 17 : (1 - easeProgress) * 17
}

const CURTAIN_MS = 1000
const DARK_CURTAIN_CLASS =
  'absolute inset-0 transition-opacity duration-500 ease-out'

type CurtainPhase = 'enter' | 'exit'

const curtain = {
  raf: 0,
  start: null as number | null,
  phase: 'enter' as CurtainPhase,
  progress: 0,
  baseClass: null as string | null,
  getIsDark: () => false,
}

function easeInOutCubic(progress: number) {
  return progress < 0.5
    ? 4 * progress * progress * progress
    : 1 - (-2 * progress + 2) ** 3 / 2
}

function stageBg(): HTMLElement | null {
  if (typeof document === 'undefined') return null
  return document.getElementById('bg-gradient') as HTMLElement | null
}

function rememberCurtainBase(el: HTMLElement) {
  if (!curtain.baseClass) curtain.baseClass = el.className
}

function applyCurtainClass(el: HTMLElement, isDark: boolean) {
  if (isDark) {
    el.className = DARK_CURTAIN_CLASS
  } else if (curtain.baseClass) {
    el.className = curtain.baseClass
  }
}

function paintCurtain(
  el: HTMLElement,
  easeProgress: number,
  entering: boolean,
) {
  applyStageGradient(el, curtain.getIsDark(), easeProgress, entering)
  const blurAmount = isExlight()
    ? 0
    : getBlurAmount(easeProgress, entering)
  el.style.backdropFilter = blurAmount > 0.5 ? `blur(${blurAmount}px)` : ''
}

function finishCurtainExit(el: HTMLElement) {
  el.style.removeProperty('background-image')
  el.style.backdropFilter = ''
  el.style.transition = ''
  if (curtain.baseClass) el.className = curtain.baseClass
  curtain.progress = 0
  curtain.phase = 'enter'
  curtain.start = null
}

function curtainTick(timestamp: number) {
  const el = stageBg()
  if (!el) {
    curtain.raf = 0
    return
  }
  if (curtain.start === null) curtain.start = timestamp
  const linear = Math.min((timestamp - curtain.start) / CURTAIN_MS, 1)
  const eased = easeInOutCubic(linear)
  curtain.progress = eased
  const entering = curtain.phase === 'enter'
  paintCurtain(el, eased, entering)
  if (linear < 1) {
    curtain.raf = requestAnimationFrame(curtainTick)
    return
  }
  curtain.raf = 0
  if (!entering) finishCurtainExit(el)
}

function startCurtain(phase: CurtainPhase) {
  const el = stageBg()
  if (!el) return
  rememberCurtainBase(el)
  if (curtain.raf) cancelAnimationFrame(curtain.raf)
  curtain.phase = phase
  curtain.start = null
  curtain.progress = 0
  el.style.transition = 'none'
  if (phase === 'enter') applyCurtainClass(el, curtain.getIsDark())
  curtain.raf = requestAnimationFrame(curtainTick)
}

function playStageCurtainEnter(getIsDark: () => boolean) {
  curtain.getIsDark = getIsDark
  startCurtain('enter')
}

function playStageCurtainExit() {
  const el = stageBg()
  if (!el) return
  rememberCurtainBase(el)
  if (curtain.phase === 'exit' && curtain.raf) return
  if (!el.style.backgroundImage && !curtain.raf) return
  startCurtain('exit')
}

function isCurtainBusy() {
  return curtain.raf !== 0
}

function syncStageCurtainTheme(isDark: boolean, active: boolean) {
  curtain.getIsDark = () => isDark
  const el = stageBg()
  if (!el) return
  rememberCurtainBase(el)
  if (!active) {
    // 光幕还在或退出扫描未开始时不要还原 class，否则扫描层会闪掉。
    if (
      !isCurtainBusy() &&
      !el.style.backgroundImage &&
      curtain.baseClass
    ) {
      el.className = curtain.baseClass
    }
    return
  }
  applyCurtainClass(el, isDark)
  paintCurtain(el, curtain.progress, curtain.phase === 'enter')
}

const CONTROL_CHARS_REGEX = /[\u0000-\u001F\u007F-\u009F]/g
const MARKDOWN_SYMBOLS_REGEX = /[*_~`]/g
const WHITESPACE_REGEX = /\s+/g
const PUNCTUATION_SPLIT_REGEX = /([。！？.!?，,])/g

function cleanText(text: string): string {
  return text
    .replaceAll(CONTROL_CHARS_REGEX, '')
    .replaceAll(MARKDOWN_SYMBOLS_REGEX, '')
    .replaceAll(WHITESPACE_REGEX, ' ')
    .trim()
}

function splitByPunctuation(text: string) {
  return text
    .replaceAll(PUNCTUATION_SPLIT_REGEX, '$1\uFFFF')
    .split('\uFFFF')
    .map((s) => s.trim())
    .filter((s) => s.length > 0)
}

interface SubtitleLine {
  text: string
  delay: number
}

interface StageChapter {
  title: string
  lines: SubtitleLine[]
}

interface StageModeProps {
  isOpen: boolean
  onClose: () => void
  reportData: {
    platform?: string
    summary?: string
    insights?: string[]
    card_visuals?: any
    type?: 'platform'
  } | null
  playAllMode?: boolean
}

const REPORT_CARD_BASE_WIDTH = 308
const REPORT_CARD_BASE_HEIGHT = REPORT_CARD_BASE_WIDTH / 2

const StageScaledReportCard = memo(({
  config,
  data,
  showOverview,
}: {
  config: WidgetConfig
  data: any
  showOverview: boolean
}) => {
  const containerRef = useRef<HTMLDivElement>(null)
  const [scale, setScale] = useState(1)

  useEffect(() => {
    const el = containerRef.current
    if (!el) return

    const updateScale = () => {
      const width = el.clientWidth
      if (width > 0) {
        setScale(width / REPORT_CARD_BASE_WIDTH)
      }
    }

    updateScale()
    const ro = new ResizeObserver(updateScale)
    ro.observe(el)
    return () => ro.disconnect()
  }, [])

  return (
    <div
      ref={containerRef}
      className="relative w-full max-w-sm md:max-w-md aspect-2/1 rounded-2xl overflow-hidden glass shadow-xl"
    >
      <div
        className="absolute left-0 top-0 origin-top-left will-change-transform"
        style={{
          width: REPORT_CARD_BASE_WIDTH,
          height: REPORT_CARD_BASE_HEIGHT,
          transform: `scale(${scale})`,
        }}
      >
        <ReportCardWidget
          config={config}
          isEditMode={false}
          data={data}
          bare
          showOverview={showOverview}
        />
      </div>
    </div>
  )
})

function SubtitleDisplay({
  lines,
  isActive,
  isPaused,
  rightContent,
}: {
  lines: SubtitleLine[]
  isActive: boolean
  isPaused?: boolean
  rightContent?: React.ReactNode
}) {
  const [visibleLines, setVisibleLines] = useState<
    { id: number; text: string }[]
  >([])

  useEffect(() => {
    if (!isActive || lines.length === 0 || isPaused) {
      if (isPaused) return
      setVisibleLines([])
      return
    }

    setVisibleLines([])

    let mounted = true
    let timer: NodeJS.Timeout

    const showNextLine = (index: number) => {
      if (index >= lines.length || !mounted) return

      const line = lines[index]
      const delay = index === 0 ? 500 : line.delay

      timer = setTimeout(() => {
        if (!mounted) return

        setVisibleLines((prev) => {
          const next = [...prev, { id: index, text: line.text }]
          return next.slice(-5)
        })
        showNextLine(index + 1)
      }, delay)
    }

    showNextLine(0)

    return () => {
      mounted = false
      clearTimeout(timer)
    }
  }, [lines, isActive, isPaused])

  return (
    <div className="w-full h-full flex flex-row pointer-events-none">
      <div className="w-[62%] md:w-[60%] h-full flex flex-col justify-end items-start pl-2 md:pl-6 overflow-hidden pb-8">
        <AnimatePresence mode="popLayout">
          {visibleLines.map((line) => (
            <motion.div
              key={line.id}
              layout
              initial={{ opacity: 0, x: -20, filter: 'blur(8.5px)' }}
              animate={{ opacity: 1, x: 0, filter: 'blur(0px)' }}
              exit={{ opacity: 0, y: -20, filter: 'blur(4.2px)' }}
              transition={{
                duration: 0.8,
                ease: [0.16, 1, 0.3, 1],
                layout: { duration: 0.5 },
              }}
              className="text-left mb-4 md:mb-8 last:mb-0 w-full"
            >
              <p
                className="text-2xl md:text-4xl font-bold text-gray-900 dark:text-gray-100 leading-tight tracking-widest"
                style={{
                  fontFamily: '"Noto Sans SC", sans-serif',
                }}
              >
                {line.text}
              </p>
            </motion.div>
          ))}
        </AnimatePresence>
      </div>
      <div className="w-[38%] md:w-[40%] h-full flex items-center justify-center p-2 md:p-6 pointer-events-auto">
        {rightContent}
      </div>
    </div>
  )
}

function parseReportToChapters(
  reportData: {
    summary: string
    insights: string[]
  },
  chapterTitles: { dataEcho: string; deepInsight: string },
): StageChapter[] {
  const chapters: StageChapter[] = []

  if (reportData.summary) {
    const summaryText = cleanText(reportData.summary)
    const sentences = splitByPunctuation(summaryText)

    chapters.push({
      title: chapterTitles.dataEcho,
      lines: sentences.map((sentence, i) => ({
        text: sentence,
        delay: i === 0 ? 800 : 1500,
      })),
    })
  }

  if (reportData.insights && reportData.insights.length > 0) {
    const insights = reportData.insights
      .map(cleanText)
      .filter((s) => s.length > 0)

    const allInsightLines: SubtitleLine[] = []
    insights.forEach((insight, i) => {
      const parts = splitByPunctuation(insight)
      parts.forEach((part, j) => {
        allInsightLines.push({
          text: part,
          delay: i === 0 && j === 0 ? 800 : 1500,
        })
      })
    })

    chapters.push({
      title: chapterTitles.deepInsight,
      lines: allInsightLines,
    })
  }

  return chapters
}

export default function StageMode({
  isOpen,
  onClose,
  reportData,
  playAllMode = false,
}: StageModeProps) {
  const { t } = useI18n()
  const [currentChapter, setCurrentChapter] = useState(0)
  const [chapters, setChapters] = useState<StageChapter[]>([])
  const [isPaused, setIsPaused] = useState(false)
  const [isDarkMode, setIsDarkMode] = useState(() => {
    if (typeof document === 'undefined') return false
    return document.documentElement.classList.contains('dark')
  })
  const isDarkModeRef = useRef(isDarkMode)

  const showOverview = useMemo(() => {
    if (!chapters[currentChapter]) return true
    const title = chapters[currentChapter].title
    return title !== t.reportsPage.deepInsight
  }, [currentChapter, chapters, t.reportsPage.deepInsight])

  const stageWidgetConfig = useMemo((): WidgetConfig | null => {
    if (!reportData?.platform) return null
    return {
      config: { platformId: reportData.platform },
    } as WidgetConfig
  }, [reportData?.platform])

  const renderWidget = () => {
    if (!reportData || !stageWidgetConfig) return null

    return (
      <StageScaledReportCard
        config={stageWidgetConfig}
        data={reportData.card_visuals}
        showOverview={showOverview}
      />
    )
  }

  useEffect(() => {
    const handleTogglePause = () => {
      setIsPaused((prev) => !prev)
    }

    window.addEventListener('stage-toggle-pause', handleTogglePause)
    return () => {
      window.removeEventListener('stage-toggle-pause', handleTogglePause)
    }
  }, [])

  useEffect(() => {
    emitAppEvent('stage-pause-state-change', { isPaused })
  }, [isPaused])

  useEffect(() => {
    if (reportData) {
      let parsedChapters: StageChapter[] = []
      const chapterTitles = {
        dataEcho: t.reportsPage.dataEcho,
        deepInsight: t.reportsPage.deepInsight,
      }

      if (reportData.summary && reportData.insights) {
        parsedChapters = parseReportToChapters(
          {
            summary: reportData.summary,
            insights: reportData.insights,
          },
          chapterTitles,
        )
      }

      setChapters(parsedChapters)
      setCurrentChapter(0)
    }
  }, [reportData, t.reportsPage.dataEcho, t.reportsPage.deepInsight])

  useEffect(() => {
    if (typeof document === 'undefined') return
    const root = document.documentElement
    if (!root) return

    const getIsDark = () => root.classList.contains('dark')

    const syncMode = () => {
      const next = getIsDark()
      setIsDarkMode((prev) => (prev === next ? prev : next))
    }

    const observer = new MutationObserver((mutations) => {
      for (const mutation of mutations) {
        if (mutation.attributeName === 'class') {
          syncMode()
          break
        }
      }
    })

    observer.observe(root, { attributes: true, attributeFilter: ['class'] })

    const supportsMatchMedia =
      typeof window !== 'undefined' && typeof window.matchMedia === 'function'
    const mediaQuery = supportsMatchMedia
      ? window.matchMedia('(prefers-color-scheme: dark)')
      : null
    const handleMediaChange = () => syncMode()

    if (mediaQuery) {
      mediaQuery.addEventListener('change', handleMediaChange)
    }

    syncMode()

    return () => {
      observer.disconnect()
      if (mediaQuery) {
        mediaQuery.removeEventListener('change', handleMediaChange)
      }
    }
  }, [])

  useEffect(() => {
    isDarkModeRef.current = isDarkMode
  }, [isDarkMode])

  useEffect(() => {
    syncStageCurtainTheme(isDarkMode, isOpen)
  }, [isDarkMode, isOpen])

  const onCloseRef = useRef(onClose)
  useEffect(() => {
    onCloseRef.current = onClose
  }, [onClose])

  useEffect(() => {
    if (!isOpen || chapters.length === 0 || isPaused) return

    const currentChapterData = chapters[currentChapter]
    if (!currentChapterData) return

    const subtitleDuration = currentChapterData.lines.reduce(
      (sum, line, i) => sum + (i === 0 ? 500 : line.delay),
      0,
    )

    let bufferTime = 2500 + currentChapterData.lines.length * 200

    if (currentChapterData.title === t.reportsPage.deepInsight) {
      const minDuration = 12000
      if (subtitleDuration + bufferTime < minDuration) {
        bufferTime = minDuration - subtitleDuration
      }
    } else if (currentChapter === chapters.length - 1) {
      bufferTime += 3000
    }

    const totalDuration = subtitleDuration + bufferTime

    const timer = setTimeout(() => {
      if (currentChapter < chapters.length - 1) {
        setCurrentChapter((prev) => prev + 1)
      } else {
        emitAppEvent('stage-playback-complete')
        if (!playAllMode) {
          onCloseRef.current()
        }
      }
    }, totalDuration)

    return () => clearTimeout(timer)
  }, [
    isOpen,
    currentChapter,
    chapters,
    isPaused,
    playAllMode,
    t.reportsPage.deepInsight,
  ])

  // 卸载时不能取消还在播的退出扫描。
  useLayoutEffect(() => {
    if (isOpen) {
      playStageCurtainEnter(() => isDarkModeRef.current)
      return
    }
    playStageCurtainExit()
  }, [isOpen])

  useLayoutEffect(() => {
    return () => {
      const stillOnReports =
        typeof window !== 'undefined' &&
        window.location.pathname === '/reports'
      if (stillOnReports) return
      playStageCurtainExit()
    }
  }, [])

  if (!reportData) return null

  return (
    <AnimatePresence mode="wait">
      {isOpen && (
        <motion.div
          className="fixed inset-0 z-40 pointer-events-none"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.5 }}
        >
            <div className="h-full flex flex-col pt-20 pb-6 px-3 xs:px-4 sm:px-6">
              <div className="flex-1 max-w-7xl mx-auto w-full">
                <div className="h-[65%] md:h-[45%] relative">
                  <div className="absolute top-0 right-0 z-50">
                    <div className="glass-surface glass-80 rounded-xl px-4 py-2 shadow-lg border border-gray-100 dark:border-neutral-700">
                      <div className="flex items-center gap-3">
                        <div className="flex flex-col items-end">
                          <div className="text-xs text-gray-500 dark:text-gray-400 font-mono">
                            ACT {currentChapter + 1}/{chapters.length}
                          </div>
                          <div className="text-sm font-bold text-gray-900 dark:text-gray-100">
                            {chapters[currentChapter]?.title || ''}
                          </div>
                        </div>
                        <div className="flex gap-1">
                          {chapters.map((_, i) => (
                            <div
                              key={i}
                              className={`w-1.5 h-1.5 rounded-full transition-all ${
                                i <= currentChapter
                                  ? 'bg-neutral-900 dark:bg-neutral-100 shadow-sm'
                                  : 'bg-gray-300 dark:bg-neutral-700'
                              }`}
                            />
                          ))}
                        </div>
                      </div>
                    </div>
                  </div>

                  <div className="absolute inset-0 pt-0 flex items-center">
                    <AnimatePresence mode="wait">
                      {chapters[currentChapter] && (
                        <motion.div
                          key={currentChapter}
                          initial={{ opacity: 0 }}
                          animate={{ opacity: 1 }}
                          exit={{ opacity: 0 }}
                          transition={{ duration: 0.5 }}
                          className="w-full h-full"
                        >
                          <SubtitleDisplay
                            lines={chapters[currentChapter].lines}
                            isActive={isOpen}
                            isPaused={isPaused}
                            rightContent={renderWidget()}
                          />
                        </motion.div>
                      )}
                    </AnimatePresence>
                  </div>
                </div>
              </div>
            </div>
        </motion.div>
      )}
    </AnimatePresence>
  )
}
