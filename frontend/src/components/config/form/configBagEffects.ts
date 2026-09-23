import type { ConfigEffect } from './configDomain'
import type { Config } from './types'
import { API_URL } from '../../../config'
import { notifyPersonaUpdated } from '../../../features/merope/events'
import { reloadSystemConfig } from '../../../services/configApi'
import { invalidateSpeechStatusCache } from '../../../services/speechApi'
import { deepEqual } from '../../../utils/deepEqual'
import { ISLAND_CONTENT_CHANGED_EVENT } from '../../../utils/islandContent'
import {
  clearDedupCache,
  clearLibraryDataCache,
  invalidatePublicConfigCache,
} from '../../../utils/requestDedup'
import {
  configChangesNeedFooterReload,
  configChangesNeedIslandReload,
  configChangesNeedMetadataReload,
  configChangesNeedPersonaPublicNameRefresh,
  configChangesNeedPlatformsCacheInvalidation,
  configChangesNeedPwaReload,
  configChangesNeedRuntimeReload,
  configChangesNeedSpeechPipelineReload,
  configChangesNeedWallpaperReload,
} from '../uiBagOwnership'

export function configBagEffects(
  next: Config | null,
  previous: Config | null,
): ConfigEffect[] {
  if (!next || !previous) return []
  const effects: ConfigEffect[] = []
  if (configChangesNeedRuntimeReload(next, previous)) {
    effects.push({
      id: 'runtime',
      run: async () => {
        const result = await reloadSystemConfig()
        if (result?.success === false)
          throw new Error(result.message || 'Runtime reload failed')
      },
    })
  }
  if (configChangesNeedWallpaperReload(next, previous)) {
    effects.push({
      id: 'wallpaper',
      run: async () => {
        clearDedupCache(`${API_URL}/api/config/ui`)
        const { invalidateWallpaperLoadCache } =
          await import('../../../hooks/useWallpaper')
        invalidateWallpaperLoadCache()
        window.dispatchEvent(new CustomEvent('wallpaperConfigChanged'))
      },
    })
  }
  if (configChangesNeedMetadataReload(next, previous)) {
    effects.push({
      id: 'metadata',
      run: async () => {
        clearDedupCache(`${API_URL}/api/config/metadata`)
        const { refreshSiteMetadata } =
          await import('../../../utils/siteMetadata')
        await refreshSiteMetadata()
      },
    })
  }
  if (configChangesNeedFooterReload(next, previous)) {
    effects.push({
      id: 'footer',
      run: () => {
        clearDedupCache(`${API_URL}/api/config/ui`)
        window.dispatchEvent(new CustomEvent('footerConfigChanged'))
      },
    })
  }
  if (configChangesNeedIslandReload(next, previous)) {
    effects.push({
      id: 'island',
      run: () => {
        clearDedupCache(`${API_URL}/api/config/ui`)
        window.dispatchEvent(new CustomEvent(ISLAND_CONTENT_CHANGED_EVENT))
      },
    })
  }
  if (configChangesNeedPwaReload(next, previous)) {
    effects.push({
      id: 'pwa',
      run: async () => {
        clearDedupCache(`${API_URL}/api/config/ui`)
        const raw = next.ui_config.config_fields.find(
          (field) => field.key === 'pwa_enabled',
        )?.value
        const { applyPwaEnabled } = await import('../../../utils/pwa')
        await applyPwaEnabled(raw !== 'false' && raw !== '0')
      },
    })
  }
  if (configChangesNeedPlatformsCacheInvalidation(next, previous, deepEqual))
    effects.push({ id: 'platforms', run: clearLibraryDataCache })
  if (configChangesNeedSpeechPipelineReload(next, previous)) {
    effects.push({
      id: 'speech',
      after: ['runtime'],
      run: async () => {
        invalidateSpeechStatusCache()
        const { getSpeechPipeline } =
          await import('../../../features/merope/speech/speechPipelineHost')
        getSpeechPipeline().cancel()
        await getSpeechPipeline().refreshStatus()
      },
    })
  }
  if (configChangesNeedPersonaPublicNameRefresh(next, previous)) {
    effects.push({
      id: 'persona-name',
      after: ['runtime'],
      run: () => {
        invalidatePublicConfigCache()
        notifyPersonaUpdated()
      },
    })
  }
  return effects
}
