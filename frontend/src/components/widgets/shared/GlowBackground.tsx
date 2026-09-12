import type { AnimationLevel } from '../../../hooks/useAnimationLevel'
import { memo } from 'react'
import {

  getCurrentAnimationConfig,
  isExlight,
  isStandardAnimation,
} from '../../../hooks/useAnimationLevel'
import './GlowBackground.css'

const SIZE_CLASS: Record<'sm' | 'md' | 'lg', string> = {
  sm: 'glow-size-sm',
  md: 'glow-size-md',
  lg: 'glow-size-lg',
}

export interface GlowBackgroundProps {
  color: string
  // 省略时读当前全局档，避免模块加载时冻成错误档。
  animLevel?: AnimationLevel
  shouldAnimate: boolean
  variant?: 'single' | 'dual' | 'single-left'
  size?: 'sm' | 'md' | 'lg'
  opacity?: number
}

export const GlowBackground = memo(
  ({
    color,
    animLevel: animLevelProp,
    shouldAnimate,
    variant = 'single',
    size = 'md',
    opacity,
  }: GlowBackgroundProps) => {
    const animLevel = animLevelProp ?? getCurrentAnimationConfig().level

    if (isExlight(animLevel)) {
      return null
    }

    const blurClass = isStandardAnimation(animLevel)
      ? 'glow-blur-lg'
      : 'glow-blur-sm'

    const sizeClass = SIZE_CLASS[size]

    const baseStyle = {
      '--glow-color': color,
      ...(opacity !== undefined && { '--glow-opacity': opacity }),
    } as React.CSSProperties

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

    return (
      <>
        <div
          className={`glow-orb glow-right ${sizeClass} ${blurClass} ${shouldAnimate ? 'glow-animate-primary' : 'glow-static'}`}
          style={baseStyle}
        />
        <div
          className={`glow-orb glow-left glow-secondary ${blurClass} ${shouldAnimate ? 'glow-animate-secondary' : 'glow-static'}`}
          style={baseStyle}
        />
      </>
    )
  },
)

export default GlowBackground
