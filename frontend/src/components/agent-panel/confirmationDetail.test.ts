import assert from 'node:assert/strict'
import { it } from 'node:test'
import { isHousekeepingStep, readConfirmationDetail } from './confirmationDetail'

it('reads a note save as its changes and ignores anything malformed', () => {
  const detail = readConfirmationDetail({
    capabilityId: 'blog.note.update',
    blogNote: {
      title: '测试2',
      total: 3,
      changes: [
        { field: 'body', before: 'Old', after: 'New' },
        { field: 'topic', before: null, after: 'Essays' },
        { field: 'secret', before: 'x', after: 'y' },
        'junk',
      ],
    },
  })
  assert.deepEqual(detail, {
    capabilityId: 'blog.note.update',
    blogNote: {
      title: '测试2',
      total: 3,
      changes: [
        { field: 'body', before: 'Old', after: 'New' },
        { field: 'topic', before: '', after: 'Essays' },
      ],
    },
  })
  assert.equal(readConfirmationDetail(null), undefined)
  assert.equal(readConfirmationDetail({ unrelated: true }), undefined)
  // A total smaller than what was sent cannot hide changes.
  assert.equal(readConfirmationDetail({ blogNote: { changes: [{ field: 'body', before: '', after: 'a' }], total: 0 } })?.blogNote?.total, 1)
})

it('the Agent recovering from its own bookkeeping is not a failure to report', () => {
  for (const id of ['read_result_3_ab12', 'discover_tools_0_ff', 'update_plan_1_x']) {
    assert.equal(isHousekeepingStep(id), true, id)
  }
  assert.equal(isHousekeepingStep('cap_blog_note_update_6e0f_2_8c0e'), false)
  assert.equal(isHousekeepingStep('read_resultless'), false)
})

it('the runtime limits reach users as their own copy, not English', async () => {
  const { exactErrorCode } = await import('../../utils/errorCodes')
  const zh = (await import('../../i18n/errors.zh-CN.json', { with: { type: 'json' } })).default as { byCode: Record<string, string> }
  for (const [text, code] of [
    ['The task reached its token budget', 'agent_work_budget'],
    ['The task reached its model-turn limit', 'agent_work_turns'],
    ['The task expired while waiting for input', 'agent_work_expired'],
  ] as const) {
    assert.equal(exactErrorCode(text), code)
    assert.match(zh.byCode[code] ?? '', /\p{Script=Han}/u)
  }
})
