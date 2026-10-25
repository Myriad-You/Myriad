/**
 * 人设卡的活形象只有一份播放器。弹窗要她时，卡片把装着播放器的容器整个挪到弹窗的
 * 舞台上；弹窗收回那一刻再挪回卡片。WebGL 上下文、动作和表情都不重来，
 * 尺寸由播放器自己的 ResizeObserver 跟上。
 *
 * 这里只记「谁的舞台在哪」。登记、注销都同步通知，挪动发生在通知里，
 * 赶在弹窗量卡片位置、复制卡片画面之前。
 */

interface FigureStage {
  ownerId: string
  element: HTMLElement
}

type Listener = () => void

const listeners = new Set<Listener>()
let stage: FigureStage | null = null

function emit(): void {
  for (const listener of listeners) listener()
}

/** 这张卡此刻该去的舞台；没有就待在卡片里。 */
export function figureStageFor(ownerId: string): HTMLElement | null {
  return stage?.ownerId === ownerId ? stage.element : null
}

export function showFigureOnStage(ownerId: string, element: HTMLElement): void {
  if (stage?.ownerId === ownerId && stage.element === element) return
  stage = { ownerId, element }
  emit()
}

export function releaseFigureStage(ownerId: string, element: HTMLElement): void {
  if (stage?.ownerId !== ownerId || stage.element !== element) return
  stage = null
  emit()
}

export function subscribeFigureStage(listener: Listener): () => void {
  listeners.add(listener)
  return () => {
    listeners.delete(listener)
  }
}

const HOLD_ATTRIBUTE = 'data-figure-hold'

/**
 * 她离开舞台时留下的一帧：容器的副本，画布按当前画面画好（播放器开了
 * preserveDrawingBuffer，读得出来）。弹窗缩回卡片那几百毫秒里舞台不会突然空掉。
 */
export function snapshotFigure(source: HTMLElement): HTMLElement {
  const copy = source.cloneNode(true) as HTMLElement
  const from = source.querySelectorAll('canvas')
  const to = copy.querySelectorAll('canvas')
  for (let index = 0; index < from.length && index < to.length; index++) {
    try {
      to[index].getContext('2d')?.drawImage(from[index], 0, 0)
    } catch {
      // 读不出来就空着，只有收回那几帧。
    }
  }
  copy.setAttribute(HOLD_ATTRIBUTE, '')
  copy.setAttribute('aria-hidden', 'true')
  copy.inert = true
  copy.style.pointerEvents = 'none'
  return copy
}

/** 她回到舞台时，拿掉上次留下的那一帧。 */
export function clearFigureHolds(element: HTMLElement): void {
  for (const hold of element.querySelectorAll(`:scope > [${HOLD_ATTRIBUTE}]`)) hold.remove()
}

export function resetFigureStageForTests(): void {
  stage = null
  listeners.clear()
}
