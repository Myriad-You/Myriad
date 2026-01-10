/**
 * 碰撞检测 Worker Hook
 *
 * 提供便捷的 API 来使用 Web Worker 进行碰撞检测，
 * 自动处理 Worker 生命周期和消息通信
 */

import { useCallback, useEffect, useRef } from 'react'

/** Widget 简化数据（用于传递给 Worker） */
export interface WidgetData {
  id: string
  position: { x: number, y: number }
  size: { w: number, h: number }
}

/** 碰撞检测结果 */
export interface CollisionResult {
  hasCollision: boolean
  collidingIds: string[]
}

/** 位置查找结果 */
export interface PositionResult {
  position: { x: number, y: number } | null
}

/** 批量检测结果 */
export interface BatchCollisionResult {
  collisions: Array<{ id1: string, id2: string }>
}

/**
 * 碰撞检测 Worker Hook
 *
 * @example
 * ```tsx
 * function WidgetGrid() {
 *   const { checkCollision, findValidPosition, isReady } = useCollisionWorker();
 *
 *   const handleDrag = async (widgetId: string, newPos: { x: number; y: number }) => {
 *     if (!isReady) return;
 *
 *     const result = await checkCollision({
 *       widgetId,
 *       position: newPos,
 *       size: { w: 2, h: 2 },
 *       allWidgets: widgets.map(w => ({ id: w.id, position: w.position, size: getSizeFromType(w.size) })),
 *       gridWidth: 16,
 *       gridHeight: 4,
 *       excludeId: widgetId,
 *     });
 *
 *     if (!result.hasCollision) {
 *       // 更新位置
 *     }
 *   };
 * }
 * ```
 */
