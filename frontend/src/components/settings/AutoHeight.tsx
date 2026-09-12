import type { Key, ReactNode, TransitionEvent } from 'react'

import React from 'react'
import { prefersReducedMotion, SETTINGS_DURATION_MS } from './motion'
import './settings-motion.css'

export interface AutoHeightProps {
  contentKey: Key
  children: ReactNode
  className?: string
  /** false for drill-down: skip height tween so a long page doesn't collapse */
  animate?: boolean
}

interface AutoHeightState {
  height?: number
  animating: boolean
}

/** slack after style recalc before the fallback timer */
const TRANSITION_SLACK_MS = 120

/** hard cap from start; do not extend while content keeps resizing */
const MAX_PINNED_MS = SETTINGS_DURATION_MS.slow * 3

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
  private timerId?: number
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

          // hard cap only via release; finish may renew while content is still changing
          this.deadlineId = window.setTimeout(this.release, MAX_PINNED_MS)
          this.setState(
            {
              height: targetHeight,
              animating: true,
            },
            this.startAnimationWatch,
          )
        })
      },
    )
  }

  componentWillUnmount() {
    this.clearSchedule()
  }

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

  /** don't release to auto until settled; retarget if content/layout lagged */
  private finish = () => {
    if (this.state.animating && this.state.height != null) {
      const target = this.state.height
      const content = this.measureContent()
      if (content != null && Math.abs(content - target) > 1) {
        this.setState({ height: content }, this.restartWatchdog)
        return
      }
      const rendered = this.wrapperRef.current?.getBoundingClientRect().height
      if (rendered != null && Math.abs(rendered - target) > 1) {
        this.restartWatchdog()
        return
      }
    }
    this.release()
  }

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
