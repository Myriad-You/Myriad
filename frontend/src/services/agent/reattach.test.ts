import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  collectReattachCandidates,
  isNonTerminalTaskStatus,
} from './reattach'

describe('collectReattachCandidates', () => {
  it('merges runId from an older first-wait message onto the newest task host', () => {
    const messages = [
      {
        id: 'msg_first_wait',
        role: 'assistant',
        taskId: 'task_1',
        runId: 'run_keep',
      },
      {
        id: 'msg_multi_round',
        role: 'assistant',
        taskId: 'task_1',
        runId: undefined,
      },
    ]
    const candidates = collectReattachCandidates(messages)
    const task = candidates.find((c) => c.taskId === 'task_1')
    assert.ok(task)
    assert.equal(task!.runId, 'run_keep')
    assert.equal(task!.messageId, 'msg_multi_round')
  })

  it('accepts runId-only hints when task_id is null (early progress notification)', () => {
    const messages = [
      {
        id: 'msg_mid',
        role: 'assistant',
        taskId: 'task_later',
        runId: 'run_early',
      },
    ]
    const candidates = collectReattachCandidates(messages, {
      runId: 'run_early',
      // Early task_progress may omit taskId.
    })
    assert.ok(candidates.some((c) => c.runId === 'run_early'))
    const hit = candidates.find((c) => c.runId === 'run_early')
    assert.equal(hit!.taskId, 'task_later')
  })

  it('accepts runId-only hints even when no messages carry identity yet', () => {
    const candidates = collectReattachCandidates(
      [{ id: 'msg_hist', role: 'assistant' }],
      { runId: 'run_orphan' },
    )
    assert.equal(candidates.length >= 1, true)
    assert.equal(candidates[0].runId, 'run_orphan')
    assert.equal(candidates[0].messageId, 'msg_hist')
  })

  it('seeds a candidate from taskId hints and fills runId from history', () => {
    const candidates = collectReattachCandidates(
      [
        {
          id: 'm1',
          role: 'assistant',
          taskId: 'task_x',
          runId: 'run_x',
        },
      ],
      { taskId: 'task_x' },
    )
    assert.equal(candidates[0].taskId, 'task_x')
    assert.equal(candidates[0].runId, 'run_x')
  })
})

describe('isNonTerminalTaskStatus', () => {
  it('recognizes live statuses only', () => {
    assert.equal(isNonTerminalTaskStatus('running'), true)
    assert.equal(isNonTerminalTaskStatus('waiting_for_input'), true)
    assert.equal(isNonTerminalTaskStatus('completed'), false)
    assert.equal(isNonTerminalTaskStatus('cancelled'), false)
  })
})
