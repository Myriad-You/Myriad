import type { Key, ReactNode, TransitionEvent } from 'react'

import React from 'react'
import { prefersReducedMotion, SETTINGS_DURATION_MS } from './motion'
import './settings-motion.css'

export interface AutoHeightProps {
  /** 内容身份变化时，从更新前的实际高度过渡到新高度。 */
  contentKey: Key
  children: ReactNode
  className?: string
  /** 换内容时是否过渡高度。层级下钻只走位移，关掉以免长页收成短页时顿一下。 */
  animate?: boolean
}

interface AutoHeightState {
  height?: number
  animating: boolean
}

/** transition 真正起跑还要等一次样式重算，兜底定时器留出这点余量。 */
const TRANSITION_SLACK_MS = 120

/**
 * 高度被钉住的硬上限（从起跑算起）。内容若一直改高度就不能无限续期，
 * 否则它会长时间被裁在动画里。
 */
const MAX_PINNED_MS = SETTINGS_DURATION_MS.slow * 3

/**
 * 在 DOM 更新前记录旧高度，再在新内容挂载后测量目标高度。
 * 动画结束会恢复 height: auto，后续异步内容仍可自然撑开。
 *
 * 动画期间内容自己变高（异步数据到位、图片加载完）时会跟着改目标值：
 * 不跟的话新内容被裁在旧目标里，收尾放开 height 就闪一下高度。
 */
export class AutoHeight extends React.PureComponent<
  AutoHeightProps,
  AutoHeightState
> {
  state: AutoHeightState = {
    height: undefined,
    animating: false,
  }

  private wrapperRef = React.createRef<HTMLDivElement>()
  private contentRef = React.createRef<HTMLDivElement>()
  private frameId?: number
  /** 兜底关掉动画（transitionend 没来）；每次重定目标都续期 */
  private timerId?: number
  /** 钉住高度的硬上限，不随重定目标续期 */
  private deadlineId?: number
  private observer?: ResizeObserver

  getSnapshotBeforeUpdate(previousProps: AutoHeightProps): number | null {
    if (previousProps.contentKey === this.props.contentKey) return null
    return this.wrapperRef.current?.getBoundingClientRect().height ?? null
  }

  componentDidUpdate(
    previousProps: AutoHeightProps,
    _previousState: AutoHeightState,
    previousHeight: number | null,
  ) {
    if (
      previousProps.contentKey === this.props.contentKey ||
      previousHeight == null
    ) {
      return
    }

    if (prefersReducedMotion() || this.props.animate === false) {
      this.release()
      return
    }

    this.clearSchedule()
    this.setState(
      {
        height: previousHeight,
        animating: false,
      },
      () => {
        this.frameId = window.requestAnimationFrame(() => {
          const targetHeight = this.measureContent()
          if (
            targetHeight == null ||
            Math.abs(targetHeight - previousHeight) < 1
          ) {
            this.release()
            return
          }

          // 硬上限只由 release 兜底：finish 会为「内容还在变」续期，不能无限续
          this.deadlineId = window.setTimeout(this.release, MAX_PINNED_MS)
          this.setState(
            {
              height: targetHeight,
              animating: true,
            },
            // 提交后再起兜底表 + 开始盯内容：兜底时间从高度真正落到 DOM 上算
            this.startAnimationWatch,
          )
        })
      },
    )
  }

  componentWillUnmount() {
    this.clearSchedule()
  }

  /** 内容盒当前的自然高度（不受包裹层被钉住的高度影响） */
  private measureContent(): number | undefined {
    return this.contentRef.current?.getBoundingClientRect().height
  }

  private startAnimationWatch = () => {
    this.restartWatchdog()
    const content = this.contentRef.current
    if (!content || typeof ResizeObserver === 'undefined') return
    if (!this.observer) {
      this.observer = new ResizeObserver(this.handleContentResize)
    }
    this.observer.observe(content)
  }

  private restartWatchdog = () => {
    window.clearTimeout(this.timerId)
    this.timerId = window.setTimeout(
      this.finish,
      SETTINGS_DURATION_MS.slow + TRANSITION_SLACK_MS,
    )
  }

  /**
   * 动画期间内容高度变了（异步数据、图片、字体）：改钉住的目标值，
   * 让 transition 继续走到新高度，而不是把内容裁掉、收尾时突变。
   * 内容盒是 height: auto 的块级元素，不受包裹层高度影响，不会自激。
   */
  private handleContentResize = () => {
    if (!this.state.animating) return
    const nextHeight = this.measureContent()
    if (nextHeight == null) return
    if (
      this.state.height != null &&
      Math.abs(nextHeight - this.state.height) < 1
    ) {
      return
    }
    this.setState({ height: nextHeight }, this.restartWatchdog)
  }

  private clearSchedule = () => {
    window.cancelAnimationFrame(this.frameId ?? 0)
    window.clearTimeout(this.timerId)
    window.clearTimeout(this.deadlineId)
    this.observer?.disconnect()
  }

  /**
   * 一段过渡跑完（transitionend / 兜底表）：没真到位就别放开 height，
   * 否则 auto 一放开就是一次突变。两种「没到位」：
   * - 内容高度跑偏了：异步数据刚好在收尾那一两帧落地 → 改目标再补一段
   * - 渲染高度还没追上目标：这是被重定目标顶掉的旧过渡的 transitionend
   *   （或 transition 起跑太晚）→ 续兜底表等新过渡走完
   * 两种都在硬上限内，到点由 release 强制收场。
   */
  private finish = () => {
    if (this.state.animating && this.state.height != null) {
      const target = this.state.height
      const content = this.measureContent()
      if (content != null && Math.abs(content - target) > 1) {
        this.setState({ height: content }, this.restartWatchdog)
        return
      }
      // 包裹层不带内边距/边框，全局 border-box 下 rect 高度就是钉住的值
      const rendered = this.wrapperRef.current?.getBoundingClientRect().height
      if (rendered != null && Math.abs(rendered - target) > 1) {
        this.restartWatchdog()
        return
      }
    }
    this.release()
  }

  /** 放开 height，回到 auto：异步内容后续仍可自然撑开 */
  private release = () => {
    this.clearSchedule()
    this.setState({
      height: undefined,
      animating: false,
    })
  }

  private handleTransitionEnd = (event: TransitionEvent<HTMLDivElement>) => {
    if (event.target !== event.currentTarget) return
    if (event.propertyName !== 'height') return
    this.finish()
  }

  render() {
    const { children, className = '' } = this.props
    const { height, animating } = this.state

    return (
      <div
        ref={this.wrapperRef}
        className={`sm-auto-height ${className}`.trim()}
        data-height={
          height == null ? undefined : animating ? 'animating' : 'measuring'
        }
        style={height == null ? undefined : { height: `${height}px` }}
        onTransitionEnd={this.handleTransitionEnd}
      >
        <div ref={this.contentRef} className="sm-auto-height-content">
          {children}
        </div>
      </div>
    )
  }
}

export default AutoHeight
