import { FaGithub, FaSteam, SiBilibili, SiNeteasecloudmusic } from '@lib/icons'
import { AnimatePresenceShim as AnimatePresence, motionShim as motion } from '@lib/motionShim'
import { memo, useEffect, useMemo, useRef, useState } from 'react'
import { useI18n } from '../contexts/I18nContext'
import { useReportsVisibilityInterval } from '../hooks/animation/pages/reports'
import { BilibiliWidget, GithubWidget, NeteaseWidget, SteamWidget } from './StageWidgets'

const DARK_ORIGINAL_BG = 'linear-gradient(to bottom, transparent 0%, transparent 35%, rgba(10, 10, 10, 0.3) 45%, rgba(10, 10, 10, 0.5) 55%, rgba(10, 10, 10, 0.75) 70%, rgba(10, 10, 10, 0.9) 85%, rgba(10, 10, 10, 0.95) 100%)'
const LIGHT_ORIGINAL_BG = 'linear-gradient(to bottom, transparent 0%, transparent 35%, rgba(255, 255, 255, 0.4) 55%, rgba(255, 255, 255, 0.9) 85%, rgba(255, 255, 255, 0.9) 100%)'
const FEATHER_RANGE = 120

const getOriginalBackground = (isDark: boolean) => (isDark ? DARK_ORIGINAL_BG : LIGHT_ORIGINAL_BG)

function buildGradientLayer(isDark: boolean, easeProgress: number, isEnteringPhase: boolean) {
  const baseColorStops = isDark
    ? ['rgba(10, 10, 10, 0.98)', 'rgba(10, 10, 10, 0.9)', 'rgba(10, 10, 10, 0.6)', 'rgba(10, 10, 10, 0.2)', 'transparent']
    : ['rgba(255, 255, 255, 0.98)', 'rgba(255, 255, 255, 0.9)', 'rgba(255, 255, 255, 0.6)', 'rgba(255, 255, 255, 0.2)', 'transparent']

  const reverseStops = isDark
    ? ['transparent', 'rgba(10, 10, 10, 0.2)', 'rgba(10, 10, 10, 0.6)', 'rgba(10, 10, 10, 0.9)', 'rgba(10, 10, 10, 0.98)']
    : ['transparent', 'rgba(255, 255, 255, 0.2)', 'rgba(255, 255, 255, 0.6)', 'rgba(255, 255, 255, 0.9)', 'rgba(255, 255, 255, 0.98)']

  const pos = (easeProgress * (100 + FEATHER_RANGE)) - FEATHER_RANGE

  if (isEnteringPhase) {
    return `linear-gradient(to top, ${baseColorStops[0]} ${pos}%, ${baseColorStops[1]} ${pos + FEATHER_RANGE * 0.2}%, ${baseColorStops[2]} ${pos + FEATHER_RANGE * 0.5}%, ${baseColorStops[3]} ${pos + FEATHER_RANGE * 0.8}%, ${baseColorStops[4]} ${pos + FEATHER_RANGE}%)`
  }

  return `linear-gradient(to bottom, ${reverseStops[0]} ${pos}%, ${reverseStops[1]} ${pos + FEATHER_RANGE * 0.2}%, ${reverseStops[2]} ${pos + FEATHER_RANGE * 0.5}%, ${reverseStops[3]} ${pos + FEATHER_RANGE * 0.8}%, ${reverseStops[4]} ${pos + FEATHER_RANGE}%)`
}

function applyStageGradient(element: HTMLElement, isDark: boolean, easeProgress: number, isEnteringPhase: boolean) {
  const gradientLayer = buildGradientLayer(isDark, easeProgress, isEnteringPhase)
  const originalBg = getOriginalBackground(isDark)
  element.style.setProperty('background-image', `${gradientLayer}, ${originalBg}`, 'important')
}

function getBlurAmount(easeProgress: number, isEnteringPhase: boolean) {
  return isEnteringPhase ? easeProgress * 20 : (1 - easeProgress) * 20
}

