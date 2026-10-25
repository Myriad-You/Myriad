import type { RefObject } from 'react'
import { useLayoutEffect, useState } from 'react'
import {
  clearFigureHolds,
  figureStageFor,
  snapshotFigure,
  subscribeFigureStage,
} from '../../features/merope/presence/figureStage'

/**
 * 播放器把画布的显示尺寸写成上一个容器的像素值，要等 ResizeObserver 才改。
 * 挪过去的这一刻先放开，让样式表的 100% 接管：形象盒子在卡片和舞台里比例相同，
 * 旧分辨率的画面等比缩进新盒子里，弹窗紧接着复制卡片画面时她也是对的大小。
 */
function releaseCanvasSize(container: HTMLElement): void {
  for (const canvas of container.querySelectorAll('canvas')) {
    canvas.style.width = ''
    canvas.style.height = ''
  }
}

/**
 * 卡片的活形象渲染进这里给的容器（用 portal），容器自己在卡片和弹窗舞台之间挪：
 * React 不重挂，播放器不重建。挪动在舞台登记、注销的通知里同步完成。
 *
 * 离开舞台时在舞台上留一帧画面，回到舞台时拿掉。
 */
export function useFigurePlacement(
  ownerId: string,
  homeRef: RefObject<HTMLElement | null>,
): { container: HTMLElement; onStage: boolean } {
  const [container] = useState(() => {
    const element = document.createElement('div')
    element.className = 'merope-widget__figure'
    return element
  })
  const [onStage, setOnStage] = useState(false)

  useLayoutEffect(() => {
    const place = () => {
      const home = homeRef.current
      const target = figureStageFor(ownerId) ?? home
      if (!target || container.parentElement === target) return
      const from = container.parentElement
      if (from && from !== home && from.isConnected) from.append(snapshotFigure(container))
      if (target !== home) clearFigureHolds(target)
      target.append(container)
      releaseCanvasSize(container)
      setOnStage(target !== home)
    }
    place()
    const unsubscribe = subscribeFigureStage(place)
    return () => {
      unsubscribe()
      container.remove()
    }
  }, [container, homeRef, ownerId])

  return { container, onStage }
}
