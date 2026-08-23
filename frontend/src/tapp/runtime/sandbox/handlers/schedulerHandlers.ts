/**
 * Scheduler 处理器
 *
 * 接通前端 TappScheduler（WebSocket 客户端）与后端已就绪的调度引擎
 * （/api/tapp/scheduler/*）。让 Tapp 通过 `Tapp.scheduler` 注册定时任务，
 * 后端到期后经 WS 推送 frontend 任务，这里转发进沙箱触发 onTask 回调。
 *
 * 懒连接：仅在 Tapp 首次调用 scheduler.* 时才初始化并建立 WebSocket，
 * 未使用调度的会话不会空开连接。后端权限（scheduler:register）由服务端强制。
 */

import type { TappInstance } from '../../../types'
import type { TappBridge } from '../../TappBridge'
import type { TaskRegistrationOptions } from '../../TappScheduler'
import { API_URL } from '../../../../config'
import { userFacingError } from '../../../../utils/userFacingError'
import { getTappScheduler } from '../../TappScheduler'

let schedulerInitialized = false

/** 懒初始化调度器（设置 apiBaseUrl 并建立 WS，仅一次） */
function ensureScheduler() {
  const scheduler = getTappScheduler()
  if (!schedulerInitialized) {
    // cookie 会话鉴权：authToken 传空，依赖同源 cookie（见 TappScheduler.apiRequest/connect）
    scheduler.initialize(`${API_URL}/api`, '')
    schedulerInitialized = true
  }
  return scheduler
}

function errResult(error: unknown) {
  return {
    success: false,
    error: userFacingError(error),
  }
}

