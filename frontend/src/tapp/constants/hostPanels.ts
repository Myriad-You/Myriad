export const HOST_PANEL_STORE_ID = 'myriad:host.store'

export function isHostPanelId(id: string): boolean {
  return id.startsWith('myriad:host.')
}

export function isStoreHostPanel(id: string): boolean {
  return id === HOST_PANEL_STORE_ID
}
