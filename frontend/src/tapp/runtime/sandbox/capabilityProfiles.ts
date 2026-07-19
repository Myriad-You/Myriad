import type { TappBridge } from '../TappBridge'

export type SandboxCapabilityProfile = 'page' | 'widget' | 'headless'

/**
 * Actions that require a visible/control-plane surface and therefore must not
 * be reachable from a background core runtime.
 */
export const HEADLESS_DENIED_ACTIONS = [
  'ui.setTitle',
  'ui.confirm',
  'ui.requestFullscreen',
  'ui.exitFullscreen',
  'ui.toggleFullscreen',
  'ui.isFullscreen',
  'widget.register',
  'widget.unregister',
  'widget.listRegistered',
  'widget.updateConfig',
  'tappList.list',
  'tappList.get',
  'tappList.getRecent',
  'tappList.getInstallPackage',
  'tappList.resolveStoreSource',
  'tappList.install',
  'tappList.uninstall',
  'tappList.start',
  'tappList.stop',
  'tappList.export',
  'component.registerTheme',
  'component.registerAgent',
  'component.unregister',
  'component.list',
  'shortcut.register',
  'shortcut.unregister',
  'shortcut.list',
  'dynamicContent.set',
  'dynamicContent.update',
  'dynamicContent.get',
  'dynamicContent.remove',
  'file.download',
] as const

export function applySandboxCapabilityProfile(
  bridge: TappBridge,
  profile: SandboxCapabilityProfile,
): void {
  if (profile !== 'headless') return
  for (const action of HEADLESS_DENIED_ACTIONS) {
    bridge.unregisterHandler(action)
  }
}
