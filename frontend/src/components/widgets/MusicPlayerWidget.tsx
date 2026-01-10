/**
 * 音乐播放器小组件
 * Glass风格设计，2x2紧凑布局
 */

import type { AnimationConfig } from '../../hooks/useAnimationLevel'
import type { LyricLine } from '../../utils/musicPlayer'
import type { WidgetConfig } from '../WidgetGrid'
import { motionShim as motion } from '@lib/motionShim'
import { memo, useCallback, useEffect, useId, useMemo, useRef, useState } from 'react'
import { useI18n } from '../../contexts/I18nContext'
import { useMusicPlayerControl } from '../../contexts/MusicPlayerContext'
import { isPageVisible, onVisibility, useLoopAnimation } from '../../hooks/animation'
import { useAnimationLevel } from '../../hooks/useAnimationLevel'
import { useWidgetSize } from '../../hooks/useWidgetSize'
import { audioManager, getCurrentLyricIndex, getNeteaseLyrics, getQQLyrics } from '../../utils/musicPlayer'

// ==================== 漂浮歌词组件 ====================

interface FloatingChar {
  char: string
  index: number
  absoluteTime: number
  seed: number
}

// 漂浮歌词显示组件 - 逐字淡入，分批显示
const FloatingLyrics = memo(({
  lyrics,
  currentLyricIndex,
  isPlaying,
  themeColor,
  fontScale,
}: {
  lyrics: LyricLine[]
  currentLyricIndex: number
  isPlaying: boolean
  themeColor: string
  fontScale: number
}) => {
  const containerRef = useRef<HTMLDivElement>(null)
  const charsRef = useRef<(HTMLSpanElement | null)[]>([])
  const animationRef = useRef<number | null>(null)
  const pageVisibleRef = useRef(isPageVisible())
  const phaseRef = useRef(0)

  // 每批显示的最大字符数
  const MAX_CHARS_PER_BATCH = 10

  // 当前播放时间 - 使用 ref 减少重渲染
  const currentTimeRef = useRef(0)
  // 用于触发批次更新的时间戳（降低更新频率）
  const [batchTrigger, setBatchTrigger] = useState(0)
  // 当前批次是否正在淡出
  const [isFadingOut, setIsFadingOut] = useState(false)
  // 频谱驱动的节奏进度调制
  const rhythmProgressRef = useRef(0)
  const lastBeatTimeRef = useRef(0)
  const energyHistoryRef = useRef<number[]>([])

  // 构建所有字符的时间映射 - 英文单词作为整体
  const allCharsWithTime = useMemo(() => {
    const chars: FloatingChar[] = []

    if (lyrics.length === 0)
      return chars

    // 将文本分割为词元（中文逐字，英文逐词，数字逐组）
    const tokenize = (text: string): string[] => {
      const tokens: string[] = []
      let i = 0
      while (i < text.length) {
        const char = text[i]
        // 英文字母：收集整个单词
        if (/[a-z]/i.test(char)) {
          let word = ''
          while (i < text.length && /[a-z']/i.test(text[i])) {
            word += text[i]
            i++
          }
          tokens.push(word)
        }
        // 数字：收集整个数字组
        else if (/\d/.test(char)) {
          let num = ''
          while (i < text.length && /[0-9.,]/.test(text[i])) {
            num += text[i]
            i++
          }
          tokens.push(num)
        }
        // 其他字符（中文、标点、空格等）：逐个处理
        else {
          tokens.push(char)
          i++
        }
      }
      return tokens
    }

    let globalIndex = 0

    // 智能时长计算：根据歌词长度推算合理显示时间
    // 每个字符约 0.35 秒，英文单词按实际字符数计算
    const CHAR_DISPLAY_TIME = 0.35
    // 偏差阈值：实际时间超过推算时间的倍数时才限制
    const DEVIATION_THRESHOLD = 2.0

    lyrics.forEach((line, lineIdx) => {
      const nextLine = lyrics[lineIdx + 1]
      const lineStartTime = line.time
      const lineEndTime = nextLine ? nextLine.time : lineStartTime + 5
      const actualDuration = lineEndTime - lineStartTime
      const text = line.text

      // 计算推荐时长：基于实际字符数（不是词元数）
      const charCount = text.replace(/\s/g, '').length // 不计空格
      const recommendedDuration = charCount * CHAR_DISPLAY_TIME

      // 计算最终时长
      let lineDuration: number
      if (actualDuration > recommendedDuration * DEVIATION_THRESHOLD) {
        // 实际时间远大于推荐时间（可能是间奏）
        // 使用推荐时间的中上值（1.3 ~ 1.5 倍）作为上限
        lineDuration = recommendedDuration * 1.4
      }
      else {
        // 正常情况：使用实际时间
        lineDuration = actualDuration
      }

      const tokens = tokenize(text)
      const tokenCount = tokens.length

      if (tokenCount === 0)
        return

      tokens.forEach((token, tokenIdx) => {
        const tokenTime = lineStartTime + (tokenIdx / tokenCount) * lineDuration
        const seed = globalIndex * 17 + token.charCodeAt(0)

        chars.push({
          char: token,
          index: globalIndex,
          absoluteTime: tokenTime,
          seed,
        })

        globalIndex++
      })
    })

    return chars
  }, [lyrics])

  // 同步当前播放时间 - 使用 ref + 节流的批次触发
  useEffect(() => {
    if (!isPlaying)
      return

    const audio = audioManager.getCurrentAudio()
    if (!audio)
      return

    let rafId: number | null = null
    let lastBatchUpdate = 0
    const BATCH_UPDATE_INTERVAL = 200 // 每200ms检查一次批次变化

    const updateTime = (timestamp: number) => {
      currentTimeRef.current = audio.currentTime

      // 节流批次更新
      if (timestamp - lastBatchUpdate >= BATCH_UPDATE_INTERVAL) {
        setBatchTrigger(audio.currentTime)
        lastBatchUpdate = timestamp
      }

      rafId = requestAnimationFrame(updateTime)
    }

    rafId = requestAnimationFrame(updateTime)

    return () => {
      if (rafId)
        cancelAnimationFrame(rafId)
    }
  }, [isPlaying])

  // 计算当前应该显示的字符批次 - 使用节流的 batchTrigger
  const { visibleChars, batchStartTime, batchEndTime } = useMemo(() => {
    if (allCharsWithTime.length === 0) {
      return { visibleChars: [], batchStartTime: 0, batchEndTime: 0 }
    }

    const time = batchTrigger

    // 找到当前时间对应的字符索引
    let currentCharIndex = 0
    for (let i = 0; i < allCharsWithTime.length; i++) {
      if (allCharsWithTime[i].absoluteTime <= time) {
        currentCharIndex = i
      }
      else {
        break
      }
    }

    // 计算当前批次的起始索引（每批 MAX_CHARS_PER_BATCH 个字符）
    const batchIndex = Math.floor(currentCharIndex / MAX_CHARS_PER_BATCH)
    const batchStart = batchIndex * MAX_CHARS_PER_BATCH
    const batchEnd = Math.min(batchStart + MAX_CHARS_PER_BATCH, allCharsWithTime.length)

    const chars = allCharsWithTime.slice(batchStart, batchEnd)
    const startTime = chars.length > 0 ? chars[0].absoluteTime : 0
    const endTime = chars.length > 0 ? chars[chars.length - 1].absoluteTime : 0

    return {
      visibleChars: chars,
      batchStartTime: startTime,
      batchEndTime: endTime,
    }
  }, [allCharsWithTime, batchTrigger])

  // 检测批次切换，触发淡出
  const prevBatchRef = useRef<number>(-1)
  useEffect(() => {
    if (allCharsWithTime.length === 0)
      return

    const currentCharIndex = allCharsWithTime.findIndex(c => c.absoluteTime > batchTrigger) - 1
    const batchIndex = Math.floor(Math.max(0, currentCharIndex) / MAX_CHARS_PER_BATCH)

    if (prevBatchRef.current !== -1 && prevBatchRef.current !== batchIndex) {
      // 批次变化，触发淡出
      setIsFadingOut(true)
      setTimeout(() => setIsFadingOut(false), 300)
    }

    prevBatchRef.current = batchIndex
  }, [batchTrigger, allCharsWithTime])

  // 计算字符布局位置 - 从左到右排列，填满区域，随机间隔
  const charPositions = useMemo(() => {
    const positions: { x: number, y: number, fontSize: number, rotation: number }[] = []

    if (visibleChars.length === 0)
      return positions

    const seededRandom = (seed: number) => {
      const x = Math.sin(seed * 9999) * 10000
      return x - Math.floor(x)
    }

    // 计算累积的随机间隔
    let cumulativeX = 4 // 起始位置
    const totalChars = visibleChars.length
    const availableWidth = 92 // 可用宽度百分比 (4% ~ 96%)

    // 先计算所有权重 - 英文单词根据长度加权
    const weights: number[] = []
    let totalWeight = 0
    visibleChars.forEach((char, idx) => {
      const tokenLength = char.char.length
      const isEnglishWord = tokenLength > 1 && /^[a-z]/i.test(char.char)

      // 基础随机权重：英文单词 0.9~1.3，其他 0.7~1.3
      const baseMin = isEnglishWord ? 0.9 : 0.7
      const baseWeight = baseMin + seededRandom(char.seed + 100) * 0.4

      // 根据词元长度计算额外权重
      // 单个字符（中文、标点）= 1，英文单词按字符数计算
      // 英文单词长度权重：每个额外字符增加 0.6 的权重
      const lengthMultiplier = 1 + (tokenLength - 1) * 0.6

      const weight = baseWeight * lengthMultiplier
      weights.push(weight)
      totalWeight += weight
    })

    // 根据权重分配位置
    visibleChars.forEach((char, idx) => {
      // 当前字符的位置
      const x = cumulativeX

      // 垂直位置：在整个区域内随机分布，调整分布使上下更均匀
      const yRandom = seededRandom(char.seed)
      // 使用平方根来让分布更均匀（偏向上方补偿）
      const y = 0 + Math.sqrt(yRandom) * 45 // 0% ~ 45%

      // 随机字体大小：0.85 ~ 1.25 倍
      const fontSizeRatio = 0.85 + seededRandom(char.seed + 200) * 0.4

      // 随机旋转角度：-8° ~ 8°
      const rotation = (seededRandom(char.seed + 300) - 0.5) * 16

      positions.push({ x, y, fontSize: fontSizeRatio, rotation })

      // 计算下一个字符的位置（基于权重的间隔）
      const charWidth = (weights[idx] / totalWeight) * availableWidth
      cumulativeX += charWidth
    })

    return positions
  }, [visibleChars])

  // 柔和的微浮动画 + 频谱节奏检测
  useEffect(() => {
    if (!isPlaying) {
      if (animationRef.current) {
        cancelAnimationFrame(animationRef.current)
        animationRef.current = null
      }
      return
    }

    const unsubscribe = onVisibility((visible) => {
      pageVisibleRef.current = visible
    })

    let lastUpdateTime = 0
    const UPDATE_INTERVAL = 50 // ~20fps 足够柔和

    // 节奏检测参数
    const ENERGY_HISTORY_SIZE = 8
    const BEAT_THRESHOLD = 1.3 // 能量比平均值高30%认为是节拍
    const BEAT_COOLDOWN = 150 // 节拍冷却时间ms

    const updateAnimation = (timestamp: number) => {
      if (!pageVisibleRef.current || !isPlaying) {
        animationRef.current = null
        return
      }

      // 确定性随机函数
      const seededRandom = (seed: number) => {
        const x = Math.sin(seed * 9999) * 10000
        return x - Math.floor(x)
      }

      if (timestamp - lastUpdateTime >= UPDATE_INTERVAL) {
        phaseRef.current += 0.015 // 非常慢的相位变化
        const phase = phaseRef.current

        // 获取频谱数据进行节奏检测
        const spectrum = audioManager.getSpectrumData()
        const currentEnergy = (spectrum[0] + spectrum[1] + spectrum[2] + spectrum[3]) / 4

        // 维护能量历史
        energyHistoryRef.current.push(currentEnergy)
        if (energyHistoryRef.current.length > ENERGY_HISTORY_SIZE) {
          energyHistoryRef.current.shift()
        }

        // 计算平均能量
        const avgEnergy = energyHistoryRef.current.reduce((a, b) => a + b, 0) / energyHistoryRef.current.length

        // 节拍检测：当前能量显著高于平均值
        const isBeat = currentEnergy > avgEnergy * BEAT_THRESHOLD
          && currentEnergy > 0.15 // 最低能量阈值
          && (timestamp - lastBeatTimeRef.current) > BEAT_COOLDOWN

        if (isBeat) {
          lastBeatTimeRef.current = timestamp
          // 节拍时加速节奏进度
          rhythmProgressRef.current += 0.08
        }
        else {
          // 缓慢衰减回基础进度
          rhythmProgressRef.current *= 0.95
        }

        // 节奏调制值 (0 ~ 0.3)
        const rhythmModulation = Math.min(0.3, rhythmProgressRef.current)

        charsRef.current.forEach((el, idx) => {
          if (!el || idx >= visibleChars.length)
            return

          const charData = visibleChars[idx]
          if (charData.char === ' ') {
            el.style.opacity = '0'
            return
          }

          // 基础时间差 - 使用 ref 避免依赖
          const baseTimeDiff = charData.absoluteTime - currentTimeRef.current
          // 应用节奏调制：节拍时字符提前显示
          const timeDiff = baseTimeDiff - rhythmModulation * 0.5

          const seed = charData.seed

          // 柔和的浮动效果 - 像水中的气泡，不同字符有不同的浮动幅度
          const floatAmplitude = 3 + seededRandom(seed + 500) * 2 // 3~5px 随机幅度
          const floatY = Math.sin(phase + seed * 0.1) * floatAmplitude
          const floatX = Math.cos(phase * 0.7 + seed * 0.15) * (floatAmplitude * 0.6)

          // 状态判断 - 考虑节奏调制
          const isActive = timeDiff >= -0.2 && timeDiff <= 0.1
          const isPast = timeDiff < -0.2
          const isFuture = timeDiff > 0.1

          let opacity = 1
          let scale = 1

          if (isFuture) {
            // 未到 - 透明，但节拍时可能微微显现
            const peekOpacity = isBeat ? 0.15 : 0
            opacity = peekOpacity
            scale = 0.95
          }
          else if (isActive) {
            // 正在唱 - 完全显示
            opacity = 1
            // 节拍时稍微放大
            scale = 1.02 + (isBeat ? 0.05 : 0)
            el.style.color = themeColor
            const glowSize = isBeat ? 12 : 8
            el.style.textShadow = `0 0 ${glowSize}px ${themeColor}60, 0 1px 2px rgba(0,0,0,0.1)`
          }
          else if (isPast) {
            // 已过 - 保持显示但稍淡
            opacity = 0.7
            scale = 1
            el.style.color = ''
            el.style.textShadow = 'none'
          }

          // 批次淡出时全部透明
          if (isFadingOut) {
            opacity = 0
          }

          const rotation = el.dataset.rotation || '0'
          el.style.transform = `translate(${floatX}px, ${floatY}px) scale(${scale}) rotate(${rotation}deg)`
          el.style.opacity = `${opacity}`
        })

        lastUpdateTime = timestamp
      }

      animationRef.current = requestAnimationFrame(updateAnimation)
    }

    animationRef.current = requestAnimationFrame(updateAnimation)

    return () => {
      unsubscribe()
      if (animationRef.current) {
        cancelAnimationFrame(animationRef.current)
        animationRef.current = null
      }
    }
  }, [isPlaying, visibleChars, themeColor, isFadingOut])

  // 重置 refs
  useEffect(() => {
    charsRef.current = []
  }, [visibleChars.length])

  if (visibleChars.length === 0 && lyrics.length > 0) {
    return (
      <div className="text-sm text-gray-400 dark:text-gray-500 opacity-40">...</div>
    )
  }

  if (lyrics.length === 0) {
    return (
      <div className="text-sm text-gray-500 dark:text-gray-400">暂无歌词</div>
    )
  }

  return (
    <div
      ref={containerRef}
      className="relative w-full h-full overflow-hidden"
      style={{ minHeight: '60px' }}
    >
      {visibleChars.map((charConfig, idx) => {
        const pos = charPositions[idx] || { x: 50, y: 50, fontSize: 1, rotation: 0 }

        return (
          <span
            key={`${charConfig.index}-${charConfig.seed}`}
            ref={(el) => { charsRef.current[idx] = el }}
            data-rotation={pos.rotation}
            className="absolute font-semibold text-gray-700 dark:text-gray-200 pointer-events-none select-none"
            style={{
              left: `${pos.x}%`,
              top: `${pos.y}%`,
              fontSize: `${22 * fontScale * pos.fontSize}px`,
              opacity: 0,
              transform: `translate(0, 0) scale(1) rotate(${pos.rotation}deg)`,
              willChange: 'transform, opacity',
              transition: 'opacity 0.3s ease-out, color 0.2s ease, text-shadow 0.2s ease, transform 0.4s ease-out',
            }}
          >
            {charConfig.char}
          </span>
        )
      })}
    </div>
  )
})

FloatingLyrics.displayName = 'FloatingLyrics'

// ==================== 静态动画常量（避免每次渲染创建新对象）====================

// 封面入场动画
const ALBUM_COVER_INITIAL = { scale: 0.5, opacity: 0, rotate: -15 }
const ALBUM_COVER_ANIMATE = { scale: 1, opacity: 1, rotate: 0 }
const ALBUM_COVER_TRANSITION = { duration: 0.6, ease: [0.34, 1.56, 0.64, 1] }

// 播放状态光晕动画 - 有限次数，配合调度器 duration=4000ms
const GLOW_ANIMATE = { opacity: [0.5, 1, 0.5] }
const GLOW_TRANSITION_LOOP = { duration: 2, repeat: 1, ease: 'easeInOut' as const } // 2轮=4s
const GLOW_TRANSITION_ONCE = { duration: 2, repeat: 0, ease: 'easeInOut' as const }

// 播放指示器动画 - 有限次数
const INDICATOR_INITIAL = { opacity: 0, scale: 0.8 }
const INDICATOR_ANIMATE = { opacity: 1, scale: 1 }
const INDICATOR_TRANSITION = { duration: 0.3 }

const BAR_ANIMATE_1 = { height: ['30%', '100%', '30%'] }
const BAR_ANIMATE_2 = { height: ['60%', '100%', '60%'] }
const BAR_ANIMATE_3 = { height: ['40%', '100%', '40%'] }
const BAR_TRANSITION_LOOP = (delay: number) => ({ duration: 0.6, repeat: 6, ease: 'easeInOut' as const, delay }) // 6轮≈4s
const BAR_TRANSITION_ONCE = (delay: number) => ({ duration: 0.6, repeat: 0, ease: 'easeInOut' as const, delay })

// 背景光效动画 - 有限次数
const BG_GLOW_ANIMATE = { opacity: [0.1, 0.2, 0.1], scale: [1, 1.15, 1] }
const BG_GLOW_TRANSITION = { duration: 3, repeat: 1, ease: 'easeInOut' as const } // 1轮=3s

// 光斑动画 - 有限次数
const LIGHT_SPOT_ANIMATE = { scale: [0.8, 1.1, 0.8], opacity: [0.2, 0.4, 0.2] }
const LIGHT_SPOT_TRANSITION = { duration: 4, repeat: 0, ease: 'easeInOut' as const } // 1轮

// 封面浮动动画 - 有限次数
const COVER_FLOAT_ANIMATE_PLAYING = { y: [0, -4, 0] }
const COVER_FLOAT_ANIMATE_STATIC = { y: 0 }
const COVER_FLOAT_TRANSITION_LOOP = { y: { duration: 4, repeat: 0, ease: 'easeInOut' as const } }
const COVER_FLOAT_TRANSITION_ONCE = { y: { duration: 4, repeat: 0, ease: 'easeInOut' as const } }

// 歌曲切换动画
const SONG_SLIDE_INITIAL = { opacity: 0, x: -10 }
const SONG_SLIDE_ANIMATE = { opacity: 1, x: 0 }
const SONG_SLIDE_TRANSITION = { duration: 0.4, ease: 'easeOut' as const }
const SONG_SLIDE_TRANSITION_DELAY = { duration: 0.4, delay: 0.1, ease: 'easeOut' as const }

// 歌词切换动画
const LYRIC_INITIAL = { opacity: 0, y: 10, scale: 0.95 }
const LYRIC_ANIMATE = { opacity: 1, y: 0, scale: 1 }
const LYRIC_EXIT = { opacity: 0, y: -10, scale: 0.95 }
const LYRIC_TRANSITION = { duration: 0.4 }

// 底部控制区入场
const CONTROL_INITIAL = { y: 10, opacity: 0 }
const CONTROL_ANIMATE = { y: 0, opacity: 1 }
const CONTROL_TRANSITION = { duration: 0.4, delay: 0.4 }

// 音乐图标入场
const MUSIC_ICON_INITIAL = { scale: 0.5, opacity: 0, rotate: -15 }
const MUSIC_ICON_ANIMATE_STATIC = { scale: 1, opacity: 1, rotate: 0 }
const MUSIC_ICON_ANIMATE_PLAYING = { scale: 1, opacity: 1, rotate: [0, 5, 0, -5, 0] }

// 歌曲信息入场
const INFO_INITIAL = { x: -20, opacity: 0 }
const INFO_ANIMATE = { x: 0, opacity: 1 }
const INFO_TRANSITION_1 = { duration: 0.6, delay: 0.2, ease: [0.34, 1.56, 0.64, 1] }
const INFO_TRANSITION_2 = { duration: 0.6, delay: 0.3, ease: [0.34, 1.56, 0.64, 1] }

export interface MusicPlayerWidgetProps {
  config: WidgetConfig
  isEditMode: boolean
  isPreview?: boolean
}

// 专辑封面组件 - 独立优化（使用静态动画常量）
const AlbumCover = memo(({
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
  const glowTransition = anim.loop ? GLOW_TRANSITION_LOOP : GLOW_TRANSITION_ONCE

  return (
    <motion.div
      className={className || 'absolute z-10'}
      style={style || { top: `${8 * scale}px`, right: `${8 * scale}px` }}
      initial={ALBUM_COVER_INITIAL}
      animate={ALBUM_COVER_ANIMATE}
      transition={ALBUM_COVER_TRANSITION}
    >
      <div
        className="rounded-md overflow-hidden shadow-lg ring-2 ring-white/20 dark:ring-white/10 backdrop-blur-sm"
        style={{ width: `${48 * scale}px`, height: `${48 * scale}px` }}
      >
        {cover
          ? (
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
            )
          : null}
        <div className={`w-full h-full flex items-center justify-center bg-gray-200 dark:bg-neutral-700 text-gray-400 dark:text-neutral-500 ${cover ? 'hidden' : ''}`}>
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
})

AlbumCover.displayName = 'AlbumCover'

// 播放状态指示器 - 高性能实时频谱版本
const PlayingIndicator = memo(({
  themeColor,
  scale = 1,
  isPlaying = false,
}: {
  themeColor: string
  scale?: number
  anim?: AnimationConfig
  isPlaying?: boolean
}) => {
  const bar1Ref = useRef<HTMLDivElement>(null)
  const bar2Ref = useRef<HTMLDivElement>(null)
  const bar3Ref = useRef<HTMLDivElement>(null)
  const bar4Ref = useRef<HTMLDivElement>(null)
  const animationRef = useRef<number | null>(null)
  const connectedRef = useRef(false)
  const pageVisibleRef = useRef(isPageVisible())

  // 频谱动画循环 - 统一处理
  useEffect(() => {
    // 监听页面可见性变化
    const unsubscribe = onVisibility((visible) => {
      pageVisibleRef.current = visible
    })

    if (!isPlaying) {
      if (animationRef.current) {
        cancelAnimationFrame(animationRef.current)
        animationRef.current = null
      }
      if (bar1Ref.current)
        bar1Ref.current.style.height = '30%'
      if (bar2Ref.current)
        bar2Ref.current.style.height = '50%'
      if (bar3Ref.current)
        bar3Ref.current.style.height = '40%'
      if (bar4Ref.current)
        bar4Ref.current.style.height = '35%'
      return unsubscribe
    }

    // 尝试连接音频到分析器
    if (!connectedRef.current) {
      const audio = audioManager.getCurrentAudio()
      if (audio) {
        audioManager.connectAudioToAnalyser(audio)
        connectedRef.current = true
      }
    }

    let lastUpdateTime = 0
    const UPDATE_INTERVAL = 60

    const updateSpectrum = (timestamp: number) => {
      if (!pageVisibleRef.current || !isPlaying) {
        animationRef.current = null
        return
      }

      if (timestamp - lastUpdateTime >= UPDATE_INTERVAL) {
        const data = audioManager.getSpectrumData()
        if (bar1Ref.current)
          bar1Ref.current.style.height = `${30 + data[0] ** 2.0 * 70}%`
        if (bar2Ref.current)
          bar2Ref.current.style.height = `${30 + data[1] ** 2.0 * 70}%`
        if (bar3Ref.current)
          bar3Ref.current.style.height = `${30 + data[2] ** 2.0 * 70}%`
        if (bar4Ref.current)
          bar4Ref.current.style.height = `${30 + data[3] ** 2.0 * 70}%`
        lastUpdateTime = timestamp
      }
      animationRef.current = requestAnimationFrame(updateSpectrum)
    }

    if (pageVisibleRef.current) {
      animationRef.current = requestAnimationFrame(updateSpectrum)
    }

    return () => {
      unsubscribe()
      if (animationRef.current) {
        cancelAnimationFrame(animationRef.current)
        animationRef.current = null
      }
    }
  }, [isPlaying])

  // 缓存样式对象
  const containerStyle = useMemo(() => ({
    gap: `${2 * scale}px`,
    height: `${16 * scale}px`,
  }), [scale])

  const barStyle = useMemo(() => ({
    background: themeColor,
    width: `${2 * scale}px`,
    transition: 'height 0.06s linear',
  }), [themeColor, scale])

  return (
    <motion.div
      className="flex items-end"
      style={containerStyle}
      initial={INDICATOR_INITIAL}
      animate={INDICATOR_ANIMATE}
      transition={INDICATOR_TRANSITION}
    >
      <div ref={bar1Ref} className="rounded-full" style={{ ...barStyle, height: '30%' }} />
      <div ref={bar2Ref} className="rounded-full" style={{ ...barStyle, height: '50%' }} />
      <div ref={bar3Ref} className="rounded-full" style={{ ...barStyle, height: '40%' }} />
      <div ref={bar4Ref} className="rounded-full" style={{ ...barStyle, height: '35%' }} />
    </motion.div>
  )
})

PlayingIndicator.displayName = 'PlayingIndicator'

export const MusicPlayerWidget = memo(({ config, isEditMode, isPreview }: MusicPlayerWidgetProps) => {
  const { containerRef, scale, fontScale } = useWidgetSize(config.size, isPreview ? 1 : undefined)
  const playerControl = useMusicPlayerControl()
  const anim = useAnimationLevel()
  const uniqueId = useId()
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
    if (!isPlaying || !anim.loop)
      return

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
    if (isPreview)
      return
    // 打开全局控制面板
    window.dispatchEvent(new Event('open-control-panel'))
  }, [isPreview])

  const handleTogglePlay = useCallback((e: React.MouseEvent) => {
    e.stopPropagation()
    if (isPreview)
      return
    window.dispatchEvent(new Event('toggle-play-pause'))
  }, [isPreview])

  const themeColor = currentSong ? musicColor : '#ef4444'

  // 歌词状态
  const [lyrics, setLyrics] = useState<LyricLine[]>([])
  const [currentLyricIndex, setCurrentLyricIndex] = useState(-1)

  // 获取歌词
  useEffect(() => {
    if (isPreview) {
      setLyrics([
        { time: 0, text: t.musicPlayer.sampleLyricPrev },
        { time: 5, text: t.musicPlayer.sampleLyricCurrent },
        { time: 10, text: t.musicPlayer.sampleLyricNext },
      ])
      setCurrentLyricIndex(1)
      return
    }

    if (!currentSong) {
      setLyrics([])
      setCurrentLyricIndex(-1)
      return
    }

    const fetchLyrics = async () => {
      let lines: LyricLine[] = []
      try {
        if (currentSong.source === 'netease') {
          lines = await getNeteaseLyrics(currentSong.id)
        }
        else if (currentSong.source === 'qq') {
          lines = await getQQLyrics(currentSong.id)
        }
      }
      catch (e) {
        console.error('Failed to fetch lyrics', e)
      }
      setLyrics(lines)
    }

    fetchLyrics()
  }, [currentSong?.id, currentSong?.source, isPreview])

  // 同步歌词进度 - 添加节流优化
  useEffect(() => {
    if (isPreview)
      return

    if (!isPlaying || !currentSong || lyrics.length === 0)
      return

    const audio = audioManager.getCurrentAudio()
    if (!audio)
      return

    // 使用节流避免过于频繁的状态更新
    let lastUpdateTime = 0
    const THROTTLE_MS = 100 // 100ms 节流

    const handleTimeUpdate = () => {
      const now = Date.now()
      if (now - lastUpdateTime < THROTTLE_MS)
        return
      lastUpdateTime = now

      const index = getCurrentLyricIndex(lyrics, audio.currentTime)
      setCurrentLyricIndex((prev) => {
        if (prev !== index)
          return index
        return prev
      })
    }

    audio.addEventListener('timeupdate', handleTimeUpdate)
    return () => audio.removeEventListener('timeupdate', handleTimeUpdate)
  }, [isPlaying, currentSong, lyrics, isPreview])

  if (!isEnabled) {
    return (
      <div ref={containerRef} className="relative h-full w-full rounded-xl overflow-hidden glass">
        {/* 背景光效 - 低端设备使用 blur-xl 减少性能消耗 */}
        <div
          className={`absolute -right-8 -top-8 w-32 h-32 rounded-full opacity-10 ${anim.level === 'standard' ? 'blur-3xl' : 'blur-xl'}`}
          style={{ background: themeColor }}
        />
        <div className="absolute inset-0 flex flex-col items-center justify-center p-3" style={{ padding: `${12 * scale}px` }}>
          <motion.span
            className="mb-2"
            style={{ fontSize: `${30 * scale}px` }}
            initial={{ scale: 0.5, opacity: 0 }}
            animate={{ scale: 1, opacity: 1 }}
            transition={{ duration: 0.6, ease: [0.34, 1.56, 0.64, 1] }}
          >
            🎵
          </motion.span>
          <span
            className="text-gray-500 dark:text-gray-400"
            style={{ fontSize: `${12 * fontScale}px` }}
          >
            音乐播放器未启用
          </span>
        </div>
      </div>
    )
  }

  if (!currentSong) {
    return (
      <div ref={containerRef} className="relative h-full w-full rounded-xl overflow-hidden glass">
        {/* 背景光效 - 低端设备使用 blur-xl */}
        <div
          className={`absolute -right-8 -top-8 w-32 h-32 rounded-full opacity-10 ${anim.level === 'standard' ? 'blur-3xl' : 'blur-xl'}`}
          style={{ background: themeColor }}
        />
        <div className="absolute inset-0 flex flex-col items-center justify-center p-3" style={{ padding: `${12 * scale}px` }}>
          <motion.span
            className="mb-2"
            style={{ fontSize: `${30 * scale}px` }}
            initial={{ scale: 0.5, opacity: 0 }}
            animate={{ scale: 1, opacity: 1 }}
            transition={{ duration: 0.6, ease: [0.34, 1.56, 0.64, 1] }}
          >
            🎵
          </motion.span>
          <span
            className="text-gray-500 dark:text-gray-400"
            style={{ fontSize: `${12 * fontScale}px` }}
          >
            {t.music.noPlaying}
          </span>
        </div>
      </div>
    )
  }

  // 4x2 布局 - 上下结构重构
  if (config.size === '4x2' && currentSong) {
    return (
      <div
        ref={containerRef}
        className="relative h-full w-full rounded-xl overflow-hidden glass cursor-pointer group flex flex-col"
        onClick={handleClick}
      >
        {/* 全局背景光效 */}
        <motion.div
          className="absolute inset-0 opacity-20"
          style={{
            background: `linear-gradient(135deg, ${themeColor}40 0%, transparent 100%)`,
          }}
        />

        {/* 上半部分：歌词 (2/3) */}
        <div className="flex-1 relative w-full overflow-hidden flex items-center justify-center px-4 z-10">
          {/* 背景：封面高斯模糊 + 呼吸动效 */}
          <div className="absolute inset-0 z-0 overflow-hidden">
            <motion.div
              key={currentSong.cover}
              className={`absolute inset-0 bg-cover bg-center ${anim.level === 'standard' ? 'blur-xl' : 'blur-sm'} opacity-30 dark:opacity-20`}
              style={{ backgroundImage: `url(${currentSong.cover || ''})` }}
              initial={{ opacity: 0, scale: 1.2 }}
              animate={anim.level === 'standard' ? {
                opacity: 0.3,
                scale: [1.2, 1.5, 1.2], // 加大呼吸幅度
                rotate: [0, 15, 0, -15, 0], // 增加旋转角度
                x: [0, 20, 0, -20, 0], // 添加水平漂移
                y: [0, -15, 0, 15, 0], // 添加垂直漂移
              } : {
                opacity: 0.3,
                scale: 1.2,
              }}
              transition={{
                opacity: { duration: 1 },
                scale: { duration: 20, repeat: anim.loop ? Infinity : 0, ease: 'easeInOut' },
                rotate: { duration: 45, repeat: anim.loop ? Infinity : 0, ease: 'easeInOut' },
                x: { duration: 25, repeat: anim.loop ? Infinity : 0, ease: 'easeInOut' },
                y: { duration: 30, repeat: anim.loop ? Infinity : 0, ease: 'easeInOut' },
              }}
            />
            {/* 遮罩层：增强文字对比度 */}
            <div className="absolute inset-0 bg-white/40 dark:bg-black/40 mix-blend-overlay" />
            <div className="absolute inset-0 bg-gradient-to-b from-transparent to-white/10 dark:to-black/10" />

            {/* 动态光斑效果 - 低端设备完全禁用，受调度器控制 */}
            {isPlaying && canAnimate && (
              <motion.div
                className="absolute top-1/2 left-1/2 w-full h-full -translate-x-1/2 -translate-y-1/2 bg-gradient-to-tr from-white/20 to-transparent rounded-full blur-xl mix-blend-overlay"
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
              currentLyricIndex={currentLyricIndex}
              isPlaying={isPlaying}
              themeColor={themeColor}
              fontScale={fontScale}
            />
          </div>
        </div>

        {/* 下半部分：信息 + 控制 (1/3) */}
        <div className="h-[36%] relative w-full border-t border-gray-200/10 dark:border-white/5 bg-white/30 dark:bg-black/20 backdrop-blur-md flex items-center justify-between px-4 z-20">
          <div className="flex items-center gap-3 min-w-0 flex-1 mr-2">
            {/* 封面 - 放大并向上溢出 + 悬浮动效 */}
            <motion.div
              className="relative shrink-0 origin-bottom-left"
              style={{ marginTop: `-${24 * scale}px` }}
              animate={{
                y: isPlaying ? [0, -4, 0] : 0,
              }}
              transition={{
                y: { duration: 4, repeat: anim.loop ? Infinity : 0, ease: 'easeInOut' },
              }}
            >
              <AlbumCover
                cover={currentSong.cover}
                name={currentSong.name}
                isPlaying={isPlaying}
                themeColor={themeColor}
                scale={scale * 1.35} // 放大封面
                className="relative z-10 shadow-xl rounded-md"
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
            {isPlaying && <PlayingIndicator themeColor={themeColor} scale={scale * 0.8} anim={anim} isPlaying={isPlaying} />}
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
              {isPlaying
                ? (
                    <svg className="w-4 h-4" fill="currentColor" viewBox="0 0 24 24"><path d="M6 4h4v16H6V4zm8 0h4v16h-4V4z" /></svg>
                  )
                : (
                    <svg className="w-4 h-4 ml-0.5" fill="currentColor" viewBox="0 0 24 24"><path d="M8 5v14l11-7z" /></svg>
                  )}
            </motion.button>
          </div>
        </div>
      </div>
    )
  }

  return (
    <div
      ref={containerRef}
      className="relative h-full w-full rounded-xl overflow-hidden glass cursor-pointer group"
      onClick={handleClick}
    >
      {/* 背景光效 - 低端设备禁用动画和减少 blur，受调度器控制 */}
      {canAnimate ? (
        <motion.div
          className="absolute -right-8 -top-8 w-32 h-32 rounded-full blur-xl"
          style={{ background: themeColor }}
          animate={{
            opacity: [0.1, 0.2, 0.1],
            scale: [1, 1.15, 1],
          }}
          transition={{
            duration: 3,
            repeat: 1, // 有限次数
            ease: 'easeInOut',
          }}
        />
      ) : (
        <div
          className="absolute -right-8 -top-8 w-32 h-32 rounded-full blur-xl opacity-10"
          style={{ background: themeColor }}
        />
      )}

      {/* 右上角：专辑封面 - 浮动元素 */}
      <AlbumCover
        cover={currentSong.cover}
        name={currentSong.name}
        isPlaying={isPlaying}
        themeColor={themeColor}
        scale={scale}
        anim={anim}
      />

      {/* 主内容区：2x2紧凑布局 */}
      <div className="absolute inset-0 flex flex-col p-3" style={{ padding: `${12 * scale}px` }}>
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
            style={{ color: themeColor, width: `${24 * scale}px`, height: `${24 * scale}px` }}
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
            transition={{ duration: 0.6, delay: 0.2, ease: [0.34, 1.56, 0.64, 1] }}
          >
            {currentSong.name}
          </motion.div>
          <motion.div
            className="text-gray-600 dark:text-gray-400 truncate transition-all duration-300 ease-out"
            style={{ fontSize: `${12 * fontScale}px` }}
            initial={{ x: -20, opacity: 0 }}
            animate={{ x: 0, opacity: 1 }}
            transition={{ duration: 0.6, delay: 0.3, ease: [0.34, 1.56, 0.64, 1] }}
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
            className="rounded-full bg-white/80 dark:bg-black/80 backdrop-blur-sm shadow-md flex items-center justify-center hover:scale-110 transition-transform"
            style={{
              color: themeColor,
              width: `${32 * scale}px`,
              height: `${32 * scale}px`,
            }}
            aria-label={isPlaying ? t.music.pause : t.music.play}
          >
            {isPlaying
              ? (
                  <svg style={{ width: `${14 * scale}px`, height: `${14 * scale}px` }} fill="currentColor" viewBox="0 0 24 24">
                    <path d="M6 4h4v16H6V4zm8 0h4v16h-4V4z" />
                  </svg>
                )
              : (
                  <svg style={{ width: `${14 * scale}px`, height: `${14 * scale}px` }} fill="currentColor" viewBox="0 0 24 24">
                    <path d="M8 5v14l11-7z" />
                  </svg>
                )}
          </button>

          {/* 播放状态指示器 */}
          {isPlaying && <PlayingIndicator themeColor={themeColor} scale={scale} anim={anim} isPlaying={isPlaying} />}
        </motion.div>
      </div>
    </div>
  )
})

MusicPlayerWidget.displayName = 'MusicPlayerWidget'
