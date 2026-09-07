/**
 * Tapp SDK 代码生成器
 *
 * 生成注入到沙箱的 SDK 代码。实现拆在 sdkFull / sdkWidget / sdkShared。
 */

export { generateFullSDK } from './sdkFull'
export { generateWidgetSDK, resolveWidgetSdkCaps } from './sdkWidget'
export type { WidgetSdkCaps } from './sdkWidget'