export function useCollisionWorker() {
  const workerRef = useRef<Worker | null>(null)
  const isReadyRef = useRef(false)
  const pendingCallbacksRef = useRef<Map<string, (data: any) => void>>(new Map())
  const requestIdRef = useRef(0)

  // 初始化 Worker
  useEffect(() => {
    // 检查 Worker 支持
    if (typeof Worker === 'undefined') {
      console.warn('[useCollisionWorker] Web Workers not supported')
      return
    }

    try {
      // 创建内联 Worker（避免额外的网络请求）
      const workerCode = `
        // AABB 碰撞检测
        function checkAABBCollision(widget, other) {
          const { x, y } = widget.position;
          const { w, h } = widget.size;
          const { x: ox, y: oy } = other.position;
          const { w: ow, h: oh } = other.size;
          return x < ox + ow && x + w > ox && y < oy + oh && y + h > oy;
        }

        // 检查单个组件的碰撞
        function checkCollision(widget, allWidgets, gridWidth, gridHeight, excludeId) {
          const { x, y } = widget.position;
          const { w, h } = widget.size;
          const collidingIds = [];

          // 检查边界
          if (x < 0 || y < 0 || x + w > gridWidth || y + h > gridHeight) {
            return { hasCollision: true, collidingIds: ['_boundary'] };
          }

          // 检查与其他组件的碰撞
          for (const other of allWidgets) {
            if (other.id === excludeId) continue;
            if (checkAABBCollision(widget, other)) {
              collidingIds.push(other.id);
            }
          }

          return { hasCollision: collidingIds.length > 0, collidingIds };
        }

        // 查找有效位置
        function findValidPosition(widgetSize, allWidgets, gridWidth, gridHeight, preferredPosition) {
          const { w, h } = widgetSize;

          // 优先尝试首选位置
          if (preferredPosition) {
            const testWidget = { position: preferredPosition, size: widgetSize };
            const result = checkCollision(testWidget, allWidgets, gridWidth, gridHeight);
            if (!result.hasCollision) {
              return preferredPosition;
            }
          }

          // 扫描网格寻找有效位置
          for (let y = 0; y <= gridHeight - h; y++) {
            for (let x = 0; x <= gridWidth - w; x++) {
              const testWidget = { position: { x, y }, size: widgetSize };
              const result = checkCollision(testWidget, allWidgets, gridWidth, gridHeight);
              if (!result.hasCollision) {
                return { x, y };
              }
            }
          }

          return null;
        }

        // 批量检测
        function batchCheckCollisions(widgets, gridWidth, gridHeight) {
          const collisions = [];
          for (let i = 0; i < widgets.length; i++) {
            for (let j = i + 1; j < widgets.length; j++) {
              if (checkAABBCollision(widgets[i], widgets[j])) {
                collisions.push({ id1: widgets[i].id, id2: widgets[j].id });
              }
            }
          }
          return { collisions };
        }

        self.onmessage = (e) => {
          const { type, requestId, payload } = e.data;

          switch (type) {
            case 'checkCollision': {
              const { widgetId, position, size, allWidgets, gridWidth, gridHeight, excludeId } = payload;
              const result = checkCollision({ position, size }, allWidgets, gridWidth, gridHeight, excludeId);
              self.postMessage({ type: 'collisionResult', requestId, payload: { widgetId, ...result } });
              break;
            }

            case 'findValidPosition': {
              const { widgetSize, allWidgets, gridWidth, gridHeight, preferredPosition } = payload;
              const position = findValidPosition(widgetSize, allWidgets, gridWidth, gridHeight, preferredPosition);
              self.postMessage({ type: 'positionResult', requestId, payload: { position } });
              break;
            }

            case 'batchCheckCollisions': {
              const { widgets, gridWidth, gridHeight } = payload;
              const result = batchCheckCollisions(widgets, gridWidth, gridHeight);
              self.postMessage({ type: 'batchResult', requestId, payload: result });
              break;
            }
          }
        };
      `

      const blob = new Blob([workerCode], { type: 'application/javascript' })
      const workerUrl = URL.createObjectURL(blob)
      workerRef.current = new Worker(workerUrl)

      // 处理 Worker 响应
      workerRef.current.onmessage = (e) => {
        const { requestId, payload } = e.data
        const callback = pendingCallbacksRef.current.get(requestId)
        if (callback) {
          callback(payload)
          pendingCallbacksRef.current.delete(requestId)
        }
      }

      workerRef.current.onerror = (error) => {
        console.error('[useCollisionWorker] Worker error:', error)
      }

      isReadyRef.current = true

      // 清理
      return () => {
        workerRef.current?.terminate()
        URL.revokeObjectURL(workerUrl)
        workerRef.current = null
        isReadyRef.current = false
        pendingCallbacksRef.current.clear()
      }
    }
    catch (error) {
      console.error('[useCollisionWorker] Failed to create worker:', error)
    }
  }, [])

  // 发送消息并等待响应
  const sendMessage = useCallback(<T>(type: string, payload: any): Promise<T> => {
    return new Promise((resolve, reject) => {
      if (!workerRef.current || !isReadyRef.current) {
        reject(new Error('Worker not ready'))
        return
      }

      const requestId = `req_${++requestIdRef.current}`
      pendingCallbacksRef.current.set(requestId, resolve)

      // 设置超时
      setTimeout(() => {
        if (pendingCallbacksRef.current.has(requestId)) {
          pendingCallbacksRef.current.delete(requestId)
          reject(new Error('Worker timeout'))
        }
      }, 5000)

      workerRef.current.postMessage({ type, requestId, payload })
    })
  }, [])

  // 碰撞检测 API
  const checkCollision = useCallback(
    (params: {
      widgetId: string
      position: { x: number, y: number }
      size: { w: number, h: number }
      allWidgets: WidgetData[]
      gridWidth: number
      gridHeight: number
      excludeId?: string
    }): Promise<CollisionResult> => {
      return sendMessage('checkCollision', params)
    },
    [sendMessage],
  )

  // 查找有效位置 API
  const findValidPosition = useCallback(
    (params: {
      widgetSize: { w: number, h: number }
      allWidgets: WidgetData[]
      gridWidth: number
      gridHeight: number
      preferredPosition?: { x: number, y: number }
    }): Promise<PositionResult> => {
      return sendMessage('findValidPosition', params)
    },
    [sendMessage],
  )

  // 批量检测 API
  const batchCheckCollisions = useCallback(
    (params: {
      widgets: WidgetData[]
      gridWidth: number
      gridHeight: number
    }): Promise<BatchCollisionResult> => {
      return sendMessage('batchCheckCollisions', params)
    },
    [sendMessage],
  )

  return {
    checkCollision,
    findValidPosition,
    batchCheckCollisions,
    isReady: isReadyRef.current,
  }
}

/**
 * 同步碰撞检测（主线程，用于不支持 Worker 的环境或简单场景）
 */
export function checkCollisionSync(
  widget: { position: { x: number, y: number }, size: { w: number, h: number } },
  allWidgets: WidgetData[],
  gridWidth: number,
  gridHeight: number,
  excludeId?: string,
): CollisionResult {
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

    const { x: ox, y: oy } = other.position
    const { w: ow, h: oh } = other.size

    // AABB 碰撞检测
    if (x < ox + ow && x + w > ox && y < oy + oh && y + h > oy) {
      collidingIds.push(other.id)
    }
  }

  return {
    hasCollision: collidingIds.length > 0,
    collidingIds,
  }
}

export default useCollisionWorker
