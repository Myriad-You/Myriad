/** HTTP CRUD 只准备 apiBase；首次前端订阅/前端任务才连 WS。scheduler:register 由服务端强制。 */

import type { TappInstance } from '../../../types'
import type { TappBridge } from '../../TappBridge'
import type { TaskRegistrationOptions } from '../../TappScheduler'
import { API_URL } from '../../../../config'
import { userFacingError } from '../../../../utils/userFacingError'
import { getTappScheduler, TappScheduler } from '../../TappScheduler'

let schedulerInitialized = false
let schedulerUsers = 0

function getOrInitScheduler() {
  const scheduler = getTappScheduler()
  if (!schedulerInitialized) {
    scheduler.initialize(`${API_URL}/api`)
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

  let acquired = false
  const takeScheduler = () => {
    const scheduler = getOrInitScheduler()
    if (!acquired) {
      acquired = true
      schedulerUsers += 1
    }
    return scheduler
  }

  const bindTask = (taskId: string) => {
    if (taskSubscriptions.has(taskId)) return
    const scheduler = takeScheduler()
    const unsubscribe = scheduler.onTask(
      tappInstance.id,
      taskId,
      (payload, event) => {
        const deferred = Promise.withResolvers<void>()
        if (!event.executionId) {
          deferred.reject(new Error('Scheduler execution ID missing'))
          return deferred.promise
        }
        const timeout = setTimeout(
          () => {
            pendingExecutions.delete(event.executionId)
            deferred.reject(new Error('Sandbox scheduler callback timed out'))
          },
          5 * 60 * 1000,
        )
        pendingExecutions.set(event.executionId, {
          resolve: deferred.resolve,
          reject: deferred.reject,
          timeout,
        })
        bridge.emit('schedulerTask', { taskId, payload, event })
        return deferred.promise
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
      const scheduler = takeScheduler()
      const task = await scheduler.registerTask(
        tappInstance.id,
        opts,
        await bridge.getRuntimeGrant(),
      )
      const target = opts.executionTarget ?? 'frontend'
      if (target !== 'backend') bindTask(opts.taskId)
      return { success: true, data: task }
    } catch (error) {
      return errResult(error)
    }
  })

  bridge.registerHandler('scheduler.unregister', async (message) => {
    const [taskId] = (message.payload as { args: unknown[] }).args || []
    if (!taskId) return { success: false, error: 'taskId required' }
    try {
      const scheduler = takeScheduler()
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
      const scheduler = takeScheduler()
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
      const task = await takeScheduler().getTask(
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
        takeScheduler().enableTask(tappInstance.id, taskId, runtimeGrant),
    ],
    [
      'disable',
      (taskId: string, runtimeGrant: string) =>
        takeScheduler().disableTask(tappInstance.id, taskId, runtimeGrant),
    ],
    [
      'trigger',
      (taskId: string, runtimeGrant: string) =>
        takeScheduler().triggerTask(tappInstance.id, taskId, runtimeGrant),
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
    if (acquired) {
      acquired = false
      schedulerUsers -= 1
      if (schedulerUsers <= 0) {
        schedulerUsers = 0
        TappScheduler.reset()
        schedulerInitialized = false
      }
    }
  }
}
