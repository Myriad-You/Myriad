import type { WidgetConfig } from './widgetGridTypes'
import {
  AnimatePresenceShim as AnimatePresence,
  motionShim as motion,
} from '@lib/motionShim'
import { memo, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'
import { useI18n } from '../contexts/I18nContext'
import { isExlight } from '../hooks/useAnimationLevel'

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
  return isEnteringPhase ? easeProgress * 20 : (1 - easeProgress) * 20
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
    // 光幕还在或退出扫描未开始时不要还原 class，否则会把扫描层闪掉
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

// 预编译正则表达式（避免每次调用时重新创建）
const CONTROL_CHARS_REGEX = /[\u0000-\u001F\u007F-\u009F]/g
const MARKDOWN_SYMBOLS_REGEX = /[*_~`]/g
const WHITESPACE_REGEX = /\s+/g
const PUNCTUATION_SPLIT_REGEX = /([。！？.!?，,])/g

// 共享文本处理工具函数
function cleanText(text: string): string {
  return text
    .replace(CONTROL_CHARS_REGEX, '')
    .replace(MARKDOWN_SYMBOLS_REGEX, '')
    .replace(WHITESPACE_REGEX, ' ')
    .trim()
}

function splitByPunctuation(text: string) {
  return text
    .replace(PUNCTUATION_SPLIT_REGEX, '$1\uFFFF')
    .split('\uFFFF')
    .map((s) => s.trim())
    .filter((s) => s.length > 0)
}

// 字幕行接口
interface SubtitleLine {
  text: string
  delay: number // 距离上一行的延迟（毫秒）
}

// 篇章接口
interface StageChapter {
  title: string
  lines: SubtitleLine[]
}

// 组件Props
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
  onRefresh?: () => void // 刷新当前报告的回调
  playAllMode?: boolean // 是否在播放全部模式下
}

/**
 * 报告页 4 列卡片基准宽度（max-w-7xl=80rem，3 个 1rem 间距）：
 * (1280 - 48) / 4 = 308px，宽高比 2:1。
 * 舞台模式卡片更大时，内部内容按此基准等比 scale，避免字号/间距相对偏小。
 */
const REPORT_CARD_BASE_WIDTH = 308
const REPORT_CARD_BASE_HEIGHT = REPORT_CARD_BASE_WIDTH / 2

/** 舞台模式专用：外层放大，内层按报告页卡片尺寸绘制后等比缩放 */
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

// 字幕显示组件
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
  const [_currentIndex, setCurrentIndex] = useState(0)

  useEffect(() => {
    if (!isActive || lines.length === 0 || isPaused) {
      if (isPaused) return // 暂停时保持当前状态
      setVisibleLines([])
      setCurrentIndex(0)
      return
    }

    // 重置状态
    setVisibleLines([])
    setCurrentIndex(0)

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
          // 只保留最后5条，控制同屏行数
          return next.slice(-5)
        })
        setCurrentIndex(index + 1)
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
              initial={{ opacity: 0, x: -20, filter: 'blur(10px)' }}
              animate={{ opacity: 1, x: 0, filter: 'blur(0px)' }}
              exit={{ opacity: 0, y: -20, filter: 'blur(5px)' }}
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
      {/* 右侧卡片区：与报告页卡片同款比例，移动端留足宽度避免裁切 */}
      <div className="w-[38%] md:w-[40%] h-full flex items-center justify-center p-2 md:p-6 pointer-events-auto">
        {rightContent}
      </div>
    </div>
  )
}

// 内容解析：将报告转换为篇章
function parseReportToChapters(
  reportData: {
    summary: string
    insights: string[]
  },
  chapterTitles: { dataEcho: string; deepInsight: string },
): StageChapter[] {
  const chapters: StageChapter[] = []

  // 第一篇章：总结
  if (reportData.summary) {
    const summaryText = cleanText(reportData.summary)
    const sentences = splitByPunctuation(summaryText)

    chapters.push({
      title: chapterTitles.dataEcho,
      lines: sentences.map((sentence, i) => ({
        text: sentence,
        delay: i === 0 ? 800 : 1500, // 缩短间隔以适应更短的句子
      })),
    })
  }

  // 第二篇章：深度洞察
  if (reportData.insights && reportData.insights.length > 0) {
    const insights = reportData.insights
      .map(cleanText)
      .filter((s) => s.length > 0)

    // 洞察可能也需要分句，如果太长的话
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

// 舞台模式主组件
export default function StageMode({
  isOpen,
  onClose,
  reportData,
  onRefresh: _onRefresh,
  playAllMode = false,
}: StageModeProps) {
  const { t } = useI18n()
  const [currentChapter, setCurrentChapter] = useState(0)
  const [chapters, setChapters] = useState<StageChapter[]>([])
  const [isPaused, setIsPaused] = useState(false) // 播放/暂停状态
  const [isDarkMode, setIsDarkMode] = useState(() => {
    if (typeof document === 'undefined') return false
    return document.documentElement.classList.contains('dark')
  })
  const isDarkModeRef = useRef(isDarkMode)

  // 自动切换概览/详情 - 基于篇章 (使用 useMemo 替代 useEffect 避免状态同步延迟)
  const showOverview = useMemo(() => {
    if (!chapters[currentChapter]) return true
    const title = chapters[currentChapter].title
    // 只有在明确是"深度洞察"时才显示内容，其他情况（包括"数据回想"或未知）都显示概览
    return title !== t.reportsPage.deepInsight
  }, [currentChapter, chapters, t.reportsPage.deepInsight])

  // 与报告页卡片共用同一 config 形状，避免 memo 无意义失效
  const stageWidgetConfig = useMemo((): WidgetConfig | null => {
    if (!reportData?.platform) return null
    return {
      config: { platformId: reportData.platform },
    } as WidgetConfig
  }, [reportData?.platform])

  const renderWidget = () => {
    if (!reportData || !stageWidgetConfig) return null

    // 平台报告：复用 ReportCardWidget，舞台放大时内部内容等比缩放
    return (
      <StageScaledReportCard
        config={stageWidgetConfig}
        data={reportData.card_visuals}
        showOverview={showOverview}
      />
    )
  }

  // 监听外部播放/暂停事件
  useEffect(() => {
    const handleTogglePause = () => {
      setIsPaused((prev) => !prev)
    }

    window.addEventListener('stage-toggle-pause', handleTogglePause)
    return () => {
      window.removeEventListener('stage-toggle-pause', handleTogglePause)
    }
  }, [])

  // 同步isPaused状态到外部
  useEffect(() => {
    window.dispatchEvent(
      new CustomEvent('stage-pause-state-change', {
        detail: { isPaused },
      }),
    )
  }, [isPaused])

  // 解析报告数据
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

  // 监听系统 / 应用主题切换，保持舞台模式背景同步
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

    // 初始化同步
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

  // 使用 ref 存储 onClose，避免因父组件重渲染导致 timer 被重置
  const onCloseRef = useRef(onClose)
  useEffect(() => {
    onCloseRef.current = onClose
  }, [onClose])

  // 自动切换篇章
  useEffect(() => {
    if (!isOpen || chapters.length === 0 || isPaused) return // 暂停时不切换

    const currentChapterData = chapters[currentChapter]
    if (!currentChapterData) return

    // 1. 计算字幕播放完成所需的总时间
    const subtitleDuration = currentChapterData.lines.reduce(
      (sum, line, i) => sum + (i === 0 ? 500 : line.delay),
      0,
    )

    // 2. 计算阅读缓冲时间
    // 之前是每行+1000ms，导致多行文本等待时间过长
    // 现在改为：基础缓冲 2.5秒 + 每行 200ms 的动态阅读时间
    let bufferTime = 2500 + currentChapterData.lines.length * 200

    // 3. 特殊场景调整
    if (currentChapterData.title === t.reportsPage.deepInsight) {
      // 深度洞察：为了配合右侧卡片轮播（5s一次），确保至少能展示 2-3 轮
      // 如果字幕很短，强制延长；如果字幕很长，就按字幕时间来
      const minDuration = 12000 // 至少12秒
      if (subtitleDuration + bufferTime < minDuration) {
        bufferTime = minDuration - subtitleDuration
      }
    } else if (currentChapter === chapters.length - 1) {
      // 最后一章：额外增加 3秒 结束感
      bufferTime += 3000
    }

    const totalDuration = subtitleDuration + bufferTime

    const timer = setTimeout(() => {
      if (currentChapter < chapters.length - 1) {
        setCurrentChapter((prev) => prev + 1)
      } else {
        // 所有篇章播放完毕，触发完成事件
        window.dispatchEvent(new CustomEvent('stage-playback-complete'))
        // 如果不是播放全部模式，才自动关闭
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

  // 光幕进出扫描挂在 #bg-gradient 上，不跟 React 树走。
  // 切页在内容退完之后才发生，卸载时不能取消还在播的退出扫描。
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
            {/* 内容容器 - 与页面布局对齐 */}
            <div className="h-full flex flex-col pt-20 pb-6 px-3 xs:px-4 sm:px-6">
              <div className="flex-1 max-w-7xl mx-auto w-full">
                {/* 上半部分区域 - 移动端65%，桌面端45% */}
                <div className="h-[65%] md:h-[45%] relative">
                  {/* 篇章指示器 - 右上角 */}
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

                  {/* 字幕显示区域 */}
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
