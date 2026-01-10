/**
 * 统一骨架屏过渡组件
 * 用于内容加载和切换时的平滑过渡
 *
 * 性能优化：
 * - 定时器自动清理防止内存泄漏
 * - 过渡完成后释放 GPU 资源
 * - 使用 visibility 代替完全移除 DOM
 */

import type { ReactNode } from 'react'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useAnimationLevel } from '../hooks/useAnimationLevel'
import './Skeleton.css'

/** 过渡阶段 */
const enum TransitionPhase {
  SKELETON = 'skeleton', // 显示骨架屏
  FADING = 'fading', // 骨架屏淡出中
  CONTENT = 'content', // 显示内容
}

interface SkeletonTransitionProps {
  /** 是否显示骨架屏 */
  loading: boolean
  /** 骨架屏内容 */
  skeleton: ReactNode
  /** 实际内容 */
  children: ReactNode
  /** 过渡延迟（毫秒） */
  delay?: number
  /** 最小显示时间（毫秒），避免闪烁 */
  minDuration?: number
  /** 自定义类名 */
  className?: string
}

export default function SkeletonTransition({
  loading,
  skeleton,
  children,
  delay = 0,
  minDuration = 300,
  className = '',
}: SkeletonTransitionProps) {
  const [phase, setPhase] = useState<TransitionPhase>(
    loading ? TransitionPhase.SKELETON : TransitionPhase.CONTENT,
  )
  const startTimeRef = useRef<number>(0)
  const timersRef = useRef<Set<ReturnType<typeof setTimeout>>>(new Set())

  // 安全的 setTimeout，自动跟踪和清理
  const safeSetTimeout = useCallback((fn: () => void, ms: number) => {
    const id = setTimeout(() => {
      timersRef.current.delete(id)
      fn()
    }, ms)
    timersRef.current.add(id)
    return id
  }, [])

  // 清理所有定时器
  useEffect(() => {
    return () => {
      timersRef.current.forEach(id => clearTimeout(id))
      timersRef.current.clear()
    }
  }, [])

  useEffect(() => {
    if (loading) {
      // 开始加载
      setPhase(TransitionPhase.SKELETON)
      startTimeRef.current = Date.now()
    }
    else {
      // 加载完成，检查是否满足最小显示时间
      const elapsed = Date.now() - startTimeRef.current
      const remaining = Math.max(0, minDuration - elapsed)

      safeSetTimeout(() => {
        // 开始淡出
        setPhase(TransitionPhase.FADING)

        // 延迟后显示内容
        safeSetTimeout(() => {
          setPhase(TransitionPhase.CONTENT)
        }, delay + 300) // 300ms 是 CSS 过渡时间
      }, remaining)
    }
  }, [loading, delay, minDuration, safeSetTimeout])

  const showSkeleton = phase === TransitionPhase.SKELETON || phase === TransitionPhase.FADING
  const contentReady = phase === TransitionPhase.CONTENT

  return (
    <div className={`skeleton-transition-container ${className}`}>
      {/* 骨架屏层 */}
      <div
        className={`skeleton-transition-layer ${showSkeleton ? 'skeleton-visible' : 'skeleton-hidden'}`}
      >
        {skeleton}
      </div>

      {/* 内容层 */}
      <div
        className={`skeleton-transition-layer content-layer ${contentReady ? 'content-visible' : 'content-hidden'}`}
      >
        {children}
      </div>
    </div>
  )
}

/**
 * 简化版：仅用于标签切换等快速过渡
 */
interface QuickTransitionProps {
  /** 是否处于过渡状态 */
  transitioning: boolean
  /** 内容 */
  children: ReactNode
  /** 自定义类名 */
  className?: string
}

export function QuickTransition({
  transitioning,
  children,
  className = '',
}: QuickTransitionProps) {
  return (
    <div
      className={`quick-transition ${transitioning ? 'transitioning' : 'visible'} ${className}`}
    >
      {children}
    </div>
  )
}

/**
 * 微粒消散效果骨架屏
 * 高级灰透明质感，介于透明与磨砂之间
 *
 * 性能优化：
 * - useMemo 缓存粒子数据，避免每次渲染重新生成
 * - 使用统一的 useAnimationLevel 判断设备性能
 * - 使用 CSS contain 隔离布局计算
 */

// 预生成的随机种子，避免每次渲染不同
const PARTICLE_SEED = Array.from({ length: 80 }, (_, i) => ({
  angle: (i * 2.39996) % (Math.PI * 2), // 黄金角分布
  distFactor: 0.3 + (i % 10) * 0.07,
  sizeFactor: 0.3 + (i % 4) * 0.2,
  delayFactor: (i % 8) * 0.5,
  durationFactor: 0.4 + (i % 6) * 0.1,
}))

interface LiquidGlassSkeletonProps {
  /** 加载文本 */
  text?: string
  /** 自定义类名 */
  className?: string
}

export function LiquidGlassSkeleton({
  text = '加载中',
  className = '',
}: LiquidGlassSkeletonProps) {
  // 使用统一的性能判断
  const { level } = useAnimationLevel()

  // 根据动画级别决定粒子数量
  const particles = useMemo(() => {
    // none: 禁用动画时完全不显示粒子
    // light: 低端设备减少粒子
    // standard: 完整粒子效果
    const config = {
      none: { count: 0, baseDistance: 0 },
      light: { count: 30, baseDistance: 250 },
      standard: { count: 80, baseDistance: 400 },
    }[level]

    if (config.count === 0)
      return []

    return PARTICLE_SEED.slice(0, config.count).map((seed, i) => {
      const distance = config.baseDistance * seed.distFactor + 200
      return {
        id: i,
        size: seed.sizeFactor * 3 + 1,
        startX: 50,
        startY: 50,
        tx: Math.cos(seed.angle) * distance,
        ty: Math.sin(seed.angle) * distance,
        delay: seed.delayFactor,
        duration: seed.durationFactor * 6 + 4,
      }
    })
  }, [level])

  return (
    <div className={`liquid-glass-skeleton ${className}`}>
      {/* 网格噪点背景 */}
      <div className="noise-grid" />

      {/* 微粒消散层 */}
      <div className="liquid-wave">
        <div className="particle-layer">
          {particles.map(particle => (
            <div
              key={particle.id}
              className="particle"
              style={{
                'width': `${particle.size}px`,
                'height': `${particle.size}px`,
                'left': `${particle.startX}%`,
                'top': `${particle.startY}%`,
                '--tx': `${particle.tx}px`,
                '--ty': `${particle.ty}px`,
                'animationDelay': `${particle.delay}s`,
                'animationDuration': `${particle.duration}s`,
              } as React.CSSProperties}
            />
          ))}
        </div>
      </div>

      {/* 内容区 */}
      <div className="glass-content">
        {/* 呼吸光环 */}
        <div className="pulse-ring" />

        {/* 加载文本 */}
        <div className="loading-text">
          {text}
        </div>

        {/* 消散点 */}
        <div className="loading-dots">
          <div className="loading-dot" />
          <div className="loading-dot" />
          <div className="loading-dot" />
          <div className="loading-dot" />
          <div className="loading-dot" />
        </div>
      </div>
    </div>
  )
}
