import type { TripoTask } from './tripo3dApi'
import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { pollTripoTask } from './tripo3dApi'

function task(status: string, progress: number): TripoTask {
  return {
    task_id: 'task_test',
    task_type: 'image_to_model',
    status,
    progress,
    output: {},
  }
}

describe('pollTripoTask', () => {
  it('returns terminal task state and publishes progress', async () => {
    const progress: number[] = []
    const controller = new AbortController()
    const result = await pollTripoTask(
      'task_test',
      async (_taskId, signal) => {
        assert.equal(signal, controller.signal)
        return task('success', 100)
      },
      {
        signal: controller.signal,
        onProgress: (value) => progress.push(value.progress),
      },
    )

    assert.equal(result.status, 'success')
    assert.deepEqual(progress, [100])
  })

  it('honors an already-aborted caller before querying', async () => {
    const controller = new AbortController()
    controller.abort()
    let queries = 0

    await assert.rejects(
      pollTripoTask(
        'task_test',
        async () => {
          queries += 1
          return task('running', 20)
        },
        { signal: controller.signal },
      ),
      (error: Error) => error.name === 'AbortError',
    )
    assert.equal(queries, 0)
  })
})
