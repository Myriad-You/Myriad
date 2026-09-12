export type ReadingQueueOrigin = 'agent' | 'starred' | 'topic' | 'feeds' | 'direct'

export interface ReadingQueueItem {
  id: number
  title: string
}

export interface ReadingQueue {
  origin: ReadingQueueOrigin
  name?: string
  items: ReadingQueueItem[]
}

export function readingQueue(
  origin: ReadingQueueOrigin,
  items: Array<{ id: number; title: string }>,
  name?: string,
): ReadingQueue {
  return {
    origin,
    name,
    items: items.map((item) => ({ id: item.id, title: item.title })),
  }
}

export function readingQueueFromStories(
  origin: ReadingQueueOrigin,
  stories: Array<{ id: number; title: string }> | null | undefined,
  fallback: Array<{ id: number; title: string }>,
  name?: string,
): ReadingQueue {
  return readingQueue(
    origin,
    stories && stories.length > 0 ? stories : fallback,
    name,
  )
}

export function neighborsInQueue(
  queue: ReadingQueue | null | undefined,
  itemId: number,
): {
  prev: ReadingQueueItem | null
  next: ReadingQueueItem | null
  index: number
  total: number
  name?: string
} | null {
  if (!queue || queue.items.length === 0) return null
  const index = queue.items.findIndex((item) => item.id === itemId)
  if (index < 0) return null
  return {
    prev: queue.items[index - 1] ?? null,
    next: queue.items[index + 1] ?? null,
    index,
    total: queue.items.length,
    name: queue.name,
  }
}
