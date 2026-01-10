/**
 * 平台与 Widget 处理器
 */

import type { CustomPlatformConfig, NewPlatformItem, TappInstance, WidgetRegistration } from '../../../types'
import type { TappBridge } from '../../TappBridge'
import * as TappApiService from '../../../services/TappApiService'
import { getTappRuntime } from '../../TappRuntime'

/**
 * 注册 Widget 处理器
 */
export function registerWidgetHandlers(
  bridge: TappBridge,
  tappInstance: TappInstance,
): void {
  bridge.registerHandler('widget.register', async (message) => {
    const [config] = (message.payload as { args: unknown[] }).args || []
    if (!config)
      return { success: false, error: 'Widget config is required' }
    try {
      const runtime = getTappRuntime()
      const widget = await runtime.registerWidget(tappInstance.id, config as WidgetRegistration)
      runtime.registerBackgroundRequirement(tappInstance.id, 'widget')
      return { success: true, data: widget }
    }
    catch (error) {
      return { success: false, error: error instanceof Error ? error.message : 'Failed' }
    }
  })

  bridge.registerHandler('widget.unregister', async (message) => {
    const [widgetId] = (message.payload as { args: unknown[] }).args || []
    if (!widgetId)
      return { success: false, error: 'Widget ID is required' }
    try {
      const runtime = getTappRuntime()
      await runtime.unregisterWidget(tappInstance.id, widgetId as string)
      const remaining = runtime.getWidgetsByTapp(tappInstance.id)
      if (remaining.length === 0) {
        runtime.unregisterBackgroundRequirement(tappInstance.id, 'widget')
      }
      return { success: true, data: null }
    }
    catch (error) {
      return { success: false, error: error instanceof Error ? error.message : 'Failed' }
    }
  })

  bridge.registerHandler('widget.listRegistered', async () => {
    try {
      const runtime = getTappRuntime()
      const widgets = runtime.getWidgetsByTapp(tappInstance.id)
      return { success: true, data: widgets }
    }
    catch (error) {
      return { success: false, error: error instanceof Error ? error.message : 'Failed' }
    }
  })

  bridge.registerHandler('widget.updateConfig', async (message) => {
    const [widgetId, config] = (message.payload as { args: unknown[] }).args || []
    if (!widgetId || !config)
      return { success: false, error: 'Widget ID and config required' }
    try {
      const runtime = getTappRuntime()
      await runtime.unregisterWidget(tappInstance.id, widgetId as string)
      const widget = await runtime.registerWidget(tappInstance.id, { ...(config as WidgetRegistration), id: widgetId as string })
      return { success: true, data: widget }
    }
    catch (error) {
      return { success: false, error: error instanceof Error ? error.message : 'Failed' }
    }
  })
}

/**
 * 注册 Platform 处理器
 */
export function registerPlatformHandlers(
  bridge: TappBridge,
  tappInstance: TappInstance,
): void {
  bridge.registerHandler('platform.listEnabled', async () => {
    try {
      const platforms = await TappApiService.listEnabledPlatforms()
      return { success: true, data: platforms }
    }
    catch (error) {
      return { success: false, error: error instanceof Error ? error.message : 'Failed' }
    }
  })

  bridge.registerHandler('platform.getData', async (message) => {
    const [platform, options] = (message.payload as { args: unknown[] }).args || []
    if (!platform)
      return { success: false, error: 'Platform required' }
    try {
      const data = await TappApiService.getPlatformData(platform as string, options as Record<string, unknown>)
      return { success: true, data }
    }
    catch (error) {
      return { success: false, error: error instanceof Error ? error.message : 'Failed' }
    }
  })

  bridge.registerHandler('platform.getStats', async (message) => {
    const [platform] = (message.payload as { args: unknown[] }).args || []
    if (!platform)
      return { success: false, error: 'Platform required' }
    try {
      const stats = await TappApiService.getPlatformStats(platform as string)
      return { success: true, data: stats }
    }
    catch (error) {
      return { success: false, error: error instanceof Error ? error.message : 'Failed' }
    }
  })

  bridge.registerHandler('platform.getDistribution', async (message) => {
    const [platform, dimension] = (message.payload as { args: unknown[] }).args || []
    if (!platform || !dimension)
      return { success: false, error: 'Platform and dimension required' }
    try {
      const dist = await TappApiService.getPlatformDistribution(platform as string, dimension as string)
      return { success: true, data: dist }
    }
    catch (error) {
      return { success: false, error: error instanceof Error ? error.message : 'Failed' }
    }
  })

  bridge.registerHandler('platform.addItem', async (message) => {
    const [item] = (message.payload as { args: unknown[] }).args || []
    if (!item)
      return { success: false, error: 'Item required' }
    try {
      const result = await TappApiService.addPlatformItem(tappInstance.id, item as NewPlatformItem)
      return { success: true, data: result }
    }
    catch (error) {
      return { success: false, error: error instanceof Error ? error.message : 'Failed' }
    }
  })

  bridge.registerHandler('platform.addItems', async (message) => {
    const [items] = (message.payload as { args: unknown[] }).args || []
    if (!items || !Array.isArray(items))
      return { success: false, error: 'Items array required' }
    try {
      const result = await TappApiService.addPlatformItems(tappInstance.id, items)
      return { success: true, data: result }
    }
    catch (error) {
      return { success: false, error: error instanceof Error ? error.message : 'Failed' }
    }
  })

  bridge.registerHandler('platform.registerPlatform', async (message) => {
    const [config] = (message.payload as { args: unknown[] }).args || []
    if (!config)
      return { success: false, error: 'Config required' }
    try {
      const runtime = getTappRuntime()
      runtime.registerPlatform(tappInstance.id, config as CustomPlatformConfig)
      return { success: true, data: { id: `tapp.${tappInstance.id}.${(config as { id: string }).id}` } }
    }
    catch (error) {
      return { success: false, error: error instanceof Error ? error.message : 'Failed' }
    }
  })
}
