/**
 * 处理器模块索引
 */

export { registerAgentInteractionHandlers } from '../../AgentInteractionBroker'
export { registerDataExchangeHandlers } from '../../DataExchangeBroker'
export { registerEventHandlers } from '../../EventBroker'
export { registerFederationHandlers } from '../../FederationBridge'
export { registerGameHandlers } from '../../GameBridge'

export {
  registerAdvancedHandlers,
  registerAnimationHandlers,
  registerBackgroundHandlers,
  registerContextHandlers,
  registerDynamicContentHandlers,
  registerMediaHandlers,
  registerSpeechHandlers,
} from './advancedHandlers'

export { registerAIHandlers, registerReportHandlers } from './aiHandlers'

export {
  registerAssetHandlers,
  registerFileHandlers,
  registerLifecycleHandlers,
  registerStorageHandlers,
  registerUIHandlers,
  registerUserHandlers,
} from './baseHandlers'

export {
  registerBrewListHandlers,
  registerTappListHandlers,
} from './contentHandlers'

export { registerModel3dHandlers } from './model3dHandlers'
export { registerPersonaHandlers } from './personaHandlers'

export {
  registerAnalyticsHandlers,
  registerPlatformHandlers,
  registerWidgetHandlers,
} from './platformHandlers'
export { registerSchedulerHandlers } from './schedulerHandlers'
export { registerWidgetInvalidateTargetHandler } from './widgetInvalidateTargetHandler'
