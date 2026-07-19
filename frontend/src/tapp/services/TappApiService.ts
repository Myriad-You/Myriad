/** Unified facade for the domain-specific Tapp API modules. */

import {
  cancelAITask,
  createAITask,
  getAITask,
  getAIUsage,
  streamAITaskEvents,
} from './TappAiApi'
import {
  dataTransform,
  executeTappApi,
  getContextApp,
  getContextGeo,
  getContextNavigation,
  getContextPlayer,
  getContextSystem,
  getContextUser,
  listTappApis,
} from './TappContextApi'
import {
  createTappReport,
  deleteTappReport,
  getTappReport,
  listTappReports,
  mediaControl,
  mediaStatus,
  updateTappReport,
} from './TappHostIntegrationApi'
import {
  buildInstallPackageFromInstalled,
  installDirect,
  installFromCode,
  installFromStore,
  installTapp,
  installTappFile,
  OFFICIAL_TAPP_STORE_URL,
  resolveStoreSourceForTapp,
  uninstallTapp,
  updateTappFromCode,
  updateTappFromStore,
} from './TappInstallationApi'
import {
  getRecentTapps,
  getTapp,
  listTapps,
  startTapp,
  stopTapp,
} from './TappLifecycleApi'
import {
  exportTapp,
  getTappAsset,
  getTappCode,
  getTappResources,
} from './TappPackageResourceApi'
import {
  addPlatformItem,
  addPlatformItems,
  getPlatformData,
  getPlatformDistribution,
  getPlatformStats,
  listEnabledPlatforms,
} from './TappPlatformApi'
import {
  getPlatformReport,
  getReport,
  listReports,
} from './TappReportCatalogApi'
import {
  clearStorage,
  getStorage,
  listStorageEntries,
  listStorageKeys,
  removeStorage,
  setStorage,
} from './TappStorageApi'
import {
  getAllWidgets,
  registerTappWidget,
  unregisterTappWidget,
} from './TappWidgetApi'

export * from './TappAiApi'
export * from './TappContextApi'
export * from './TappHostIntegrationApi'
export * from './TappInstallationApi'
export * from './TappInteractionApi'
export * from './TappLifecycleApi'
export * from './TappPackageResourceApi'
export * from './TappPlatformApi'
export * from './TappReportCatalogApi'
export * from './TappRuntimeAccessApi'
export * from './TappStorageApi'
export * from './TappWidgetApi'

export default {
  listTapps,
  getRecentTapps,
  installTapp,
  installTappFile,
  installFromCode,
  installFromStore,
  installDirect,
  buildInstallPackageFromInstalled,
  resolveStoreSourceForTapp,
  OFFICIAL_TAPP_STORE_URL,
  updateTappFromCode,
  updateTappFromStore,
  getTapp,
  getTappCode,
  getTappResources,
  startTapp,
  stopTapp,
  uninstallTapp,
  exportTapp,
  getAllWidgets,
  registerTappWidget,
  unregisterTappWidget,
  getStorage,
  setStorage,
  removeStorage,
  listStorageKeys,
  listStorageEntries,
  clearStorage,
  listEnabledPlatforms,
  getPlatformData,
  getPlatformStats,
  getPlatformDistribution,
  addPlatformItem,
  addPlatformItems,
  createAITask,
  getAITask,
  cancelAITask,
  getAIUsage,
  streamAITaskEvents,
  listReports,
  getReport,
  getPlatformReport,
  dataTransform,
  getContextApp,
  getContextUser,
  getContextPlayer,
  getContextNavigation,
  getContextSystem,
  getContextGeo,
  executeTappApi,
  listTappApis,
  createTappReport,
  listTappReports,
  getTappReport,
  updateTappReport,
  deleteTappReport,
  mediaControl,
  mediaStatus,
  getTappAsset,
}
