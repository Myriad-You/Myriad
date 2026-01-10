/**
 * 光晕背景组件 - 纯 CSS 动画版本
 *
 * 性能优化策略：
 * 1. 使用 React.memo 避免父组件状态变化导致重渲染
 * 2. 使用 will-change 提示浏览器启用 GPU 加速
 * 3. 使用 transform3d 强制创建合成层
 * 4. 使用纯 CSS @keyframes 动画，无 JS 运行时开销
 * 5. 移除调度器依赖，CSS 动画由浏览器原生优化
 */

import { memo, useMemo } from 'react'
import { getAnimationConfigSync } from '../../../hooks/useAnimationLevel'
import './GlowBackground.css'

// 🔑 模块加载时同步获取动画配置，确保首次渲染正确
const INITIAL_ANIM_CONFIG = getAnimationConfigSync()

// ========== 组件接口 ==========

export interface GlowBackgroundProps {
  /** 光晕颜色 (CSS 颜色值或 CSS 变量) */
  color: string
  /** 动画级别 */
  animLevel: 'none' | 'light' | 'standard'
  /** 是否启用动画 */
  shouldAnimate: boolean
  /**
   * 布局变体：
   * - 'single': 单光晕 (右上角)
   * - 'dual': 双光晕 (右上 + 左下)
   * - 'single-left': 单光晕 (左下角)
   */
  variant?: 'single' | 'dual' | 'single-left'
  /** 光晕大小 (sm/md/lg) */
  size?: 'sm' | 'md' | 'lg'
  /** 自定义透明度 (0-1) */
  opacity?: number
}

/**
 * 光晕背景组件 - 纯 CSS 动画版本
 */
export const GlowBackground = memo(({
  color,
  animLevel,
  shouldAnimate,
  variant = 'single',
  size = 'md',
  opacity,
}: GlowBackgroundProps) => {
  // 根据动画级别选择模糊程度
  const blurClass = INITIAL_ANIM_CONFIG.level === 'standard' ? 'glow-blur-lg' : 'glow-blur-sm'

  // 根据 size 确定尺寸类
  const sizeClass = useMemo(() => {
    switch (size) {
      case 'sm': return 'glow-size-sm'
      case 'lg': return 'glow-size-lg'
      default: return 'glow-size-md'
    }
  }, [size])

  // 基础样式
  const baseStyle = useMemo(() => ({
    background: color,
    ...(opacity !== undefined && { '--glow-opacity': opacity }),
  } as React.CSSProperties), [color, opacity])

  // 根据 variant 渲染不同布局
  if (variant === 'single-left') {
    return (
      <div
        className={`glow-orb glow-left ${sizeClass} ${blurClass} ${shouldAnimate ? 'glow-animate-primary' : 'glow-static'}`}
        style={baseStyle}
      />
    )
  }

  if (variant === 'single') {
    return (
      <div
        className={`glow-orb glow-right ${sizeClass} ${blurClass} ${shouldAnimate ? 'glow-animate-primary' : 'glow-static'}`}
        style={baseStyle}
      />
    )
  }

  // dual variant
  return (
    <>
      {/* 第一个光晕 - 右上角 */}
      <div
        className={`glow-orb glow-right ${sizeClass} ${blurClass} ${shouldAnimate ? 'glow-animate-primary' : 'glow-static'}`}
        style={baseStyle}
      />
      {/* 第二个光晕 - 左下角 */}
      <div
        className={`glow-orb glow-left glow-secondary ${blurClass} ${shouldAnimate ? 'glow-animate-secondary' : 'glow-static'}`}
        style={baseStyle}
      />
    </>
  )
})

export default GlowBackground
