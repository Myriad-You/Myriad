/**
 * 这台浏览器里别的页面改了哪类数据：创作台的每个工具窗口是独立的页面，
 * 一个窗口里保存、删除了什么，别的窗口靠这个知道该重新取数。
 * 发出去的页面自己收不到（同一个频道对象不回送给自己），它本来就已经更新了。
 */

export type DataTopic = 'notes' | 'categories' | 'comments' | 'reviews' | 'media'

const CHANNEL = 'myriad-data-changes'

let channel: BroadcastChannel | null | undefined

function shared(): BroadcastChannel | null {
  if (channel !== undefined) return channel
  try {
    channel = typeof BroadcastChannel === 'undefined' ? null : new BroadcastChannel(CHANNEL)
    // Node 里（跑测试时）开着的频道会让进程一直不退出。
    ;(channel as { unref?: () => void } | null)?.unref?.()
  } catch {
    channel = null
  }
  return channel
}

export function announceDataChange(topic: DataTopic): void {
  try {
    shared()?.postMessage({ topic })
  } catch {
    // 发不出去就只是别的窗口晚一点才看到。
  }
}

export function subscribeDataChanges(listener: (topic: DataTopic) => void): () => void {
  const target = shared()
  if (!target) return () => {}
  const onMessage = (event: MessageEvent) => {
    const topic = (event.data as { topic?: unknown } | null)?.topic
    if (typeof topic === 'string') listener(topic as DataTopic)
  }
  target.addEventListener('message', onMessage)
  return () => target.removeEventListener('message', onMessage)
}