export function registerSchedulerHandlers(
  bridge: TappBridge,
  tappInstance: TappInstance,
): () => void {
  const taskSubscriptions = new Map<string, () => void>()
  const pendingExecutions = new Map<
    number,
    {
      resolve: () => void
      reject: (error: Error) => void
      timeout: ReturnType<typeof setTimeout>
    }
  >()

  const bindTask = (taskId: string) => {
    if (taskSubscriptions.has(taskId)) return
    const scheduler = ensureScheduler()
    const unsubscribe = scheduler.onTask(
      tappInstance.id,
      taskId,
      (payload, event) => {
        return new Promise<void>((resolve, reject) => {
          if (!event.executionId) {
            reject(new Error('Scheduler execution ID missing'))
            return
          }
          const timeout = setTimeout(
            () => {
              pendingExecutions.delete(event.executionId)
              reject(new Error('Sandbox scheduler callback timed out'))
            },
            4 * 60 * 1000,
          )
          pendingExecutions.set(event.executionId, { resolve, reject, timeout })
          bridge.emit('schedulerTask', { taskId, payload, event })
        })
      },
    )
    taskSubscriptions.set(taskId, unsubscribe)
  }

  const unbindTask = (taskId: string) => {
    taskSubscriptions.get(taskId)?.()
    taskSubscriptions.delete(taskId)
  }

  bridge.registerHandler('scheduler.register', async (message) => {
    const [options] = (message.payload as { args: unknown[] }).args || []
    const opts = options as TaskRegistrationOptions | undefined
    if (!opts || !opts.taskId || !opts.scheduleType || !opts.schedule) {
      return { success: false, error: 'taskId/scheduleType/schedule required' }
    }
    try {
      const scheduler = ensureScheduler()
      let task
      try {
        task = await scheduler.registerTask(
          tappInstance.id,
          opts,
          await bridge.getRuntimeGrant(),
        )
      } catch (error) {
        // core 会在 Page/Widget/headless 生命周期中重复启动。注册操作保持幂等：
        // 若后端已有同 ID 任务，复用现有定义并重新绑定本次沙箱回调。
        task = await scheduler.getTask(
          tappInstance.id,
          opts.taskId,
          await bridge.getRuntimeGrant(),
        )
        if (!task) throw error
      }
      bindTask(opts.taskId)
      return { success: true, data: task }
    } catch (error) {
      return errResult(error)
    }
  })

  bridge.registerHandler('scheduler.unregister', async (message) => {
    const [taskId] = (message.payload as { args: unknown[] }).args || []
    if (!taskId) return { success: false, error: 'taskId required' }
    try {
      const scheduler = ensureScheduler()
      await scheduler.unregisterTask(
        tappInstance.id,
        taskId as string,
        await bridge.getRuntimeGrant(),
      )
      unbindTask(taskId as string)
      return { success: true, data: { taskId, cancelled: true } }
    } catch (error) {
      return errResult(error)
    }
  })

  bridge.registerHandler('scheduler.list', async () => {
    try {
      const scheduler = ensureScheduler()
      const tasks = await scheduler.listTasks(
        tappInstance.id,
        await bridge.getRuntimeGrant(),
      )
      return { success: true, data: tasks }
    } catch (error) {
      return errResult(error)
    }
  })

  bridge.registerHandler('scheduler.get', async (message) => {
    const [taskId] = (message.payload as { args: unknown[] }).args || []
    if (!taskId) return { success: false, error: 'taskId required' }
    try {
      const task = await ensureScheduler().getTask(
        tappInstance.id,
        taskId as string,
        await bridge.getRuntimeGrant(),
      )
      return { success: true, data: task }
    } catch (error) {
      return errResult(error)
    }
  })

  for (const [action, operation] of [
    [
      'enable',
      (taskId: string, runtimeGrant: string) =>
        ensureScheduler().enableTask(tappInstance.id, taskId, runtimeGrant),
    ],
    [
      'disable',
      (taskId: string, runtimeGrant: string) =>
        ensureScheduler().disableTask(tappInstance.id, taskId, runtimeGrant),
    ],
    [
      'trigger',
      (taskId: string, runtimeGrant: string) =>
        ensureScheduler().triggerTask(tappInstance.id, taskId, runtimeGrant),
    ],
  ] as const) {
    bridge.registerHandler(`scheduler.${action}`, async (message) => {
      const [taskId] = (message.payload as { args: unknown[] }).args || []
      if (!taskId) return { success: false, error: 'taskId required' }
      try {
        await operation(taskId as string, await bridge.getRuntimeGrant())
        return { success: true, data: { taskId, [action]: true } }
      } catch (error) {
        return errResult(error)
      }
    })
  }

  // onTask 是沙箱内的同步事件 API；subscribe/unsubscribe 只负责把宿主 WS
  // 回调绑定到当前 bridge，任务本身无需重新注册。
  bridge.registerHandler('scheduler.subscribe', async (message) => {
    const [taskId] = (message.payload as { args: unknown[] }).args || []
    if (!taskId) return { success: false, error: 'taskId required' }
    bindTask(taskId as string)
    return { success: true, data: { taskId, subscribed: true } }
  })

  bridge.registerHandler('scheduler.unsubscribe', async (message) => {
    const [taskId] = (message.payload as { args: unknown[] }).args || []
    if (!taskId) return { success: false, error: 'taskId required' }
    unbindTask(taskId as string)
    return { success: true, data: { taskId, subscribed: false } }
  })

  bridge.registerHandler('scheduler.complete', async (message) => {
    const [executionId, success, error] =
      (message.payload as { args: unknown[] }).args || []
    if (typeof executionId !== 'number' || typeof success !== 'boolean') {
      return { success: false, error: 'executionId and success required' }
    }
    const pending = pendingExecutions.get(executionId)
    if (!pending) {
      return { success: false, error: 'Execution is no longer pending' }
    }
    clearTimeout(pending.timeout)
    pendingExecutions.delete(executionId)
    if (success) {
      pending.resolve()
    } else {
      pending.reject(
        new Error(typeof error === 'string' ? error : 'Task failed'),
      )
    }
    return { success: true, data: { executionId, completed: true } }
  })

  return () => {
    for (const unsubscribe of taskSubscriptions.values()) unsubscribe()
    taskSubscriptions.clear()
    for (const pending of pendingExecutions.values()) {
      clearTimeout(pending.timeout)
      pending.reject(new Error('Scheduler sandbox destroyed'))
    }
    pendingExecutions.clear()
  }
}
