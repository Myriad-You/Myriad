/**
 * 分类切换容器：换设置分类时，旧页先退场，新页再进场。
 *
 * 只靠重挂载 + 进场动画是不够的——旧页会瞬间消失、新页凭空长出，
 * 看起来就是「没有切换动效」。这里把旧页多留一个出场时长，
 * 期间它不可点击，播完再换成新页。
 *
 * 方向来自调用方（侧边栏里往下选 = forward，往上 = back），
 * 具体位移由 settings-motion.css 的 --sm-switch-dir 决定。
 *
 * 高度过渡复用 AutoHeight（与平台列表↔详情同一套），
 * 本组件只负责 phase / direction / commit 时机。
 */

import type { ReactNode } from 'react'

import React, { useEffect, useRef, useState } from 'react'
import { AutoHeight } from './AutoHeight'
import { prefersReducedMotion, SETTINGS_DURATION_MS } from './motion'
import './settings-motion.css'

export type SectionSwitchDirection = 'forward' | 'back'

export interface SectionSwitchProps {
  /** 当前分类标识；变化即触发一次切换 */
  sectionKey: string
  /** 渲染指定分类的内容（退场期间仍会用旧 key 调用） */
  children: (sectionKey: string) => ReactNode
  /** 进出方向；默认 forward */
  direction?: SectionSwitchDirection
  /**
   * 新分类真正换上时触发（此时旧页已淡出，页面看不见）。
   * 调用方通常在这里把滚动位置归零：换分类等同换页，
   * 停在上一页的滚动位置会让粘顶侧栏看起来「凭空位移」。
   */
  onCommit?: (sectionKey: string) => void
  className?: string
}

export const SectionSwitch: React.FC<SectionSwitchProps> = ({
  sectionKey,
  children,
  direction = 'forward',
  onCommit,
  className = '',
}) => {
  /** 正在显示的分类：切换时先停在旧值，等出场动画播完再跟上 */
  const [shownKey, setShownKey] = useState(sectionKey)
  const shownKeyRef = useRef(sectionKey)
  /** out=旧内容退场 / in=新内容进场 / idle=落定 */
  const [phase, setPhase] = useState<'in' | 'out' | 'idle'>('idle')
  const timerRef = useRef<number | undefined>(undefined)
  /** 用 ref 存回调，避免它每次渲染换引用就重跑切换 effect */
  const onCommitRef = useRef(onCommit)
  onCommitRef.current = onCommit

  useEffect(() => {
    window.clearTimeout(timerRef.current)

    // 已经停在目标分类（含「切走又立刻切回」）：取消退场，回到常态
    if (sectionKey === shownKeyRef.current) {
      setPhase('idle')
      return undefined
    }

    const commitIn = () => {
      shownKeyRef.current = sectionKey
      setShownKey(sectionKey)
      setPhase('in')
      onCommitRef.current?.(sectionKey)
      timerRef.current = window.setTimeout(
        setPhase,
        SETTINGS_DURATION_MS.slow + 60,
        'idle',
      )
    }

    // 减弱动效：不要先出场再进场，避免空一拍。
    if (prefersReducedMotion()) {
      commitIn()
      return undefined
    }

    setPhase('out')
    const id = window.setTimeout(commitIn, SETTINGS_DURATION_MS.fast)
    timerRef.current = id
    // 目标再次变化时必须清掉当前阶段的定时器，否则旧目标或旧阶段会串进来
    return () => window.clearTimeout(timerRef.current)
  }, [sectionKey])

  useEffect(
    () => () => {
      window.clearTimeout(timerRef.current)
    },
    [],
  )

  /** 卡片内容的进场动画结束后回到 idle。 */
  const handleAnimationEnd = React.useCallback(
    (event: React.AnimationEvent<HTMLDivElement>) => {
      if (event.animationName !== 'sm-switch-in') return
      setPhase('idle')
    },
    [],
  )

  return (
    <div
      className={`sm-switch ${className}`.trim()}
      data-phase={phase}
      data-dir={direction}
      onAnimationEnd={handleAnimationEnd}
      aria-busy={phase === 'out' || undefined}
    >
      <AutoHeight contentKey={shownKey} className="sm-switch-height">
        {children(shownKey)}
      </AutoHeight>
    </div>
  )
}

SectionSwitch.displayName = 'SectionSwitch'

export default SectionSwitch
