import type * as TappApiService from '../../../services/TappApiService'

export class GuestShortcutSession {
  private readonly shortcuts = new Map<string, TappApiService.ShortcutConfig>()

  register(config: TappApiService.ShortcutConfig): void {
    this.shortcuts.set(config.id, config)
  }

  unregister(id: string): void {
    this.shortcuts.delete(id)
  }

  list(): TappApiService.ShortcutConfig[] {
    return [...this.shortcuts.values()]
  }

  clear(): void {
    this.shortcuts.clear()
  }
}
