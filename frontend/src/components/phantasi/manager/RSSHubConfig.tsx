import type { RSSHubConfig, RSSHubQueryParams } from '../../../types/phantasi'
import type { RsshubInstance } from './RSSHubInstances'
import type { RouteConfigRequirement, RouteTemplate } from './RSSHubRoutes'
import {
  LuAlertCircle as AlertCircle,
  LuCheck as Check,
  LuInfo as Info,
} from '@lib/icons'
import { useCallback, useEffect, useMemo, useState } from 'react'
import { useI18n } from '../../../contexts/I18nContext'
import { InputItem } from '../../settings/items/InputItem'
import { Spinner } from '../../Spinner'
import { RSSHubAdvanced } from './RSSHubAdvanced'
import { RSSHubInstances } from './RSSHubInstances'
import { RSSHubRouteExplorer } from './RSSHubRoutes'
import '../ui/phantasi.css'

interface RSSHubConfigProps {
  initialConfig?: RSSHubConfig
  onConfigChange: (config: RSSHubConfig, fullUrl: string) => void
  disabled?: boolean
}

export default function RSSHubConfigComponent({
  initialConfig,
  onConfigChange,
  disabled = false,
}: RSSHubConfigProps) {
  const { t, format } = useI18n()
  const [currentInstance, setCurrentInstance] =
    useState<RsshubInstance | null>(null)

  const [routePath, setRoutePath] = useState(initialConfig?.routePath || '')
  const [routeParams, setRouteParams] = useState<Record<string, string>>(
    initialConfig?.routeParams ?? {},
  )

  const [queryParams, setQueryParams] = useState<RSSHubQueryParams>(
    initialConfig?.queryParams ?? {},
  )
  const [currentRouteConfig, setCurrentRouteConfig] = useState<{
    requiresConfig?: RouteConfigRequirement
    configNote?: string
  } | null>(null)

  const [testing, setTesting] = useState(false)
  const [testResult, setTestResult] = useState<{
    success: boolean
    message: string
  } | null>(null)

  const fullUrl = useMemo(() => {
    if (!routePath || !currentInstance) return ''
    const baseUrl = currentInstance.url
    let path = routePath
    Object.entries(routeParams).forEach(([key, value]) => {
      if (value) {
        path = path.replaceAll(`:${key}?`, value).replaceAll(`:${key}`, value)
      }
    })
    path = path.replaceAll(/\/:[^/]+\?/g, '')

    const queryParts: string[] = []

    Object.entries(queryParams).forEach(([key, value]) => {
      if (value !== undefined && value !== '' && value !== null) {
        queryParts.push(
          `${encodeURIComponent(key)}=${encodeURIComponent(String(value))}`,
        )
      }
    })

    const queryString = queryParts.length > 0 ? `?${queryParts.join('&')}` : ''
    return `${baseUrl}${path}${queryString}`
  }, [currentInstance, routePath, routeParams, queryParams])

  const extractedParams = useMemo(() => {
    const matches = routePath.match(/:([^/]+)/g) ?? []
    return matches.map((m) => ({
      name: m.slice(1).replaceAll('?', ''),
      required: !m.endsWith('?'),
    }))
  }, [routePath])

  useEffect(() => {
    if (routePath && currentInstance) {
      const config: RSSHubConfig = {
        instanceUrl: currentInstance.url,
        routePath,
        routeParams,
        queryParams:
          Object.keys(queryParams).length > 0 ? queryParams : undefined,
      }
      onConfigChange(config, fullUrl)
    }
  }, [
    currentInstance,
    routePath,
    routeParams,
    queryParams,
    fullUrl,
    onConfigChange,
  ])

  const handleTestConnection = async () => {
    if (!fullUrl) return
    setTesting(true)
    setTestResult(null)
    try {
      await fetch(fullUrl, {
        method: 'HEAD',
        mode: 'no-cors',
      })
      setTestResult({ success: true, message: t.phantasi.rsshubConnectionOk })
    } catch {
      setTestResult({ success: false, message: t.phantasi.rsshubConnectionFailed })
    } finally {
      setTesting(false)
    }
  }

  const handleSelectRoute = useCallback((route: RouteTemplate) => {
    setRoutePath(route.path)
    setRouteParams({})
    setCurrentRouteConfig(
      route.requiresConfig
        ? {
            requiresConfig: route.requiresConfig,
            configNote: route.configNote,
          }
        : null,
    )
  }, [])

  return (
    <div className="phantasi-skin phantasi-rsshub">
      <RSSHubInstances
        disabled={disabled}
        initialUrl={initialConfig?.instanceUrl}
        onChange={setCurrentInstance}
      />

      <InputItem
        itemKey="rsshub-route-path"
        size="sm"
        required
        label={t.phantasi.rsshubRoutePath}
        value={routePath}
        onChange={setRoutePath}
        placeholder="/bilibili/user/video/:uid"
        disabled={disabled}
        autoComplete="off"
      />

      <RSSHubRouteExplorer
        routePath={routePath}
        disabled={disabled}
        onSelect={handleSelectRoute}
      />

      {extractedParams.length > 0 && (
        <div className="phantasi-rsshub-params">
          {extractedParams.map((param) => (
            <InputItem
              key={param.name}
              itemKey={`rsshub-route-param-${param.name}`}
              size="sm"
              required={param.required}
              label={`:${param.name}`}
              description={param.required ? undefined : t.phantasi.rsshubOptional}
              value={routeParams[param.name] || ''}
              onChange={(value) =>
                setRouteParams((prev) => ({
                  ...prev,
                  [param.name]: value,
                }))
              }
              placeholder={format(t.phantasi.rsshubEnterParam, {
                param: param.name,
              })}
              disabled={disabled}
              autoComplete="off"
            />
          ))}
        </div>
      )}

      {currentRouteConfig && currentRouteConfig.requiresConfig && (
        <div
          className={`p-3 rounded-xl flex items-start gap-2 ${
            currentRouteConfig.requiresConfig === 'server'
              ? 'bg-amber-50 dark:bg-amber-900/20 border border-amber-200 dark:border-amber-700/50'
              : 'bg-blue-50 dark:bg-blue-900/20 border border-blue-200 dark:border-blue-700/50'
          }`}
        >
          <AlertCircle
            className={`w-4 h-4 shrink-0 mt-0.5 ${
              currentRouteConfig.requiresConfig === 'server'
                ? 'text-amber-500'
                : 'text-blue-500'
            }`}
          />
          <div className="flex-1 min-w-0">
            <div
              className={`text-xs font-medium mb-0.5 ${
                currentRouteConfig.requiresConfig === 'server'
                  ? 'text-amber-700 dark:text-amber-300'
                  : 'text-blue-700 dark:text-blue-300'
              }`}
            >
              {currentRouteConfig.requiresConfig === 'server' ? (
                <>
                  <AlertCircle size={12} /> {t.phantasi.rsshubRouteNeedsServer}
                </>
              ) : (
                <>
                  <Info size={12} /> {t.phantasi.rsshubRouteSupportsOptional}
                </>
              )}
            </div>
            <div
              className={`text-xs ${
                currentRouteConfig.requiresConfig === 'server'
                  ? 'text-amber-600 dark:text-amber-400'
                  : 'text-blue-600 dark:text-blue-400'
              }`}
            >
              {currentRouteConfig.requiresConfig === 'server' ? (
                <>
                  {t.phantasi.rsshubNeedEnvConfig}
                  <code className="px-1 py-0.5 bg-amber-100 dark:bg-amber-800/50 rounded text-[11px] ml-1">
                    {currentRouteConfig.configNote}
                  </code>
                  <div className="mt-1.5 text-amber-500 dark:text-amber-400">
                    {t.phantasi.rsshubUsePrivateInstance}
                    <a
                      href="https://docs.rsshub.app/deploy/config#route-specific-configurations"
                      target="_blank"
                      rel="noopener noreferrer"
                      className="underline hover:no-underline ml-1"
                    >
                      {t.phantasi.rsshubDeployDocs}
                    </a>
                  </div>
                </>
              ) : (
                <>
                  {t.phantasi.rsshubOptionalConfigNote}
                  <code className="px-1 py-0.5 bg-blue-100 dark:bg-blue-800/50 rounded text-[11px] ml-1">
                    {currentRouteConfig.configNote}
                  </code>
                  <div className="mt-1 text-blue-500 dark:text-blue-400">
                    {t.phantasi.rsshubOptionalConfigHint}
                  </div>
                </>
              )}
            </div>
          </div>
        </div>
      )}

      <RSSHubAdvanced
        queryParams={queryParams}
        disabled={disabled}
        onChange={setQueryParams}
      />

      {fullUrl && (
        <div className="p-3 bg-gray-50 dark:bg-neutral-800/50 border border-gray-200 dark:border-neutral-700 rounded-xl">
          <div className="flex items-center justify-between mb-1">
            <span className="text-xs font-medium text-gray-700 dark:text-gray-300">
              {t.phantasi.rsshubFullUrl}
            </span>
            <button
              type="button"
              onClick={handleTestConnection}
              disabled={testing || disabled}
              className="flex items-center gap-1 px-2 py-1 text-xs border border-gray-300 dark:border-neutral-600 text-gray-600 dark:text-gray-400 rounded-lg hover:bg-gray-100 dark:hover:bg-neutral-700 disabled:opacity-50"
            >
              {testing ? (
                <Spinner size="xs" color="current" />
              ) : (
                <Check className="w-3 h-3" />
              )}
              {t.phantasi.rsshubTest}
            </button>
          </div>
          <div className="text-xs text-gray-600 dark:text-gray-400 font-mono break-all">
            {fullUrl}
          </div>
          {testResult && (
            <div
              className={`mt-2 flex items-center gap-1 text-xs ${testResult.success ? 'text-gray-700 dark:text-gray-300' : 'text-gray-500'}`}
            >
              {testResult.success ? (
                <Check className="w-3 h-3" />
              ) : (
                <AlertCircle className="w-3 h-3" />
              )}
              {testResult.message}
            </div>
          )}
        </div>
      )}
    </div>
  )
}

export type { RSSHubConfigProps }
