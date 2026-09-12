// 非 React.lazy。渲染期禁止 Suspense 错峰挂载；挂载前只 await 布局用到的平台 chunk。
import type { ComponentType } from 'react'

export type PlatformFaceComponent = ComponentType<{
  data: any
  showOverview: boolean
  onContentChange: (content: any) => void
  allowLoop?: boolean
  // 预览须关掉真实网络轮询。
  isPreview?: boolean
}>

const registry = new Map<string, PlatformFaceComponent>()
const moduleInflight = new Map<string, Promise<void>>()

type FaceModuleKey =
  | 'anime'
  | 'bilibili'
  | 'gaming'
  | 'github'
  | 'youtube'
  | 'netease'
  | 'social'

const PLATFORM_MODULE: Record<string, FaceModuleKey> = {
  bilibili: 'bilibili',
  steam: 'gaming',
  xbox: 'gaming',
  psn: 'gaming',
  github: 'github',
  youtube: 'youtube',
  netease: 'netease',
  bangumi: 'anime',
  mal: 'anime',
  x: 'social',
  discord: 'social',
}

async function loadFaceModule(key: FaceModuleKey): Promise<void> {
  const existing = moduleInflight.get(key)
  if (existing) return existing

  const job = (async () => {
    switch (key) {
      case 'bilibili': {
        const m = await import('./platforms/bilibili')
        registry.set('bilibili', m.BilibiliWidget as PlatformFaceComponent)
        break
      }
      case 'gaming': {
        const m = await import('./platforms/gaming')
        registry.set('steam', m.SteamWidget as PlatformFaceComponent)
        registry.set('xbox', m.XboxWidget as PlatformFaceComponent)
        registry.set('psn', m.PsnWidget as PlatformFaceComponent)
        break
      }
      case 'github': {
        const m = await import('./platforms/github')
        registry.set('github', m.GithubWidget as PlatformFaceComponent)
        break
      }
      case 'youtube': {
        const m = await import('./platforms/youtube')
        registry.set('youtube', m.YoutubeWidget as PlatformFaceComponent)
        break
      }
      case 'netease': {
        const m = await import('./platforms/netease')
        registry.set('netease', m.NeteaseWidget as PlatformFaceComponent)
        break
      }
      case 'anime': {
        const m = await import('./platforms/anime')
        registry.set('bangumi', m.BangumiWidget as PlatformFaceComponent)
        registry.set('mal', m.MalWidget as PlatformFaceComponent)
        break
      }
      case 'social': {
        const m = await import('./platforms/social')
        registry.set('x', m.XWidget as PlatformFaceComponent)
        registry.set('discord', m.DiscordWidget as PlatformFaceComponent)
        break
      }
    }
  })()

  moduleInflight.set(key, job)
  try {
    await job
  } catch (err) {
    moduleInflight.delete(key)
    throw err
  }
}

export function isPlatformFaceReady(platformId: string): boolean {
  return registry.has(platformId)
}

export function getPlatformFace(
  platformId: string,
): PlatformFaceComponent | undefined {
  return registry.get(platformId)
}

export function preloadPlatformFaces(
  platformIds: Iterable<string>,
): Promise<void> {
  const modules = new Set<FaceModuleKey>()
  for (const id of platformIds) {
    const mod = PLATFORM_MODULE[id]
    if (mod && !registry.has(id)) modules.add(mod)
  }
  if (modules.size === 0) return Promise.resolve()
  return Promise.all(
    Iterator.from(modules).map((k) => loadFaceModule(k)),
  ).then(
    () => undefined,
  )
}

export function preloadPlatformFacesForWidgetTypes(
  types: Iterable<string>,
): Promise<void> {
  const ids: string[] = []
  for (const type of types) {
    if (type.startsWith('report-')) {
      ids.push(type.slice('report-'.length))
    }
  }
  return preloadPlatformFaces(ids)
}
