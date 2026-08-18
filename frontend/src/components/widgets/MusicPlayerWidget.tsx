/**
 * 音乐播放器小组件
 * Glass风格设计，2x2紧凑布局
 */

import type { AnimationConfig } from '../../hooks/useAnimationLevel'
import type { LyricLine, WordLyricLine } from '../../utils/musicPlayer'
import type { WidgetConfig } from '../WidgetGrid'

import { LuMusic } from '@lib/icons'
import { motionShim as motion } from '@lib/motionShim'
import { memo, useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useI18n } from '../../contexts/I18nContext'
import { useMusicPlayerControl } from '../../contexts/MusicPlayerContext'
import {
  isPageVisible,
  onVisibility,
  useLoopAnimation,
} from '../../hooks/animation'
import { useAnimationLevel } from '../../hooks/useAnimationLevel'
import { useWidgetSize } from '../../hooks/useWidgetSize'
import { cssBackgroundImage } from '../../utils/cssUrl'
import { audioManager } from '../../utils/musicPlayer'
import { PlayingSpectrum } from '../shared/PlayingSpectrum'
import { GlowBackground } from './shared/GlowBackground'
import { WidgetShell } from './shared/WidgetShell'

// 漂浮歌词组件

interface FloatingChar {
  char: string
  index: number
  absoluteTime: number
  duration: number
  seed: number
}

interface FloatingLyricPage {
  index: number
  startTime: number
  chars: FloatingChar[]
}

// 确定性随机函数 - 模块级别，避免在每次渲染时重新创建
function seededRandom(seed: number): number {
  const x = Math.sin(seed * 9999) * 10000
  return x - Math.floor(x)
}

// 计算字符布局位置 - 纯函数，模块级别
function computeCharPositions(
  visibleChars: FloatingChar[],
): { x: number; y: number; fontSize: number; rotation: number }[] {
  if (visibleChars.length === 0) return []

  const positions: {
    x: number
    y: number
    fontSize: number
    rotation: number
  }[] = []
  let cumulativeX = 4
  const availableWidth = 92

  const weights: number[] = []
  let totalWeight = 0
  visibleChars.forEach((char) => {
    const tokenLength = char.char.length
    const isEnglishWord = tokenLength > 1 && /^[a-z]/i.test(char.char)
    const baseMin = isEnglishWord ? 0.9 : 0.7
    const baseWeight = baseMin + seededRandom(char.seed + 100) * 0.4
    const lengthMultiplier = 1 + (tokenLength - 1) * 0.6
    const weight = baseWeight * lengthMultiplier
    weights.push(weight)
    totalWeight += weight
  })

  visibleChars.forEach((char, idx) => {
    const x = cumulativeX
    const yRandom = seededRandom(char.seed)
    const y = Math.sqrt(yRandom) * 45
    const fontSizeRatio = 0.85 + seededRandom(char.seed + 200) * 0.4
    const rotation = (seededRandom(char.seed + 300) - 0.5) * 16
    positions.push({ x, y, fontSize: fontSizeRatio, rotation })
    const charWidth = (weights[idx] / totalWeight) * availableWidth
    cumulativeX += charWidth
  })

  return positions
}

// 批次检查间隔（毫秒）
const BATCH_CHECK_MS = 80
// 动画更新间隔（毫秒）~20fps，足够柔和
const ANIM_UPDATE_MS = 50
// 每批显示的最大字符数
const MAX_CHARS_PER_BATCH = 10
// 回退估算模式下单页歌词最长停留时间。真实逐字歌词不走这个限制。
const MAX_ESTIMATED_LYRIC_PAGE_SECONDS = 4.8
const MAX_ESTIMATED_LINE_SECONDS = 14
const FALLBACK_LAST_LINE_SECONDS = 6

