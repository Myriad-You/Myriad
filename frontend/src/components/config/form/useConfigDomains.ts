import type { LocaleConfig } from '../../../i18n/assembleLocale'
import { useFederationDomain } from './domains/useFederationDomain'
import { useHitokotoDomain } from './domains/useHitokotoDomain'
import { useLibraryDomain } from './domains/useLibraryDomain'
import { useModuleVisibilityDomain } from './domains/useModuleVisibilityDomain'
import { useNotificationDomain } from './domains/useNotificationDomain'
import { useOAuthDomain } from './domains/useOAuthDomain'
import { usePermissionDomain } from './domains/usePermissionDomain'
import { useReportDomain } from './domains/useReportDomain'

export function useConfigDomains(
  isAdmin: boolean,
  messages: LocaleConfig,
  userId?: number,
  federationEnabled = true,
) {
  const library = useLibraryDomain(messages)
  const visibility = useModuleVisibilityDomain()
  const hitokoto = useHitokotoDomain()
  const reports = useReportDomain()
  const permissions = usePermissionDomain(messages)
  const notifications = useNotificationDomain(userId)
  const oauth = useOAuthDomain()
  const federation = useFederationDomain(isAdmin, federationEnabled)
  return {
    library,
    visibility,
    hitokoto,
    reports,
    permissions,
    notifications,
    oauth,
    federation,
  }
}
