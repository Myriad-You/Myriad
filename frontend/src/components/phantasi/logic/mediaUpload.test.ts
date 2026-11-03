import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  isOutsideFileDrag,
  splitWorkbenchUploads,
  uploadInOrder,
  WORKBENCH_MEDIA_ACCEPT,
} from './mediaUpload.ts'

describe('splitWorkbenchUploads', () => {
  it('按类型分，类型为空时看扩展名，保持原顺序', () => {
    const files = [
      { name: 'a.png', type: 'image/png' },
      { name: 'b.pdf', type: 'application/pdf' },
      { name: 'c.MOV', type: '' },
      { name: 'd.txt', type: '' },
      { name: 'e.webm', type: 'video/webm' },
      { name: 'f.svg', type: 'image/svg+xml' },
    ]
    const { accepted, rejected } = splitWorkbenchUploads(files)
    assert.deepEqual(accepted.map(file => file.name), ['a.png', 'c.MOV', 'e.webm'])
    assert.deepEqual(rejected.map(file => file.name), ['b.pdf', 'd.txt', 'f.svg'])
  })

  it('选择框 accept 里的每种类型都收', () => {
    const files = WORKBENCH_MEDIA_ACCEPT.split(',').map(type => ({ name: 'x', type }))
    assert.equal(splitWorkbenchUploads(files).rejected.length, 0)
  })
})

describe('isOutsideFileDrag', () => {
  it('只认从页面外拖进来的文件', () => {
    assert.equal(isOutsideFileDrag(['Files'], false), true)
    assert.equal(isOutsideFileDrag(['text/uri-list', 'text/html', 'Files'], true), false)
    assert.equal(isOutsideFileDrag(['text/plain'], false), false)
    assert.equal(isOutsideFileDrag([], false), false)
  })
})

describe('uploadInOrder', () => {
  it('按顺序一个一个传，失败的不拦后面', async () => {
    let inFlight = 0
    let peak = 0
    const order: string[] = []
    const started: number[] = []
    const result = await uploadInOrder(
      ['a', 'b', 'c', 'd'],
      async (item) => {
        inFlight += 1
        peak = Math.max(peak, inFlight)
        await new Promise(resolve => setTimeout(resolve, 1))
        order.push(item)
        inFlight -= 1
        if (item === 'b') throw new Error('nope')
      },
      index => started.push(index),
    )
    assert.equal(peak, 1)
    assert.deepEqual(order, ['a', 'b', 'c', 'd'])
    assert.deepEqual(started, [0, 1, 2, 3])
    assert.equal(result.done, 3)
    assert.deepEqual(result.failed.map(row => row.item), ['b'])
    assert.equal((result.failed[0]?.error as Error).message, 'nope')
  })

  it('空列表什么都不做', async () => {
    const result = await uploadInOrder([], async () => assert.fail('不该调用'))
    assert.deepEqual(result, { done: 0, failed: [] })
  })
})
