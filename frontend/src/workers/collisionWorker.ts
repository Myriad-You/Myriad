/**
 * 碰撞检测 Web Worker
 *
 * 将计算密集型的碰撞检测逻辑移至后台线程，
 * 避免阻塞主线程影响 UI 响应性
 */

// Worker 消息类型
interface CollisionCheckMessage {
  type: 'checkCollision'
  payload: {
    widgetId: string
    position: { x: number, y: number }
    size: { w: number, h: number }
    allWidgets: Array<{
      id: string
      position: { x: number, y: number }
      size: { w: number, h: number }
    }>
    gridWidth: number
    gridHeight: number
    excludeId?: string
  }
}

interface FindPositionMessage {
  type: 'findValidPosition'
  payload: {
    widgetSize: { w: number, h: number }
    allWidgets: Array<{
      id: string
      position: { x: number, y: number }
      size: { w: number, h: number }
    }>
    gridWidth: number
    gridHeight: number
    preferredPosition?: { x: number, y: number }
  }
}

interface BatchCollisionMessage {
  type: 'batchCheckCollisions'
  payload: {
    widgets: Array<{
      id: string
      position: { x: number, y: number }
      size: { w: number, h: number }
    }>
    gridWidth: number
    gridHeight: number
  }
}

type WorkerMessage = CollisionCheckMessage | FindPositionMessage | BatchCollisionMessage

// 碰撞检测核心算法（AABB）
function checkAABBCollision(
  widget: { position: { x: number, y: number }, size: { w: number, h: number } },
  other: { position: { x: number, y: number }, size: { w: number, h: number } },
): boolean {
  const { x, y } = widget.position
  const { w, h } = widget.size
  const { x: ox, y: oy } = other.position
  const { w: ow, h: oh } = other.size

  return (
    x < ox + ow
    && x + w > ox
    && y < oy + oh
    && y + h > oy
  )
}

// 检查单个组件的碰撞
function checkCollision(
  widget: { position: { x: number, y: number }, size: { w: number, h: number } },
  allWidgets: Array<{
    id: string
    position: { x: number, y: number }
    size: { w: number, h: number }
  }>,
  gridWidth: number,
  gridHeight: number,
  excludeId?: string,
): { hasCollision: boolean, collidingIds: string[] } {
  const { x, y } = widget.position
  const { w, h } = widget.size
  const collidingIds: string[] = []

  // 检查边界
  if (x < 0 || y < 0 || x + w > gridWidth || y + h > gridHeight) {
    return { hasCollision: true, collidingIds: ['_boundary'] }
  }

  // 检查与其他组件的碰撞
  for (const other of allWidgets) {
    if (other.id === excludeId)
      continue

    if (checkAABBCollision(widget, other)) {
      collidingIds.push(other.id)
    }
  }

  return {
    hasCollision: collidingIds.length > 0,
    collidingIds,
  }
}

// 查找有效位置（贪心算法）
function findValidPosition(
  widgetSize: { w: number, h: number },
  allWidgets: Array<{
    id: string
    position: { x: number, y: number }
    size: { w: number, h: number }
  }>,
  gridWidth: number,
  gridHeight: number,
  preferredPosition?: { x: number, y: number },
): { x: number, y: number } | null {
  const { w, h } = widgetSize

  // 优先尝试首选位置
  if (preferredPosition) {
    const testWidget = { position: preferredPosition, size: widgetSize }
    const result = checkCollision(testWidget, allWidgets, gridWidth, gridHeight)
    if (!result.hasCollision) {
      return preferredPosition
    }
  }

  // 扫描网格寻找有效位置
  for (let y = 0; y <= gridHeight - h; y++) {
    for (let x = 0; x <= gridWidth - w; x++) {
      const testWidget = { position: { x, y }, size: widgetSize }
      const result = checkCollision(testWidget, allWidgets, gridWidth, gridHeight)
      if (!result.hasCollision) {
        return { x, y }
      }
    }
  }

  return null // 没有有效位置
}

// 批量检测所有组件间的碰撞
function batchCheckCollisions(
  widgets: Array<{
    id: string
    position: { x: number, y: number }
    size: { w: number, h: number }
  }>,
  gridWidth: number,
  gridHeight: number,
): { collisions: Array<{ id1: string, id2: string }> } {
  const collisions: Array<{ id1: string, id2: string }> = []

  for (let i = 0; i < widgets.length; i++) {
    for (let j = i + 1; j < widgets.length; j++) {
      if (checkAABBCollision(widgets[i], widgets[j])) {
        collisions.push({ id1: widgets[i].id, id2: widgets[j].id })
      }
    }
  }

  return { collisions }
}

// Worker 消息处理
self.onmessage = (e: MessageEvent<WorkerMessage>) => {
  const { type, payload } = e.data

  switch (type) {
    case 'checkCollision': {
      const { widgetId, position, size, allWidgets, gridWidth, gridHeight, excludeId } = payload
      const result = checkCollision(
        { position, size },
        allWidgets,
        gridWidth,
        gridHeight,
        excludeId,
      )
      self.postMessage({
        type: 'collisionResult',
        payload: {
          widgetId,
          ...result,
        },
      })
      break
    }

    case 'findValidPosition': {
      const { widgetSize, allWidgets, gridWidth, gridHeight, preferredPosition } = payload
      const position = findValidPosition(
        widgetSize,
        allWidgets,
        gridWidth,
        gridHeight,
        preferredPosition,
      )
      self.postMessage({
        type: 'positionResult',
        payload: { position },
      })
      break
    }

    case 'batchCheckCollisions': {
      const { widgets, gridWidth, gridHeight } = payload
      const result = batchCheckCollisions(widgets, gridWidth, gridHeight)
      self.postMessage({
        type: 'batchResult',
        payload: result,
      })
      break
    }

    default:
      console.warn('[CollisionWorker] Unknown message type:', type)
  }
}

// 导出类型供 TypeScript 使用
export type { BatchCollisionMessage, CollisionCheckMessage, FindPositionMessage }