function getEstimatedTokenWeight(token: string): number {
  if (!token.trim()) return 0.15
  if (/^[，。！？、,.!?;:：；'"“”‘’()[\]{}<>《》…-]+$/.test(token)) {
    return 0.35
  }
  if (/^[a-z']/i.test(token)) {
    return Math.min(2.6, 0.75 + token.length * 0.22)
  }
  if (/^\d/.test(token)) {
    return Math.min(2, 0.8 + token.length * 0.18)
  }
  return 1
}

// 漂浮歌词显示组件 - 逐字淡入，分批显示
// 低性能模式：保留批次切换，关闭频谱节拍 + 漂浮位移
const FloatingLyrics = memo(
  ({
    lyrics,
    verbatimLyrics,
    currentLyricIndex,
    isPlaying,
    themeColor,
    fontScale,
    animLevel = 'standard',
  }: {
    lyrics: LyricLine[]
    verbatimLyrics: WordLyricLine[]
    currentLyricIndex: number
    isPlaying: boolean
    themeColor: string
    fontScale: number
    animLevel?: AnimationConfig['level']
  }) => {
    const charsRef = useRef<(HTMLSpanElement | null)[]>([])
    const animationRef = useRef<number | null>(null)
    const pageVisibleRef = useRef(isPageVisible())
    const phaseRef = useRef(0)
    const currentTimeRef = useRef(0)
    const currentBatchIndexRef = useRef(-1)
    // 当前批次数据存于 ref，渲染时读取
    const visibleCharsRef = useRef<FloatingChar[]>([])
    const charPositionsRef = useRef<
      { x: number; y: number; fontSize: number; rotation: number }[]
    >([])
    // 频谱节奏检测
    const rhythmProgressRef = useRef(0)
    const lastBeatTimeRef = useRef(0)
    const energyHistoryRef = useRef<number[]>([])
    const useFullFx = animLevel === 'standard'

    // 唯一触发 React 重渲染的 state — 只在批次真正切换时递增
    const [, setBatchVersion] = useState(0)

    // 构建歌词页映射 — 逐字优先，逐行歌词兜底
    const lyricPages = useMemo(() => {
      const pages: FloatingLyricPage[] = []
      if (lyrics.length === 0 && verbatimLyrics.length === 0) return pages

      const tokenize = (text: string): string[] => {
        const tokens: string[] = []
        let i = 0
        while (i < text.length) {
          const char = text[i]
          if (/[a-z]/i.test(char)) {
            let word = ''
            while (i < text.length && /[a-z']/i.test(text[i])) {
              word += text[i]
              i++
            }
            tokens.push(word)
          } else if (/\d/.test(char)) {
            let num = ''
            while (i < text.length && /[0-9.,]/.test(text[i])) {
              num += text[i]
              i++
            }
            tokens.push(num)
          } else {
            tokens.push(char)
            i++
          }
        }
        return tokens
      }

      let globalIndex = 0
      let pageIndex = 0

      if (verbatimLyrics.length > 0) {
        verbatimLyrics.forEach((line) => {
          const words = line.words.filter((word) => word.text)
          if (words.length === 0) return

          for (
            let pageStart = 0;
            pageStart < words.length;
            pageStart += MAX_CHARS_PER_BATCH
          ) {
            const pageWords = words.slice(
              pageStart,
              pageStart + MAX_CHARS_PER_BATCH,
            )
            const firstWord = pageWords[0]
            const chars = pageWords.map((word) => {
              const seed = globalIndex * 17 + word.text.charCodeAt(0)
              const charConfig = {
                char: word.text,
                index: globalIndex,
                absoluteTime: word.time,
                duration: Math.max(0.08, word.duration || 0.18),
                seed,
              }
              globalIndex++
              return charConfig
            })

            pages.push({
              index: pageIndex,
              startTime: firstWord?.time ?? line.time,
              chars,
            })
            pageIndex++
          }
        })

        return pages
      }

      lyrics.forEach((line, lineIdx) => {
        const nextLine = lyrics[lineIdx + 1]
        const lineStartTime = line.time
        const rawDuration = nextLine
          ? nextLine.time - lineStartTime
          : FALLBACK_LAST_LINE_SECONDS
        const tokens = tokenize(line.text)
        if (tokens.length === 0) return

        const pageCount = Math.ceil(tokens.length / MAX_CHARS_PER_BATCH)
        const estimatedLineDuration = Math.min(
          MAX_ESTIMATED_LINE_SECONDS,
          Math.max(0.9, rawDuration),
          pageCount * MAX_ESTIMATED_LYRIC_PAGE_SECONDS,
        )
        const pageDuration = estimatedLineDuration / pageCount

        for (let pageOffset = 0; pageOffset < pageCount; pageOffset++) {
          const pageStart = pageOffset * MAX_CHARS_PER_BATCH
          const pageTokens = tokens.slice(
            pageStart,
            pageStart + MAX_CHARS_PER_BATCH,
          )
          const pageStartTime = lineStartTime + pageOffset * pageDuration
          const revealDuration = Math.min(
            Math.max(0.6, pageDuration * 0.78),
            pageDuration,
          )
          const tokenWeights = pageTokens.map(getEstimatedTokenWeight)
          const totalWeight = tokenWeights.reduce((sum, w) => sum + w, 0) || 1
          let cumulativeWeight = 0

          const chars = pageTokens.map((token, tokenIdx) => {
            const tokenStartRatio = cumulativeWeight / totalWeight
            cumulativeWeight += tokenWeights[tokenIdx]
            const tokenEndRatio = cumulativeWeight / totalWeight
            const tokenTime = pageStartTime + tokenStartRatio * revealDuration
            const estimatedDuration = Math.max(
              0.08,
              (tokenEndRatio - tokenStartRatio) * revealDuration,
            )
            const seed = globalIndex * 17 + token.charCodeAt(0)
            const charConfig = {
              char: token,
              index: globalIndex,
              absoluteTime: tokenTime,
              duration: estimatedDuration,
              seed,
            }
            globalIndex++
            return charConfig
          })

          pages.push({
            index: pageIndex,
            startTime: pageStartTime,
            chars,
          })
          pageIndex++
        }
      })

      return pages
    }, [lyrics, verbatimLyrics])

    // 计算指定时间点应显示的批次 — 在 RAF 内调用的纯计算
    const computeCurrentBatch = useCallback(
      (time: number) => {
        if (lyricPages.length === 0) {
          return {
            chars: [] as FloatingChar[],
            positions: [] as ReturnType<typeof computeCharPositions>,
            batchIndex: -1,
          }
        }

        let page = lyricPages[0]
        for (let i = 0; i < lyricPages.length; i++) {
          if (lyricPages[i].startTime <= time) {
            page = lyricPages[i]
          } else {
            break
          }
        }

        return {
          chars: page.chars,
          positions: computeCharPositions(page.chars),
          batchIndex: page.index,
        }
      },
      [lyricPages],
    )

    // 歌词变化时初始化第一批（非播放状态下也需要显示内容）
    useEffect(() => {
      currentBatchIndexRef.current = -1
      if (lyricPages.length > 0) {
        const fallbackTime =
          currentLyricIndex >= 0
            ? (lyrics[currentLyricIndex]?.time ?? currentTimeRef.current)
            : currentTimeRef.current
        const { chars, positions, batchIndex } =
          computeCurrentBatch(fallbackTime)
        visibleCharsRef.current = chars
        charPositionsRef.current = positions
        currentBatchIndexRef.current = batchIndex
      } else {
        visibleCharsRef.current = []
        charPositionsRef.current = []
      }
      setBatchVersion((v) => v + 1)
    }, [lyricPages, lyrics, currentLyricIndex, computeCurrentBatch])

    // 统一 RAF 循环：合并时间同步 + 批次检查 + 动画更新
    // 重渲染触发从每200ms降为批次切换时（约每4秒）
    useEffect(() => {
      if (!isPlaying) {
        if (animationRef.current) {
          cancelAnimationFrame(animationRef.current)
          animationRef.current = null
        }
        return
      }

      const audio = audioManager.getCurrentAudio()
      if (!audio) return

      const unsubscribe = onVisibility((visible) => {
        pageVisibleRef.current = visible
      })

      let lastBatchCheck = 0
      let lastAnimUpdate = 0
      const ENERGY_HISTORY_SIZE = 8
      const BEAT_THRESHOLD = 1.3
      const BEAT_COOLDOWN = 150
      // 低性能：批次检查放宽，跳过频谱与漂浮
      const batchInterval = useFullFx ? BATCH_CHECK_MS : 320
      const animInterval = useFullFx ? ANIM_UPDATE_MS : 100

      const loop = (timestamp: number) => {
        if (!pageVisibleRef.current) {
          animationRef.current = requestAnimationFrame(loop)
          return
        }

        currentTimeRef.current = audio.currentTime

        // --- 批次检查 --- 只在批次真正变化时触发 React 重渲染
        if (timestamp - lastBatchCheck >= batchInterval) {
          const { chars, positions, batchIndex } = computeCurrentBatch(
            audio.currentTime,
          )
          if (
            batchIndex !== currentBatchIndexRef.current &&
            batchIndex !== -1
          ) {
            currentBatchIndexRef.current = batchIndex
            visibleCharsRef.current = chars
            charPositionsRef.current = positions
            if (chars.length > 0) {
              setBatchVersion((v) => v + 1)
            }
          }
          lastBatchCheck = timestamp
        }

        // --- 动画更新 --- 直接操作 DOM，不触发 React 渲染
        if (timestamp - lastAnimUpdate >= animInterval) {
          const visibleChars = visibleCharsRef.current

          if (!useFullFx) {
            // 低性能：仅时间驱动透明度/高亮，无频谱、无漂浮
            charsRef.current.forEach((el, idx) => {
              if (!el || idx >= visibleChars.length) return
              const charData = visibleChars[idx]
              if (charData.char === ' ') {
                el.style.opacity = '0'
                return
              }
              const wordStart = charData.absoluteTime
              const wordEnd = charData.absoluteTime + charData.duration
              const isFuture = currentTimeRef.current < wordStart - 0.06
              const isActive =
                currentTimeRef.current >= wordStart - 0.06 &&
                currentTimeRef.current <= wordEnd + 0.08
              const isPast = currentTimeRef.current > wordEnd + 0.08
              if (isFuture) {
                el.style.opacity = '0'
                el.style.color = ''
                el.style.textShadow = 'none'
                el.style.transform = 'none'
              } else if (isActive) {
                el.style.opacity = '1'
                el.style.color = themeColor
                el.style.textShadow = 'none'
                el.style.transform = 'none'
              } else if (isPast) {
                el.style.opacity = '0.7'
                el.style.color = ''
                el.style.textShadow = 'none'
                el.style.transform = 'none'
              }
            })
            lastAnimUpdate = timestamp
            animationRef.current = requestAnimationFrame(loop)
            return
          }

          phaseRef.current += 0.015
          const phase = phaseRef.current

          const spectrum = audioManager.getSpectrumData()
          const currentEnergy =
            (spectrum[0] + spectrum[1] + spectrum[2] + spectrum[3]) / 4
          energyHistoryRef.current.push(currentEnergy)
          if (energyHistoryRef.current.length > ENERGY_HISTORY_SIZE) {
            energyHistoryRef.current.shift()
          }
          const avgEnergy =
            energyHistoryRef.current.reduce((a, b) => a + b, 0) /
            energyHistoryRef.current.length
          const isBeat =
            currentEnergy > avgEnergy * BEAT_THRESHOLD &&
            currentEnergy > 0.15 &&
            timestamp - lastBeatTimeRef.current > BEAT_COOLDOWN

          if (isBeat) {
            lastBeatTimeRef.current = timestamp
            rhythmProgressRef.current += 0.08
          } else {
            rhythmProgressRef.current *= 0.95
          }

          const rhythmModulation = Math.min(0.3, rhythmProgressRef.current)

          charsRef.current.forEach((el, idx) => {
            if (!el || idx >= visibleChars.length) return

            const charData = visibleChars[idx]
            if (charData.char === ' ') {
              el.style.opacity = '0'
              return
            }

            const wordStart =
              charData.absoluteTime - Math.min(0.12, rhythmModulation * 0.4)
            const wordEnd = charData.absoluteTime + charData.duration
            const timeUntilStart = wordStart - currentTimeRef.current
            const { seed } = charData

            const floatAmplitude = 3 + seededRandom(seed + 500) * 2
            const floatY = Math.sin(phase + seed * 0.1) * floatAmplitude
            const floatX =
              Math.cos(phase * 0.7 + seed * 0.15) * (floatAmplitude * 0.6)

            const isFuture = currentTimeRef.current < wordStart - 0.06
            const isActive =
              currentTimeRef.current >= wordStart - 0.06 &&
              currentTimeRef.current <= wordEnd + 0.08
            const isPast = currentTimeRef.current > wordEnd + 0.08

            let opacity = 1
            let scale = 1

            if (isFuture) {
              opacity = isBeat ? 0.15 : 0
              scale = 0.95
            } else if (isActive) {
              const activeProgress = Math.min(
                1,
                Math.max(
                  0,
                  (currentTimeRef.current - wordStart) /
                    Math.max(0.08, charData.duration),
                ),
              )
              opacity = 1
              scale = 1.02 + activeProgress * 0.08 + (isBeat ? 0.05 : 0)
              el.style.color = themeColor
              el.style.textShadow = `0 0 ${isBeat ? 12 : 8}px ${themeColor}60, 0 1px 2px rgba(0,0,0,0.1)`
            } else if (isPast) {
              opacity = 0.7
              scale = 1
              el.style.color = ''
              el.style.textShadow = 'none'
            } else if (timeUntilStart <= 0) {
              opacity = 0.85
              scale = 1
            }

            const rotation = el.dataset.rotation || '0'
            el.style.transform = `translate(${floatX}px, ${floatY}px) scale(${scale}) rotate(${rotation}deg)`
            el.style.opacity = `${opacity}`
          })

          lastAnimUpdate = timestamp
        }

        animationRef.current = requestAnimationFrame(loop)
      }

      animationRef.current = requestAnimationFrame(loop)

      return () => {
        unsubscribe()
        if (animationRef.current) {
          cancelAnimationFrame(animationRef.current)
          animationRef.current = null
        }
      }
    }, [isPlaying, computeCurrentBatch, themeColor, useFullFx])

    const visibleChars = visibleCharsRef.current
    const charPositions = charPositionsRef.current
    const hasLyrics = lyrics.length > 0 || verbatimLyrics.length > 0

    if (visibleChars.length === 0 && hasLyrics) {
      return (
        <div className="text-sm text-gray-400 dark:text-gray-500 opacity-40">
          ...
        </div>
      )
    }

    if (!hasLyrics) {
      return (
        <div className="text-sm text-gray-500 dark:text-gray-400">暂无歌词</div>
      )
    }

    return (
      <div
        className="relative w-full h-full overflow-hidden"
        style={{ minHeight: '60px' }}
      >
        {visibleChars.map((charConfig, idx) => {
          const pos = charPositions[idx] || {
            x: 50,
            y: 50,
            fontSize: 1,
            rotation: 0,
          }

          return (
            <span
              key={`${charConfig.index}-${charConfig.seed}`}
              ref={(el) => {
                charsRef.current[idx] = el
              }}
              data-rotation={pos.rotation}
              className="absolute font-semibold text-gray-700 dark:text-gray-200 pointer-events-none select-none"
              style={{
                left: `${pos.x}%`,
                top: `${pos.y}%`,
                fontSize: `${22 * fontScale * pos.fontSize}px`,
                opacity: isPlaying ? 0 : 0.75,
                transform: `translate(0, 0) scale(1) rotate(${pos.rotation}deg)`,
                willChange: 'transform, opacity',
                transition:
                  'opacity 0.3s ease-out, color 0.2s ease, text-shadow 0.2s ease, transform 0.4s ease-out',
              }}
            >
              {charConfig.char}
            </span>
          )
        })}
      </div>
    )
  },
)

FloatingLyrics.displayName = 'FloatingLyrics'

// 静态动画常量（避免每次渲染创建新对象）

// 封面入场动画
const ALBUM_COVER_INITIAL = { scale: 0.5, opacity: 0, rotate: -15 }
const ALBUM_COVER_ANIMATE = { scale: 1, opacity: 1, rotate: 0 }
const ALBUM_COVER_TRANSITION = { duration: 0.6, ease: [0.34, 1.56, 0.64, 1] }

// 播放状态光晕动画 - 有限次数，配合调度器 duration=4000ms
const GLOW_ANIMATE = { opacity: [0.5, 1, 0.5] }
const GLOW_TRANSITION_LOOP = {
  duration: 2,
  repeat: 1,
  ease: 'easeInOut' as const,
} // 2轮=4s
const GLOW_TRANSITION_ONCE = {
  duration: 2,
  repeat: 0,
  ease: 'easeInOut' as const,
}

// 播放指示器动画 - 有限次数
const INDICATOR_INITIAL = { opacity: 0, scale: 0.8 }
const INDICATOR_ANIMATE = { opacity: 1, scale: 1 }
const INDICATOR_TRANSITION = { duration: 0.3 }

export interface MusicPlayerWidgetProps {
  config: WidgetConfig
  isEditMode: boolean
  isPreview?: boolean
}

// 专辑封面组件 - 独立优化（使用静态动画常量）
const AlbumCover = memo(
  ({
    cover,
    name,
    isPlaying,
    themeColor,
    scale = 1,
    className,
    style,
    anim,
  }: {
    cover: string | undefined
    name: string
    isPlaying: boolean
    themeColor: string
    scale?: number
    className?: string
    style?: React.CSSProperties
    anim: AnimationConfig
  }) => {
    // 缓存 transition 避免重复创建
    const glowTransition = anim.loop
      ? GLOW_TRANSITION_LOOP
      : GLOW_TRANSITION_ONCE

    return (
      <motion.div
        className={className || 'absolute z-10'}
        style={style || { top: `${8 * scale}px`, right: `${8 * scale}px` }}
        initial={ALBUM_COVER_INITIAL}
        animate={ALBUM_COVER_ANIMATE}
        transition={ALBUM_COVER_TRANSITION}
      >
        <div
          className="rounded-lg overflow-hidden shadow-lg ring-2 ring-white/20 dark:ring-white/10"
          style={{ width: `${48 * scale}px`, height: `${48 * scale}px` }}
        >
          {cover ? (
            <img
              key={cover}
              src={cover}
              alt={name}
              className="w-full h-full object-cover"
              loading="eager"
              onError={(e) => {
                e.currentTarget.style.display = 'none'
                e.currentTarget.nextElementSibling?.classList.remove('hidden')
              }}
            />
          ) : null}
          <div
            className={`w-full h-full flex items-center justify-center bg-gray-200 dark:bg-neutral-700 text-gray-400 dark:text-neutral-500 ${cover ? 'hidden' : ''}`}
          >
            <svg className="w-6 h-6" fill="currentColor" viewBox="0 0 24 24">
              <path d="M12 3v10.55c-.59-.34-1.27-.55-2-.55-2.21 0-4 1.79-4 4s1.79 4 4 4 4-1.79 4-4V7h4V3h-6z" />
            </svg>
          </div>
        </div>
        {/* 播放状态光晕 */}
        {isPlaying && (
          <motion.div
            className="absolute inset-0 rounded-lg pointer-events-none"
            style={{ boxShadow: `0 0 20px ${themeColor}40` }}
            animate={GLOW_ANIMATE}
            transition={glowTransition}
          />
        )}
      </motion.div>
    )
  },
)

AlbumCover.displayName = 'AlbumCover'

/** 小组件播放指示：共享频谱 + 入场动效 */
const PlayingIndicator = memo(
  ({
    themeColor,
    scale = 1,
    anim,
    isPlaying = false,
  }: {
    themeColor: string
    scale?: number
    anim?: AnimationConfig
    isPlaying?: boolean
  }) => {
    const useSpectrum = (anim?.level ?? 'standard') === 'standard'

    return (
      <motion.div
        initial={INDICATOR_INITIAL}
        animate={INDICATOR_ANIMATE}
        transition={INDICATOR_TRANSITION}
      >
        <PlayingSpectrum
          themeColor={themeColor}
          scale={scale}
          isPlaying={isPlaying}
          useSpectrum={useSpectrum}
        />
      </motion.div>
    )
  },
)

PlayingIndicator.displayName = 'PlayingIndicator'

export const MusicPlayerWidget = memo(
  ({ config, isEditMode: _isEditMode, isPreview }: MusicPlayerWidgetProps) => {
    const { containerRef, scale, fontScale } = useWidgetSize(
      config.size,
      isPreview ? 1 : undefined,
    )
    const playerControl = useMusicPlayerControl()
    const anim = useAnimationLevel()
    const { t } = useI18n()

    const currentSong = isPreview
      ? {
          name: t.musicPlayer.sampleSong,
          artist: t.musicPlayer.sampleArtist,
          cover: '',
          duration: 180,
          id: '0',
          url: '',
          source: 'netease' as const,
          isVip: false,
        }
      : playerControl.currentSong

    const isEnabled = isPreview ? true : playerControl.isEnabled
    const isPlaying = isPreview ? false : playerControl.isPlaying
    const musicColor = isPreview ? '#ef4444' : playerControl.musicColor

    // 🆕 使用触发式动画 - isPlaying 变化时重新触发动画，并持续循环
    const animationTrigger = useRef(0)

    // 当 isPlaying 变为 true 时增加 trigger 计数，持续触发动画
    useEffect(() => {
      if (!isPlaying || !anim.loop) return

      // 立即触发一次
      animationTrigger.current += 1

      // 设置间隔定时器，每 4 秒重新触发动画
      const intervalId = setInterval(() => {
        animationTrigger.current += 1
      }, 4000)

      return () => clearInterval(intervalId)
    }, [isPlaying, anim.loop])

    const { isAnimating } = useLoopAnimation({
      duration: 4000, // 光效动画约4秒周期
      trigger: isPlaying ? animationTrigger.current : 'stopped', // 播放时持续触发
      enabled: anim.loop && isPlaying, // 低端设备禁用，且只在播放时启用
    })

    const canAnimate = anim.loop && isAnimating && isPlaying

    const handleClick = useCallback(() => {
      if (isPreview) return
      // 打开全局控制面板
      window.dispatchEvent(new Event('open-control-panel'))
    }, [isPreview])

    const handleTogglePlay = useCallback(
      (e: React.MouseEvent) => {
        e.stopPropagation()
        if (isPreview) return
        window.dispatchEvent(new Event('toggle-play-pause'))
      },
      [isPreview],
    )

    const themeColor = currentSong ? musicColor : '#ef4444'

    const previewLyrics = useMemo<LyricLine[]>(
      () => [
        { time: 0, text: t.musicPlayer.sampleLyricPrev },
        { time: 5, text: t.musicPlayer.sampleLyricCurrent },
        { time: 10, text: t.musicPlayer.sampleLyricNext },
      ],
      [
        t.musicPlayer.sampleLyricCurrent,
        t.musicPlayer.sampleLyricNext,
        t.musicPlayer.sampleLyricPrev,
      ],
    )

    const lyrics = isPreview ? previewLyrics : playerControl.lyrics
    const verbatimLyrics = isPreview ? [] : playerControl.verbatimLyrics
    const currentLyricIndex = isPreview ? 1 : playerControl.currentLyricIndex

    if (!isEnabled) {
      return (
        <WidgetShell
          containerRef={containerRef}
          scale={scale}
          padding={12}
          contentClassName="flex flex-col items-center justify-center"
          background={
            <GlowBackground
              color={themeColor}
              animLevel={anim.level}
              shouldAnimate={false}
              variant="single"
              size="md"
            />
          }
        >
          <motion.span
            className="mb-2"
            style={{ fontSize: `${30 * scale}px` }}
            initial={{ scale: 0.5, opacity: 0 }}
            animate={{ scale: 1, opacity: 1 }}
            transition={{ duration: 0.6, ease: [0.34, 1.56, 0.64, 1] }}
          >
            <LuMusic
              style={{ width: `${30 * scale}px`, height: `${30 * scale}px` }}
            />
          </motion.span>
          <span
            className="text-gray-500 dark:text-gray-400"
            style={{ fontSize: `${12 * fontScale}px` }}
          >
            音乐播放器未启用
          </span>
        </WidgetShell>
      )
    }

    if (!currentSong) {
      return (
        <WidgetShell
          containerRef={containerRef}
          scale={scale}
          padding={12}
          contentClassName="flex flex-col items-center justify-center"
          background={
            <GlowBackground
              color={themeColor}
              animLevel={anim.level}
              shouldAnimate={false}
              variant="single"
              size="md"
            />
          }
        >
          <motion.span
            className="mb-2"
            style={{ fontSize: `${30 * scale}px` }}
            initial={{ scale: 0.5, opacity: 0 }}
            animate={{ scale: 1, opacity: 1 }}
            transition={{ duration: 0.6, ease: [0.34, 1.56, 0.64, 1] }}
          >
            <LuMusic
              style={{ width: `${30 * scale}px`, height: `${30 * scale}px` }}
            />
          </motion.span>
          <span
            className="text-gray-500 dark:text-gray-400"
            style={{ fontSize: `${12 * fontScale}px` }}
          >
            {t.music.noPlaying}
          </span>
        </WidgetShell>
      )
    }

    // 4x2 布局 - 上下结构重构
    if (config.size === '4x2' && currentSong) {
      return (
        <WidgetShell
          containerRef={containerRef}
          padding={0}
          className="cursor-pointer group"
          contentClassName="flex flex-col"
          rootProps={{ onClick: handleClick }}
          background={
            <motion.div
              className="absolute inset-0 opacity-20"
              style={{
                background: `linear-gradient(135deg, ${themeColor}40 0%, transparent 100%)`,
              }}
            />
          }
        >
          {/* 上半部分：歌词 (2/3) */}
          <div className="flex-1 relative w-full overflow-hidden flex items-center justify-center px-4 z-10">
            {/* 背景：封面高斯模糊 + 呼吸动效 */}
            <div className="absolute inset-0 z-0 overflow-hidden">
              <motion.div
                key={currentSong.cover}
                className={`absolute inset-0 bg-cover bg-center ${anim.level === 'standard' ? 'blur-xl' : 'blur-sm'} opacity-30 dark:opacity-20`}
                style={{
                  backgroundImage: currentSong.cover
                    ? cssBackgroundImage(currentSong.cover)
                    : undefined,
                }}
                initial={{ opacity: 0, scale: 1.2 }}
                animate={
                  anim.level === 'standard'
                    ? {
                        opacity: 0.3,
                        scale: [1.2, 1.5, 1.2], // 加大呼吸幅度
                        rotate: [0, 15, 0, -15, 0], // 增加旋转角度
                        x: [0, 20, 0, -20, 0], // 添加水平漂移
                        y: [0, -15, 0, 15, 0], // 添加垂直漂移
                      }
                    : {
                        opacity: 0.3,
                        scale: 1.2,
                      }
                }
                transition={{
                  opacity: { duration: 1 },
                  scale: {
                    duration: 20,
                    repeat: anim.loop ? Infinity : 0,
                    ease: 'easeInOut',
                  },
                  rotate: {
                    duration: 45,
                    repeat: anim.loop ? Infinity : 0,
                    ease: 'easeInOut',
                  },
                  x: {
                    duration: 25,
                    repeat: anim.loop ? Infinity : 0,
                    ease: 'easeInOut',
                  },
                  y: {
                    duration: 30,
                    repeat: anim.loop ? Infinity : 0,
                    ease: 'easeInOut',
                  },
                }}
              />
              {/* 遮罩层：增强文字对比度 */}
              <div className="absolute inset-0 bg-white/40 dark:bg-black/40 mix-blend-overlay" />
              <div className="absolute inset-0 bg-linear-to-b from-transparent to-white/10 dark:to-black/10" />

              {/* 动态光斑效果 - 低端设备完全禁用，受调度器控制 */}
              {isPlaying && canAnimate && (
                <motion.div
                  className="absolute top-1/2 left-1/2 w-full h-full -translate-x-1/2 -translate-y-1/2 bg-linear-to-tr from-white/20 to-transparent rounded-full blur-xl mix-blend-overlay"
                  animate={{
                    scale: [0.8, 1.1, 0.8],
                    opacity: [0.2, 0.4, 0.2],
                  }}
                  transition={{
                    duration: 4,
                    repeat: 1, // 有限次数
                    ease: 'easeInOut',
                  }}
                />
              )}
            </div>

            {/* 漂浮歌词显示 - 逐字漂浮效果 */}
            <div className="relative z-10 w-full h-full flex items-center justify-center">
              <FloatingLyrics
                lyrics={lyrics}
                verbatimLyrics={verbatimLyrics}
                currentLyricIndex={currentLyricIndex}
                isPlaying={isPlaying}
                themeColor={themeColor}
                fontScale={fontScale}
                animLevel={anim.level}
              />
            </div>
          </div>

          {/* 下半部分：信息 + 控制 (1/3) */}
          <div className="h-[36%] relative w-full border-t border-gray-200/10 dark:border-white/5 glass-surface glass-30 flex items-center justify-between px-4 z-20">
            <div className="flex items-center gap-3 min-w-0 flex-1 mr-2">
              {/* 封面 - 放大并向上溢出 + 悬浮动效 */}
              <motion.div
                className="relative shrink-0 origin-bottom-left"
                style={{ marginTop: `-${24 * scale}px` }}
                animate={{
                  y: isPlaying ? [0, -4, 0] : 0,
                }}
                transition={{
                  y: {
                    duration: 4,
                    repeat: anim.loop ? Infinity : 0,
                    ease: 'easeInOut',
                  },
                }}
              >
                <AlbumCover
                  cover={currentSong.cover}
                  name={currentSong.name}
                  isPlaying={isPlaying}
                  themeColor={themeColor}
                  scale={scale * 1.35} // 放大封面
                  className="relative z-10 shadow-xl rounded-lg"
                  style={{}}
                  anim={anim}
                />
              </motion.div>
              {/* 信息 - 切换时滑入动效 */}
              <div className="flex flex-col justify-center min-w-0 pr-1">
                <motion.div
                  key={currentSong.name}
                  className="font-bold text-gray-800 dark:text-gray-100 leading-tight truncate"
                  style={{ fontSize: `${14 * fontScale}px` }}
                  initial={{ opacity: 0, x: -10 }}
                  animate={{ opacity: 1, x: 0 }}
                  transition={{ duration: 0.4, ease: 'easeOut' }}
                >
                  {currentSong.name}
                </motion.div>
                <motion.div
                  key={currentSong.artist}
                  className="text-gray-600 dark:text-gray-400 truncate text-xs mt-0.5"
                  style={{ fontSize: `${11 * fontScale}px` }}
                  initial={{ opacity: 0, x: -10 }}
                  animate={{ opacity: 1, x: 0 }}
                  transition={{ duration: 0.4, delay: 0.1, ease: 'easeOut' }}
                >
                  {currentSong.artist}
                </motion.div>
              </div>
            </div>

            {/* 控制区 */}
            <div className="flex items-center gap-3 shrink-0">
              {isPlaying && (
                <PlayingIndicator
                  themeColor={themeColor}
                  scale={scale * 0.8}
                  anim={anim}
                  isPlaying={isPlaying}
                />
              )}
              <motion.button
                onClick={handleTogglePlay}
                className="rounded-full bg-white dark:bg-white/10 shadow-sm flex items-center justify-center ring-1 ring-black/5 dark:ring-white/10"
                style={{
                  color: themeColor,
                  width: `${34 * scale}px`,
                  height: `${34 * scale}px`,
                }}
                whileHover={{ scale: 1.1 }}
                whileTap={{ scale: 0.9 }}
                aria-label={isPlaying ? t.music.pause : t.music.play}
              >
                {isPlaying ? (
                  <svg
                    className="w-4 h-4"
                    fill="currentColor"
                    viewBox="0 0 24 24"
                  >
                    <path d="M6 4h4v16H6V4zm8 0h4v16h-4V4z" />
                  </svg>
                ) : (
                  <svg
                    className="w-4 h-4 ml-0.5"
                    fill="currentColor"
                    viewBox="0 0 24 24"
                  >
                    <path d="M8 5v14l11-7z" />
                  </svg>
                )}
              </motion.button>
            </div>
          </div>
        </WidgetShell>
      )
    }

    return (
      <WidgetShell
        containerRef={containerRef}
        scale={scale}
        padding={12}
        className="cursor-pointer group"
        contentClassName="flex flex-col"
        rootProps={{ onClick: handleClick }}
        background={
          <GlowBackground
            color={themeColor}
            animLevel={anim.level}
            shouldAnimate={canAnimate}
            variant="single"
            size="md"
          />
        }
      >
        {/* 右上角：专辑封面 - 浮动元素（绝对定位，不受安全区影响） */}
        <AlbumCover
          cover={currentSong.cover}
          name={currentSong.name}
          isPlaying={isPlaying}
          themeColor={themeColor}
          scale={scale}
          anim={anim}
        />

        {/* 顶部：音乐图标 */}
        <motion.div
          className="mb-1"
          initial={{ scale: 0.5, opacity: 0, rotate: -15 }}
          animate={{
            scale: 1,
            opacity: 1,
            rotate: isPlaying ? [0, 5, 0, -5, 0] : 0,
          }}
          transition={{
            scale: { duration: 0.6, ease: [0.34, 1.56, 0.64, 1] },
            opacity: { duration: 0.6 },
            rotate: isPlaying
              ? {
                  duration: 2,
                  repeat: anim.loop ? Infinity : 0,
                  ease: 'easeInOut',
                }
              : {},
          }}
        >
          <svg
            className="w-6 h-6"
            style={{
              color: themeColor,
              width: `${24 * scale}px`,
              height: `${24 * scale}px`,
            }}
            fill="currentColor"
            viewBox="0 0 24 24"
          >
            <path d="M12 3v10.55c-.59-.34-1.27-.55-2-.55-2.21 0-4 1.79-4 4s1.79 4 4 4 4-1.79 4-4V7h4V3h-6z" />
          </svg>
        </motion.div>

        {/* 中部：歌曲信息 */}
        <div className="flex-1 flex flex-col justify-center min-h-0 translate-y-1.5">
          <motion.div
            className="font-bold text-gray-800 dark:text-gray-100 truncate mb-0.5 transition-all duration-300 ease-out"
            style={{ fontSize: `${14 * fontScale}px` }}
            initial={{ x: -20, opacity: 0 }}
            animate={{ x: 0, opacity: 1 }}
            transition={{
              duration: 0.6,
              delay: 0.2,
              ease: [0.34, 1.56, 0.64, 1],
            }}
          >
            {currentSong.name}
          </motion.div>
          <motion.div
            className="text-gray-600 dark:text-gray-400 truncate transition-all duration-300 ease-out"
            style={{ fontSize: `${12 * fontScale}px` }}
            initial={{ x: -20, opacity: 0 }}
            animate={{ x: 0, opacity: 1 }}
            transition={{
              duration: 0.6,
              delay: 0.3,
              ease: [0.34, 1.56, 0.64, 1],
            }}
          >
            {currentSong.artist}
          </motion.div>
        </div>

        {/* 底部：播放控制 */}
        <motion.div
          className="flex items-center justify-between"
          initial={{ y: 10, opacity: 0 }}
          animate={{ y: 0, opacity: 1 }}
          transition={{ duration: 0.4, delay: 0.4 }}
        >
          <button
            onClick={handleTogglePlay}
            className="rounded-full glass-surface glass-80 shadow-md flex items-center justify-center hover:scale-110 transition-transform"
            style={{
              color: themeColor,
              width: `${32 * scale}px`,
              height: `${32 * scale}px`,
            }}
            aria-label={isPlaying ? t.music.pause : t.music.play}
          >
            {isPlaying ? (
              <svg
                style={{
                  width: `${14 * scale}px`,
                  height: `${14 * scale}px`,
                }}
                fill="currentColor"
                viewBox="0 0 24 24"
              >
                <path d="M6 4h4v16H6V4zm8 0h4v16h-4V4z" />
              </svg>
            ) : (
              <svg
                style={{
                  width: `${14 * scale}px`,
                  height: `${14 * scale}px`,
                }}
                fill="currentColor"
                viewBox="0 0 24 24"
              >
                <path d="M8 5v14l11-7z" />
              </svg>
            )}
          </button>

          {/* 播放状态指示器 */}
          {isPlaying && (
            <PlayingIndicator
              themeColor={themeColor}
              scale={scale}
              anim={anim}
              isPlaying={isPlaying}
            />
          )}
        </motion.div>
      </WidgetShell>
    )
  },
)

MusicPlayerWidget.displayName = 'MusicPlayerWidget'
