export { getSettingGuidesCatalog } from './catalog'
export {
  extractMatchSnippet,
  parseSearchQuery,
  rankConfigSearch,
  scoreSearchItem,
} from './configSearch'
export type {
  ConfigSearchableItem,
  RankedSearchItem,
} from './configSearch'
export {
  findGuideElement,
  GUIDE_PATH_ATTR,
  guideAnchorId,
  guideDomProps,
  scheduleScrollToSettingGuide,
  scrollToSettingGuide,
} from './guideAnchor'
export {
  buildGuideSearchIndex,
  GUIDE_CATALOG_TO_SECTION,
  guideEntryTitle,
  guideKeywordsForSection,
  tokenizeForSearch,
} from './guideSearchIndex'
export type { GuideSearchEntry } from './guideSearchIndex'
export { SettingGuideBody } from './SettingGuideBody'
export {
  getTappPermissionGuide,
  getTappPermissionGuides,
  tappPermissionGuidePath,
} from './tappPermissionGuides'
export type { TappPermissionGuides } from './tappPermissionGuides'
export type {
  GuideSectionLabels,
  SettingGuideEntry,
  SettingGuidesCatalog,
} from './types'
export { useSettingGuide } from './useSettingGuide'
export type { GuideBinding } from './useSettingGuide'
