import type { TappBridge } from '../TappBridge'

export type SandboxCapabilityProfile = 'page' | 'widget' | 'headless'

/** 可见/控制面动作不得从后台 core 触及。 */
export const HEADLESS_DENIED_ACTIONS = [
  'ui.setTitle',
  'ui.confirm',
  'ui.openUrl',
  'ui.listOpenUrls',
  'ui.fullscreen.request',
  'ui.fullscreen.exit',
  'ui.fullscreen.toggle',
  'ui.fullscreen.isFullscreen',
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
  'model3d.status',
  'model3d.upload',
  'model3d.createTask',
  'model3d.getTask',
  'model3d.awaitTask',
  'model3d.getUrl',
  'model3d.getMetadata',
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
