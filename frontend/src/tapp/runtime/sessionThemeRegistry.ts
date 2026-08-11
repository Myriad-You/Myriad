import type { ComponentConfig, RegisteredComponent } from '../services/TappApiService'

type Owner = object

type Listener = () => void

const themesByOwner = new Map<Owner, Map<string, RegisteredComponent>>()
const listeners = new Set<Listener>()

function emitChange(): void {
  listeners.forEach((listener) => listener())
}

export function registerSessionTheme(
  owner: Owner,
  tappId: string,
  config: ComponentConfig,
): RegisteredComponent {
  const component: RegisteredComponent = {
    id: config.id,
    type: 'theme',
    tappId,
    config,
    registeredAt: new Date().toISOString(),
    enabled: true,
  }
  const themes = themesByOwner.get(owner) ?? new Map()
  themes.set(config.id, component)
  themesByOwner.set(owner, themes)
  emitChange()
  return component
}

export function unregisterSessionTheme(owner: Owner, id: string): boolean {
  const themes = themesByOwner.get(owner)
  const removed = themes?.delete(id) ?? false
  if (themes?.size === 0) themesByOwner.delete(owner)
  if (removed) emitChange()
  return removed
}

export function listSessionThemes(owner?: Owner): RegisteredComponent[] {
  if (owner) return [...(themesByOwner.get(owner)?.values() ?? [])]
  return [...themesByOwner.values()].flatMap((themes) => [...themes.values()])
}

export function clearSessionThemes(owner: Owner): void {
  if (themesByOwner.delete(owner)) emitChange()
}

export function subscribeSessionThemes(listener: Listener): () => void {
  listeners.add(listener)
  return () => listeners.delete(listener)
}
