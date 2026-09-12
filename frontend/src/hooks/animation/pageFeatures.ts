export enum Feature {
  Visibility = 1 << 0,
  Resize = 1 << 1,
  Intersection = 1 << 2,
  Interval = 1 << 3,
  Timeout = 1 << 4,
  RAF = 1 << 5,
  Idle = 1 << 6,
  DOMBatch = 1 << 7,
  MessageChannel = 1 << 8,
}

export const PAGE_FEATURES: Record<string, number> = {
  home:
    Feature.Visibility |
    Feature.Resize |
    Feature.RAF |
    Feature.Idle |
    Feature.Interval,
  library: Feature.Resize | Feature.Intersection | Feature.Idle,
  reports:
    Feature.Visibility | Feature.Interval | Feature.RAF | Feature.DOMBatch,
  brew:
    Feature.Visibility | Feature.Intersection | Feature.Timeout | Feature.Idle,
  config: Feature.Timeout,
  login: Feature.Timeout,
  details: 0,
  setup: Feature.Timeout,
  'tapp-multi': Feature.Visibility | Feature.Idle,
}

export function hasFeature(pageId: string, feature: Feature): boolean {
  const features = PAGE_FEATURES[pageId] ?? 0
  return (features & feature) !== 0
}

export function getFeatureList(pageId: string): string[] {
  const features = PAGE_FEATURES[pageId] ?? 0
  const list: string[] = []

  if (features & Feature.Visibility) list.push('Visibility')
  if (features & Feature.Resize) list.push('Resize')
  if (features & Feature.Intersection) list.push('Intersection')
  if (features & Feature.Interval) list.push('Interval')
  if (features & Feature.Timeout) list.push('Timeout')
  if (features & Feature.RAF) list.push('RAF')
  if (features & Feature.Idle) list.push('Idle')
  if (features & Feature.DOMBatch) list.push('DOMBatch')
  if (features & Feature.MessageChannel) list.push('MessageChannel')

  return list
}