// 🚀 性能优化：预编译正则表达式（避免每次调用时重新创建）
const CONTROL_CHARS_REGEX = /[\u0000-\u001F\u007F-\u009F]/g
const MARKDOWN_SYMBOLS_REGEX = /[*_~`]/g
const WHITESPACE_REGEX = /\s+/g
const PUNCTUATION_SPLIT_REGEX = /([。！？.!?，,])/g

// 🚀 性能优化：共享文本处理工具函数
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
    .map(s => s.trim())
    .filter(s => s.length > 0)
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
    // 综合报告字段
    综合分析?: any
    library_items?: Array<{ title: string, cover?: string, type: string, platform?: string }>
    type?: 'platform' | 'comprehensive'
  } | null
  onRefresh?: () => void // 刷新当前报告的回调
  playAllMode?: boolean // 是否在播放全部模式下
}

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
  const [visibleLines, setVisibleLines] = useState<{ id: number, text: string }[]>([])
  const [currentIndex, setCurrentIndex] = useState(0)

  useEffect(() => {
    if (!isActive || lines.length === 0 || isPaused) {
      if (isPaused)
        return // 暂停时保持当前状态
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
      if (index >= lines.length || !mounted)
        return

      const line = lines[index]
      const delay = index === 0 ? 500 : line.delay

      timer = setTimeout(() => {
        if (!mounted)
          return

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
      <div className="w-[90%] md:w-[60%] h-full flex flex-col justify-end items-start pl-2 md:pl-6 overflow-hidden pb-8">
        <AnimatePresence mode="popLayout">
          {visibleLines.map(line => (
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
      {/* 右侧留白给卡片 - 移动端10%，桌面端40% */}
      <div className="w-[10%] md:w-[40%] h-full flex items-center justify-center p-8 pointer-events-auto">
        {rightContent}
      </div>
    </div>
  )
}

// 综合报告资料库卡片组件
const ComprehensiveLibraryWidget = memo(({ libraryItems }: { libraryItems: Array<{ title: string, cover?: string, type: string, platform?: string }> }) => {
  const { t } = useI18n()

  // 内容类型标签映射
  const getTypeLabel = (type: string) => {
    switch (type) {
      case 'game': return t.reportsPage.game
      case 'anime': return t.reportsPage.anime
      case 'tv_series': return t.reportsPage.tvSeries
      case 'video': return t.reportsPage.video
      case 'music': return t.reportsPage.music
      default: return t.reportsPage.content
    }
  }

  // 随机打乱数组并缓存
  const shuffledItems = useMemo(() => {
    const items = [...libraryItems]
    for (let i = items.length - 1; i > 0; i--) {
      const j = Math.floor(Math.random() * (i + 1));
      [items[i], items[j]] = [items[j], items[i]]
    }
    return items
  }, [libraryItems])

  const [currentIndex, setCurrentIndex] = useState(0)

  // 🔧 使用报告页原子化可见性感知定时器进行舞台模式轮播
  useReportsVisibilityInterval(
    () => setCurrentIndex(prev => (prev + 1) % shuffledItems.length),
    shuffledItems.length > 0 ? 5000 : null,
  )

  if (shuffledItems.length === 0)
    return null

  const currentItem = shuffledItems[currentIndex]

  // 获取平台图标
  const getPlatformIcon = () => {
    const platform = currentItem.platform?.toLowerCase()
    switch (platform) {
      case 'bilibili':
        return <SiBilibili className="w-4 h-4" />
      case 'steam':
        return <FaSteam className="w-4 h-4" />
      case 'github':
        return <FaGithub className="w-4 h-4" />
      case 'netease':
      case 'netease music':
        return <SiNeteasecloudmusic className="w-4 h-4" />
      default:
        return null
    }
  }

  // 获取平台颜色
  const getPlatformColor = () => {
    const platform = currentItem.platform?.toLowerCase()
    switch (platform) {
      case 'bilibili':
        return '#00A1D6'
      case 'steam':
        return '#171a21'
      case 'github':
        return '#24292e'
      case 'netease':
      case 'netease music':
        return '#d33a31'
      default:
        return '#6b7280'
    }
  }

  return (
    <AnimatePresence mode="wait">
      <motion.div
        key={currentIndex}
        initial={{ opacity: 0, scale: 0.9 }}
        animate={{ opacity: 1, scale: 1 }}
        exit={{ opacity: 0, scale: 0.9 }}
        transition={{ duration: 0.5 }}
        className="w-full h-full"
      >
        <div className="relative w-full h-full rounded-xl overflow-hidden shadow-2xl bg-white dark:bg-black/90">
          <div className="absolute inset-0">
            {currentItem.cover
              ? (
                  <img
                    src={currentItem.cover}
                    alt={currentItem.title}
                    className="w-full h-full object-cover"
                    loading="lazy"
                  />
                )
              : (
                  <div className="w-full h-full flex items-center justify-center bg-gradient-to-br from-[var(--color-primary,#10b981)] to-[var(--color-accent,#059669)]">
                    <span className="text-6xl">
                      {currentItem.type === 'game'
                        ? '🎮'
                        : currentItem.type === 'video' || currentItem.type === 'anime' || currentItem.type === 'tv_series'
                          ? '📺'
                          : currentItem.type === 'music' ? '🎵' : '📚'}
                    </span>
                  </div>
                )}
            <div className="absolute inset-0 bg-gradient-to-t from-black/90 via-black/50 to-transparent" />
          </div>

          <div className="absolute bottom-0 left-0 right-0 p-4">
            <div className="inline-flex max-w-full">
              <div className="bg-white/95 dark:bg-neutral-950/95 backdrop-blur-sm rounded-lg p-3 shadow-lg">
                <h3 className="font-bold text-gray-900 dark:text-white text-sm line-clamp-2 mb-2">
                  {currentItem.title}
                </h3>
                <div className="flex items-center gap-2">
                  {currentItem.platform && getPlatformIcon() && (
                    <div
                      className="flex items-center justify-center w-6 h-6 rounded-md"
                      style={{
                        backgroundColor: `${getPlatformColor()}15`,
                        color: getPlatformColor(),
                      }}
                    >
                      {getPlatformIcon()}
                    </div>
                  )}
                  <span className="px-2 py-0.5 rounded text-xs font-medium bg-indigo-100 dark:bg-indigo-900 text-indigo-700 dark:text-indigo-300">
                    {getTypeLabel(currentItem.type)}
                  </span>
                </div>
              </div>
            </div>
          </div>
        </div>
      </motion.div>
    </AnimatePresence>
  )
})

// 综合报告解析：将综合分析转换为篇章
function parseComprehensiveReportToChapters(analysis: any): StageChapter[] {
  const chapters: StageChapter[] = []

  // 遍历综合分析的所有字段
  const styleFields = [
    'theme_color',
    'theme_icon',
    'visual_style',
    'decorative_emojis',
    'card_subtitle',
    'key_metric',
    'background_elements',
    'icon_image_url',
    'icon_prompt',
  ]

  Object.entries(analysis)
    .filter(([key]) => !styleFields.includes(key))
    .forEach(([key, value]) => {
      if (typeof value === 'string' && value.trim()) {
        const cleanedText = cleanText(value)
        const sentences = splitByPunctuation(cleanedText)

        if (sentences.length > 0) {
          const formatFieldName = (name: string) => {
            return name
              .split('_')
              .map(word => word.charAt(0).toUpperCase() + word.slice(1))
              .join(' ')
          }

          chapters.push({
            title: formatFieldName(key),
            lines: sentences.map((sentence, i) => ({
              text: sentence,
              delay: i === 0 ? 800 : 1500,
            })),
          })
        }
      }
      else if (Array.isArray(value) && value.length > 0 && typeof value[0] === 'string') {
        const isShortStrings = value.every((item: any) => typeof item === 'string' && item.length < 20)

        if (!isShortStrings) {
          const formatFieldName = (name: string) => {
            return name
              .split('_')
              .map(word => word.charAt(0).toUpperCase() + word.slice(1))
              .join(' ')
          }

          // 对数组中的每一项进行标点分段处理
          const allLines: SubtitleLine[] = []
          value.forEach((item: string, itemIndex: number) => {
            const cleanedItem = cleanText(item)
            const sentences = splitByPunctuation(cleanedItem)

            sentences.forEach((sentence, sentenceIndex) => {
              allLines.push({
                text: sentence,
                delay: (itemIndex === 0 && sentenceIndex === 0) ? 800 : 1500,
              })
            })
          })

          chapters.push({
            title: formatFieldName(key),
            lines: allLines,
          })
        }
      }
    })

  return chapters
}

// 内容解析：将报告转换为篇章
function parseReportToChapters(reportData: {
  summary: string
  insights: string[]
}, chapterTitles: { dataEcho: string, deepInsight: string }): StageChapter[] {
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
      .filter(s => s.length > 0)

    // 洞察可能也需要分句，如果太长的话
    const allInsightLines: SubtitleLine[] = []
    insights.forEach((insight, i) => {
      const parts = splitByPunctuation(insight)
      parts.forEach((part, j) => {
        allInsightLines.push({
          text: part,
          delay: (i === 0 && j === 0) ? 800 : 1500,
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
export default function StageMode({ isOpen, onClose, reportData, onRefresh, playAllMode = false }: StageModeProps) {
  const { t } = useI18n()
  const [currentChapter, setCurrentChapter] = useState(0)
  const [chapters, setChapters] = useState<StageChapter[]>([])
  const [isPaused, setIsPaused] = useState(false) // 播放/暂停状态
  const [isDarkMode, setIsDarkMode] = useState(() => {
    if (typeof document === 'undefined')
      return false
    return document.documentElement.classList.contains('dark')
  })
  const isDarkModeRef = useRef(isDarkMode)
  const gradientStateRef = useRef({ easeProgress: 0, isEntering: true })
  const bgGradientBaseClassRef = useRef<string | null>(null)

  // 自动切换概览/详情 - 基于篇章 (使用 useMemo 替代 useEffect 避免状态同步延迟)
  const showOverview = useMemo(() => {
    if (!chapters[currentChapter])
      return true
    const title = chapters[currentChapter].title
    // 只有在明确是"深度洞察"时才显示内容，其他情况（包括"数据回想"或未知）都显示概览
    return title !== t.reportsPage.deepInsight
  }, [currentChapter, chapters, t.reportsPage.deepInsight])

  const renderWidget = () => {
    if (!reportData)
      return null

    // 综合报告显示资料库卡片
    if (reportData.type === 'comprehensive') {
      const libraryItems = reportData.library_items || []
      if (libraryItems.length > 0) {
        return <ComprehensiveLibraryWidget libraryItems={libraryItems} />
      }
      return null
    }

    // 平台报告显示对应平台的卡片
    switch (reportData.platform) {
      case 'bilibili': return <BilibiliWidget data={reportData.card_visuals} showOverview={showOverview} />
      case 'steam': return <SteamWidget data={reportData.card_visuals} showOverview={showOverview} />
      case 'github': return <GithubWidget data={reportData.card_visuals} showOverview={showOverview} />
      case 'netease': return <NeteaseWidget data={reportData.card_visuals} showOverview={showOverview} />
      default: return null
    }
  }

  // 监听外部播放/暂停事件
  useEffect(() => {
    const handleTogglePause = () => {
      setIsPaused(prev => !prev)
    }

    window.addEventListener('stage-toggle-pause', handleTogglePause)
    return () => {
      window.removeEventListener('stage-toggle-pause', handleTogglePause)
    }
  }, [])

  // 同步isPaused状态到外部
  useEffect(() => {
    window.dispatchEvent(new CustomEvent('stage-pause-state-change', {
      detail: { isPaused },
    }))
  }, [isPaused])

  // 解析报告数据
  useEffect(() => {
    if (reportData) {
      let parsedChapters: StageChapter[] = []
      const chapterTitles = {
        dataEcho: t.reportsPage.dataEcho,
        deepInsight: t.reportsPage.deepInsight,
      }

      // 判断是综合报告还是平台报告
      if (reportData.type === 'comprehensive' && reportData.综合分析) {
        parsedChapters = parseComprehensiveReportToChapters(reportData.综合分析)
      }
      else if (reportData.summary && reportData.insights) {
        parsedChapters = parseReportToChapters({
          summary: reportData.summary,
          insights: reportData.insights,
        }, chapterTitles)
      }

      setChapters(parsedChapters)
      setCurrentChapter(0)
    }
  }, [reportData, t.reportsPage.dataEcho, t.reportsPage.deepInsight])

  // 监听系统 / 应用主题切换，保持舞台模式背景同步
  useEffect(() => {
    if (typeof document === 'undefined')
      return
    const root = document.documentElement
    if (!root)
      return

    const getIsDark = () => root.classList.contains('dark')

    const syncMode = () => {
      const next = getIsDark()
      setIsDarkMode(prev => (prev === next ? prev : next))
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

    const supportsMatchMedia = typeof window !== 'undefined' && typeof window.matchMedia === 'function'
    const mediaQuery = supportsMatchMedia ? window.matchMedia('(prefers-color-scheme: dark)') : null
    const handleMediaChange = () => syncMode()

    if (mediaQuery) {
      if (mediaQuery.addEventListener) {
        mediaQuery.addEventListener('change', handleMediaChange)
      }
      else if (mediaQuery.addListener) {
        mediaQuery.addListener(handleMediaChange)
      }
    }

    // 初始化同步
    syncMode()

    return () => {
      observer.disconnect()
      if (mediaQuery) {
        if (mediaQuery.removeEventListener) {
          mediaQuery.removeEventListener('change', handleMediaChange)
        }
        else if (mediaQuery.removeListener) {
          mediaQuery.removeListener(handleMediaChange)
        }
      }
    }
  }, [])

  useEffect(() => {
    isDarkModeRef.current = isDarkMode
  }, [isDarkMode])

  useEffect(() => {
    const bgGradient = typeof document !== 'undefined' ? document.getElementById('bg-gradient') : null
    if (bgGradient && !bgGradientBaseClassRef.current) {
      bgGradientBaseClassRef.current = bgGradient.className
    }
  }, [])

  // 修复：确保组件卸载时清理全局背景副作用，防止切换页面时背景卡死
  useEffect(() => {
    return () => {
      if (typeof document === 'undefined')
        return
      const bgGradient = document.getElementById('bg-gradient')
      if (bgGradient && bgGradient.style.backgroundImage) {
        bgGradient.style.removeProperty('background-image')
        bgGradient.style.backdropFilter = ''
        bgGradient.style.transition = ''
        if (bgGradientBaseClassRef.current) {
          bgGradient.className = bgGradientBaseClassRef.current
        }
      }
    }
  }, [])

  useEffect(() => {
    if (typeof document === 'undefined')
      return
    const bgGradient = document.getElementById('bg-gradient') as HTMLElement | null
    if (!bgGradient)
      return

    if (bgGradientBaseClassRef.current && !isOpen) {
      bgGradient.className = bgGradientBaseClassRef.current
      return
    }

    if (!isOpen)
      return

    if (isDarkMode) {
      bgGradient.className = 'absolute inset-0 transition-opacity duration-500 ease-out'
    }
    else if (bgGradientBaseClassRef.current) {
      bgGradient.className = bgGradientBaseClassRef.current
    }

    const { easeProgress, isEntering } = gradientStateRef.current
    applyStageGradient(bgGradient, isDarkMode, easeProgress, isEntering)
    const blurAmount = getBlurAmount(easeProgress, isEntering)
    if (blurAmount > 0.5) {
      bgGradient.style.backdropFilter = `blur(${blurAmount}px)`
    }
    else {
      bgGradient.style.backdropFilter = ''
    }
  }, [isDarkMode, isOpen])

  // 使用 ref 存储 onClose，避免因父组件重渲染导致 timer 被重置
  const onCloseRef = useRef(onClose)
  useEffect(() => {
    onCloseRef.current = onClose
  }, [onClose])

  // 自动切换篇章
  useEffect(() => {
    if (!isOpen || chapters.length === 0 || isPaused)
      return // 暂停时不切换

    const currentChapterData = chapters[currentChapter]
    if (!currentChapterData)
      return

    // 1. 计算字幕播放完成所需的总时间
    const subtitleDuration = currentChapterData.lines.reduce(
      (sum, line, i) => sum + (i === 0 ? 500 : line.delay),
      0,
    )

    // 2. 计算阅读缓冲时间
    // 之前是每行+1000ms，导致多行文本等待时间过长
    // 现在改为：基础缓冲 2.5秒 + 每行 200ms 的动态阅读时间
    let bufferTime = 2500 + (currentChapterData.lines.length * 200)

    // 3. 特殊场景调整
    if (currentChapterData.title === t.reportsPage.deepInsight) {
      // 深度洞察：为了配合右侧卡片轮播（5s一次），确保至少能展示 2-3 轮
      // 如果字幕很短，强制延长；如果字幕很长，就按字幕时间来
      const minDuration = 12000 // 至少12秒
      if (subtitleDuration + bufferTime < minDuration) {
        bufferTime = minDuration - subtitleDuration
      }
    }
    else if (currentChapter === chapters.length - 1) {
      // 最后一章：额外增加 3秒 结束感
      bufferTime += 3000
    }

    const totalDuration = subtitleDuration + bufferTime

    const timer = setTimeout(() => {
      if (currentChapter < chapters.length - 1) {
        setCurrentChapter(prev => prev + 1)
      }
      else {
        // 所有篇章播放完毕，触发完成事件
        window.dispatchEvent(new CustomEvent('stage-playback-complete'))
        // 如果不是播放全部模式，才自动关闭
        if (!playAllMode) {
          onCloseRef.current()
        }
      }
    }, totalDuration)

    return () => clearTimeout(timer)
  }, [isOpen, currentChapter, chapters, isPaused, playAllMode]) // 添加 isPaused 和 playAllMode 依赖

  // 激活时调整全局背景 - 柔和光幕扫描动画
  useEffect(() => {
    if (typeof document === 'undefined')
      return
    const bgGradient = document.getElementById('bg-gradient') as HTMLElement | null
    if (!bgGradient)
      return

    if (!bgGradientBaseClassRef.current) {
      bgGradientBaseClassRef.current = bgGradient.className
    }

    let animationFrameId: number | null = null
    let startTime: number | null = null
    const duration = 1000 // 稍微延长动画时间以配合柔和感

    const cleanupOverlay = () => {
      bgGradient.style.removeProperty('background-image')
      bgGradient.style.backdropFilter = ''
      bgGradient.style.transition = ''
      gradientStateRef.current = { easeProgress: 0, isEntering: true }
      if (bgGradientBaseClassRef.current) {
        bgGradient.className = bgGradientBaseClassRef.current
      }
    }

    const animate = (timestamp: number, isEnteringPhase: boolean) => {
      if (startTime === null)
        startTime = timestamp
      const progress = Math.min((timestamp - startTime) / duration, 1)
      const easeProgress = progress < 0.5
        ? 4 * progress * progress * progress
        : 1 - (-2 * progress + 2) ** 3 / 2

      gradientStateRef.current = { easeProgress, isEntering: isEnteringPhase }

      const themeIsDark = isDarkModeRef.current
      applyStageGradient(bgGradient, themeIsDark, easeProgress, isEnteringPhase)

      const blurAmount = getBlurAmount(easeProgress, isEnteringPhase)
      if (blurAmount > 0.5) {
        bgGradient.style.backdropFilter = `blur(${blurAmount}px)`
      }
      else {
        bgGradient.style.backdropFilter = ''
      }

      if (progress < 1) {
        animationFrameId = requestAnimationFrame(t => animate(t, isEnteringPhase))
      }
      else if (!isEnteringPhase) {
        cleanupOverlay()
      }
    }

    if (isOpen) {
      if (isDarkModeRef.current) {
        bgGradient.className = 'absolute inset-0 transition-opacity duration-500 ease-out'
      }
      else if (bgGradientBaseClassRef.current) {
        bgGradient.className = bgGradientBaseClassRef.current
      }
      bgGradient.style.transition = 'none'
      startTime = null
      animationFrameId = requestAnimationFrame(t => animate(t, true))
    }
    else if (bgGradient.style.backgroundImage) {
      bgGradient.style.transition = 'none'
      startTime = null
      animationFrameId = requestAnimationFrame(t => animate(t, false))
    }
    else if (bgGradientBaseClassRef.current) {
      bgGradient.className = bgGradientBaseClassRef.current
    }

    return () => {
      if (animationFrameId)
        cancelAnimationFrame(animationFrameId)
      if (!isOpen) {
        cleanupOverlay()
      }
    }
  }, [isOpen])

  if (!reportData)
    return null

  return (
    <AnimatePresence mode="wait">
      {isOpen && (
        <>
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
                    <div className="bg-white/80 dark:bg-neutral-900/80 rounded-xl px-4 py-2 backdrop-blur-xl shadow-lg border border-gray-100 dark:border-neutral-700">
                      <div className="flex items-center gap-3">
                        <div className="flex flex-col items-end">
                          <div className="text-xs text-gray-500 dark:text-gray-400 font-mono">
                            ACT
                            {' '}
                            {currentChapter + 1}
                            /
                            {chapters.length}
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
        </>
      )}
    </AnimatePresence>
  )
}
