/**
 * MCP (Model Context Protocol) admin panel for Advanced settings.
 *
 * Edit `mcp_servers.json` in the UI: list / add / edit / delete / enable,
 * then save (writes disk + hot-reloads children). Uses ManagedList + form items.
 */

import type { McpServerConfig } from '../../services/agent/agentApi'
import type {
  ManagedListItem,
  ManagedListStat,
  ManagedListTone,
} from '../settings/ManagedList'
import React, { useCallback, useEffect, useMemo, useState } from 'react'
import { useAuth } from '../../contexts/AuthContext'
import { useI18n } from '../../contexts/I18nContext'
import {
  FaEdit,
  FaPlus,
  FaSearch,
  FaSyncAlt,
  LuServer,
} from '../../lib/icons'
import { agentService } from '../../services/agent'
import { userFacingError } from '../../utils/userFacingError'
import {
  InfoActionCard,
  InputItem,
  ManagedList,
  NumberItem,
  SettingFieldErrorTag,
  SettingGroup,
  SettingsButton,
  SwitchItem,
  useSettingGuide,
} from '../settings'

type Msg = (
  msg: string,
  type?: 'success' | 'error' | 'warning' | 'info',
) => void

type RuntimeMap = Record<
  string,
  { healthy: boolean; tool_count: number }
>

interface DraftServer {
  /** Original id when editing; empty when adding. */
  originalId: string
  id: string
  command: string
  /** Space-separated args for editing. */
  argsText: string
  /** KEY=value lines. */
  envText: string
  enabled: boolean
  auto_restart: boolean
  max_restart_attempts: number
  trust_annotations: boolean
}

function emptyDraft(): DraftServer {
  return {
    originalId: '',
    id: '',
    command: '',
    argsText: '',
    envText: '',
    enabled: true,
    auto_restart: true,
    max_restart_attempts: 3,
    trust_annotations: false,
  }
}

function configToDraft(s: McpServerConfig): DraftServer {
  const envText = Object.entries(s.env)
    .map(([k, v]) => `${k}=${v}`)
    .join('\n')
  return {
    originalId: s.id,
    id: s.id,
    command: s.command,
    argsText: s.args.join(' '),
    envText,
    enabled: s.enabled,
    auto_restart: s.auto_restart,
    max_restart_attempts: s.max_restart_attempts,
    trust_annotations: s.trust_annotations ?? false,
  }
}

function parseArgsText(text: string): string[] {
  // Simple whitespace split; quote-aware enough for common `npx -y pkg` cases.
  const out: string[] = []
  const re = /"([^"]*)"|'([^']*)'|(\S+)/g
  let m: RegExpExecArray | null
  // eslint-disable-next-line no-cond-assign -- standard re.exec loop
  while ((m = re.exec(text)) !== null) {
    out.push(m[1] ?? m[2] ?? m[3] ?? '')
  }
  return out.filter(Boolean)
}

function parseEnvText(text: string): Record<string, string> {
  const env: Record<string, string> = {}
  for (const line of text.split(/\r?\n/)) {
    const trimmed = line.trim()
    if (!trimmed || trimmed.startsWith('#')) continue
    const eq = trimmed.indexOf('=')
    if (eq <= 0) continue
    const key = trimmed.slice(0, eq).trim()
    const value = trimmed.slice(eq + 1)
    if (key) env[key] = value
  }
  return env
}

function draftToConfig(d: DraftServer): McpServerConfig {
  return {
    id: d.id.trim(),
    command: d.command.trim(),
    args: parseArgsText(d.argsText),
    env: parseEnvText(d.envText),
    enabled: d.enabled,
    auto_restart: d.auto_restart,
    max_restart_attempts: Math.max(
      0,
      Math.min(50, Math.floor(d.max_restart_attempts) || 0),
    ),
    trust_annotations: d.trust_annotations,
  }
}

export interface McpConfigPanelProps {
  onMessage?: Msg
}

/**
 * Advanced → MCP: full config editor + live status (admin only).
 */
export function McpConfigPanel({ onMessage }: McpConfigPanelProps) {
  const { t } = useI18n()
  const { isAdmin } = useAuth()
  const { catalog: g, bindGuide } = useSettingGuide()
  const c = t.config

  const [loading, setLoading] = useState(false)
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [servers, setServers] = useState<McpServerConfig[]>([])
  const [runtime, setRuntime] = useState<RuntimeMap>({})
  const [toolCount, setToolCount] = useState(0)
  const [query, setQuery] = useState('')
  const [filter, setFilter] = useState<'all' | 'enabled' | 'disabled' | 'unhealthy'>(
    'all',
  )
  const [formOpen, setFormOpen] = useState(false)
  const [draft, setDraft] = useState<DraftServer>(emptyDraft)
  const [formError, setFormError] = useState<string | null>(null)
  const [expandedId, setExpandedId] = useState<string | null>(null)

  const applySnapshot = useCallback(
    (snap: Awaited<ReturnType<typeof agentService.getMcpConfig>>) => {
      setServers(snap.servers)
      setToolCount(snap.toolCount)
      const map: RuntimeMap = {}
      for (const r of snap.runtimeServers) {
        map[r.id] = { healthy: r.healthy, tool_count: r.tool_count }
      }
      setRuntime(map)
    },
    [],
  )

  const load = useCallback(async () => {
    if (!isAdmin) return
    setLoading(true)
    setError(null)
    try {
      const snap = await agentService.getMcpConfig()
      applySnapshot(snap)
    } catch (e) {
      const msg = userFacingError(e, c.mcpLoadFailed)
      setError(msg)
      setServers([])
      setRuntime({})
      setToolCount(0)
    } finally {
      setLoading(false)
    }
  }, [isAdmin, c.mcpLoadFailed, applySnapshot])

  useEffect(() => {
    void load()
  }, [load])

  const persist = useCallback(
    async (next: McpServerConfig[], successMsg: string) => {
      setSaving(true)
      setError(null)
      try {
        const snap = await agentService.putMcpConfig(next)
        applySnapshot(snap)
        onMessage?.(successMsg, 'success')
        return true
      } catch (e) {
        const msg = userFacingError(e, c.mcpSaveFailed)
        setError(msg)
        onMessage?.(msg, 'error')
        return false
      } finally {
        setSaving(false)
      }
    },
    [applySnapshot, onMessage, c.mcpSaveFailed],
  )

  const openAdd = useCallback(() => {
    setDraft(emptyDraft())
    setFormError(null)
    setFormOpen(true)
  }, [])

  const openEdit = useCallback((s: McpServerConfig) => {
    setDraft(configToDraft(s))
    setFormError(null)
    setFormOpen(true)
  }, [])

  const closeForm = useCallback(() => {
    setFormOpen(false)
    setDraft(emptyDraft())
    setFormError(null)
  }, [])

  const submitForm = useCallback(async () => {
    const nextCfg = draftToConfig(draft)
    if (!nextCfg.id) {
      setFormError(c.mcpValidateIdRequired)
      return
    }
    if (!/^[\w.-]+$/.test(nextCfg.id)) {
      setFormError(c.mcpValidateIdCharset)
      return
    }
    if (!nextCfg.command) {
      setFormError(c.mcpValidateCommandRequired)
      return
    }
    const isEdit = Boolean(draft.originalId)
    const duplicate = servers.some(
      (s) =>
        s.id === nextCfg.id &&
        (!isEdit || s.id !== draft.originalId),
    )
    if (duplicate) {
      setFormError(c.mcpValidateIdDuplicate)
      return
    }

    let next: McpServerConfig[]
    if (isEdit) {
      next = servers.map((s) =>
        s.id === draft.originalId ? nextCfg : s,
      )
    } else {
      next = [...servers, nextCfg]
    }

    const ok = await persist(
      next,
      isEdit ? c.mcpSaveUpdated : c.mcpSaveCreated,
    )
    if (ok) closeForm()
  }, [draft, servers, persist, closeForm, c])

  const removeServer = useCallback(
    async (id: string) => {
      const next = servers.filter((s) => s.id !== id)
      await persist(next, c.mcpSaveDeleted)
    },
    [servers, persist, c.mcpSaveDeleted],
  )

  const toggleEnabled = useCallback(
    async (id: string, enabled: boolean) => {
      const next = servers.map((s) =>
        s.id === id ? { ...s, enabled } : s,
      )
      // Optimistic
      setServers(next)
      const ok = await persist(next, c.mcpSaveUpdated)
      if (!ok) void load()
    },
    [servers, persist, load, c.mcpSaveUpdated],
  )

  const healthyCount = useMemo(
    () => Object.values(runtime).filter((r) => r.healthy).length,
    [runtime],
  )
  const enabledCount = useMemo(
    () => servers.filter((s) => s.enabled).length,
    [servers],
  )

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase()
    return servers.filter((s) => {
      if (filter === 'enabled' && !s.enabled) return false
      if (filter === 'disabled' && s.enabled) return false
      if (filter === 'unhealthy') {
        const rt = runtime[s.id]
        if (!s.enabled || (rt && rt.healthy)) return false
      }
      if (!q) return true
      return (
        s.id.toLowerCase().includes(q) ||
        s.command.toLowerCase().includes(q)
      )
    })
  }, [servers, query, filter, runtime])

  const stats: ManagedListStat[] = useMemo(
    () => [
      {
        key: 'servers',
        label: c.mcpStatServers,
        value: servers.length,
        tone: servers.length === 0 ? 'muted' : 'default',
      },
      {
        key: 'enabled',
        label: c.mcpStatEnabled,
        value: enabledCount,
        tone: 'default',
      },
      {
        key: 'healthy',
        label: c.mcpStatHealthy,
        value: healthyCount,
        tone: 'success',
      },
      {
        key: 'tools',
        label: c.mcpStatTools,
        value: toolCount,
        tone: 'default',
      },
    ],
    [c, servers.length, enabledCount, healthyCount, toolCount],
  )

  const items: ManagedListItem[] = useMemo(
    () =>
      filtered.map((s) => {
        const rt = runtime[s.id]
        let badge: { label: string; tone: ManagedListTone }
        if (!s.enabled) {
          badge = { label: c.mcpStatusDisabled, tone: 'muted' }
        } else if (rt?.healthy) {
          badge = { label: c.mcpStatusHealthy, tone: 'success' }
        } else if (rt) {
          badge = { label: c.mcpStatusUnhealthy, tone: 'danger' }
        } else {
          badge = { label: c.mcpStatusStopped, tone: 'warn' }
        }
        const tools = rt?.tool_count ?? 0
        const expanded = expandedId === s.id
        return {
          id: s.id,
          title: s.id,
          subtitle: `${s.command}${s.args.length ? ` ${s.args.join(' ')}` : ''}`,
          meta: c.mcpToolsCount.replace('{n}', String(tools)),
          badge,
          badges: [
            {
              label: s.auto_restart
                ? c.mcpAutoRestartOn
                : c.mcpAutoRestartOff,
              tone: s.auto_restart ? 'default' : 'muted',
            },
          ],
          // 行操作与联邦内容过滤一致：动词 label + variant（无 chrome 双行描述）
          actions: [
            {
              key: 'toggle',
              label: s.enabled ? c.mcpDisable : c.mcpEnable,
              variant: s.enabled ? 'secondary' : 'primary',
              onClick: () => void toggleEnabled(s.id, !s.enabled),
              disabled: saving,
              loading: saving,
            },
            {
              key: 'edit',
              label: t.common.edit,
              variant: 'secondary',
              onClick: () => openEdit(s),
              disabled: saving,
            },
            {
              key: 'delete',
              label: t.common.delete,
              variant: 'danger',
              confirm: c.mcpDeleteConfirm.replace('{id}', s.id),
              onClick: () => void removeServer(s.id),
              disabled: saving,
            },
          ],
          expanded,
          onToggleExpand: () =>
            setExpandedId((cur) => (cur === s.id ? null : s.id)),
          expandContent: (
            <InfoActionCard
              embedded
              tone={
                !s.enabled
                  ? 'muted'
                  : rt?.healthy
                    ? 'success'
                    : 'danger'
              }
              fields={[
                {
                  key: 'id',
                  label: c.mcpFieldId,
                  value: s.id,
                  mono: true,
                  copyText: s.id,
                },
                {
                  key: 'command',
                  label: c.mcpFieldCommand,
                  value: [s.command, ...s.args].join(' '),
                  mono: true,
                  copyText: [s.command, ...s.args].join(' '),
                },
                {
                  key: 'enabled',
                  label: c.mcpFieldEnabled,
                  value: s.enabled ? t.common.enabled : t.common.disabled,
                  copyable: false,
                },
                {
                  key: 'health',
                  label: c.mcpFieldHealth,
                  value: badge.label,
                  copyable: false,
                },
                {
                  key: 'tools',
                  label: c.mcpFieldTools,
                  value: String(tools),
                  copyable: false,
                },
                {
                  key: 'env',
                  label: c.mcpFieldEnv,
                  value:
                    Object.keys(s.env).length > 0
                      ? Object.keys(s.env).join(', ')
                      : '—',
                  copyable: false,
                },
              ]}
              actions={[
                {
                  key: 'edit',
                  label: t.common.edit,
                  onClick: () => openEdit(s),
                  variant: 'secondary',
                  title: c.mcpEditDesc,
                },
              ]}
            />
          ),
        }
      }),
    [
      filtered,
      runtime,
      expandedId,
      c,
      t,
      saving,
      toggleEnabled,
      openEdit,
      removeServer,
    ],
  )

  // 校验错误贴在对应字段标题旁（SettingFieldErrorTag），非更新器 toast 区
  const idFieldError =
    formError === c.mcpValidateIdRequired ||
    formError === c.mcpValidateIdCharset ||
    formError === c.mcpValidateIdDuplicate
      ? formError
      : undefined
  const commandFieldError =
    formError === c.mcpValidateCommandRequired ? formError : undefined

  const formBody = (
    <div className="flex flex-col gap-3 p-1">
      <InputItem
        itemKey="mcp_id"
        label={c.mcpFieldId}
        description={c.mcpFieldIdHint}
        {...bindGuide('advanced.mcpId', g.advanced.mcpId)}
        value={draft.id}
        onChange={(v) => {
          setFormError(null)
          setDraft((d) => ({ ...d, id: v }))
        }}
        placeholder="github"
        layout="vertical"
        disabled={saving}
        error={idFieldError}
      />
      <InputItem
        itemKey="mcp_command"
        label={c.mcpFieldCommand}
        description={c.mcpFieldCommandHint}
        {...bindGuide('advanced.mcpCommand', g.advanced.mcpCommand)}
        value={draft.command}
        onChange={(v) => {
          setFormError(null)
          setDraft((d) => ({ ...d, command: v }))
        }}
        placeholder="npx"
        layout="vertical"
        disabled={saving}
        error={commandFieldError}
      />
      <InputItem
        itemKey="mcp_args"
        label={c.mcpFieldArgs}
        description={c.mcpFieldArgsHint}
        {...bindGuide('advanced.mcpArgs', g.advanced.mcpArgs)}
        value={draft.argsText}
        onChange={(v) => setDraft((d) => ({ ...d, argsText: v }))}
        placeholder="-y @modelcontextprotocol/server-github"
        layout="vertical"
        disabled={saving}
      />
      <InputItem
        itemKey="mcp_env"
        label={c.mcpFieldEnv}
        description={c.mcpFieldEnvHint}
        {...bindGuide('advanced.mcpEnv', g.advanced.mcpEnv)}
        value={draft.envText}
        onChange={(v) => setDraft((d) => ({ ...d, envText: v }))}
        placeholder={'GITHUB_TOKEN=ghp_…\nFOO=bar'}
        layout="vertical"
        multiline
        rows={4}
        disabled={saving}
      />
      <NumberItem
        itemKey="mcp_max_restart"
        label={c.mcpFieldMaxRestart}
        description={c.mcpFieldMaxRestartHint}
        {...bindGuide('advanced.mcpMaxRestart', g.advanced.mcpMaxRestart)}
        value={draft.max_restart_attempts}
        onChange={(v) =>
          setDraft((d) => ({
            ...d,
            max_restart_attempts: typeof v === 'number' ? v : 3,
          }))
        }
        min={0}
        max={50}
        layout="horizontal"
        disabled={saving}
      />
      <SwitchItem
        itemKey="mcp_enabled"
        label={c.mcpFieldEnabled}
        description={c.mcpFieldEnabledHint}
        {...bindGuide('advanced.mcpEnabled', g.advanced.mcpEnabled)}
        value={draft.enabled}
        onChange={(v) => setDraft((d) => ({ ...d, enabled: v }))}
        layout="horizontal"
        disabled={saving}
      />
      <SwitchItem
        itemKey="mcp_auto_restart"
        label={c.mcpFieldAutoRestart}
        description={c.mcpFieldAutoRestartHint}
        {...bindGuide('advanced.mcpAutoRestart', g.advanced.mcpAutoRestart)}
        value={draft.auto_restart}
        onChange={(v) => setDraft((d) => ({ ...d, auto_restart: v }))}
        layout="horizontal"
        disabled={saving}
      />
      <SwitchItem
        itemKey="mcp_trust_annotations"
        label={c.mcpFieldTrustAnnotations}
        description={c.mcpFieldTrustAnnotationsHint}
        {...bindGuide(
          'advanced.mcpTrustAnnotations',
          g.advanced.mcpTrustAnnotations,
        )}
        value={draft.trust_annotations}
        onChange={(v) => setDraft((d) => ({ ...d, trust_annotations: v }))}
        layout="horizontal"
        disabled={saving}
      />
      {/* 与联邦「添加过滤规则」一致：表单底部 primary 提交，收起由 formCollapse 负责 */}
      <div className="managed-list-form-actions">
        <SettingsButton
          variant="primary"
          size="sm"
          loading={saving}
          onClick={() => void submitForm()}
        >
          {draft.originalId ? c.mcpFormSaveEdit : c.mcpFormSaveAdd}
        </SettingsButton>
      </div>
    </div>
  )

  if (!isAdmin) return null

  return (
    <SettingGroup
      title={c.mcpTitle}
      description={c.mcpDesc}
      icon={<LuServer />}
      className="mcp-config-panel"
      titleExtra={
        error ? (
          <SettingFieldErrorTag title={error}>{error}</SettingFieldErrorTag>
        ) : null
      }
      {...bindGuide('advanced.mcp', g.advanced.mcp)}
    >
      <ManagedList
        stats={stats}
        // 工具栏：与联邦投递队列相同 — label + description + icon
        toolbar={[
          {
            key: 'refresh',
            label: t.common.refresh,
            description: c.mcpRefreshDesc,
            icon: <FaSyncAlt aria-hidden />,
            onClick: () => void load(),
            loading,
            disabled: loading || saving,
            variant: 'secondary',
          },
        ]}
        // 查询栏 chrome：与联邦已知实例 / 内容过滤共用文案
        queryToggleLabel={c.federationListQueryToggle}
        queryToggleDescription={c.federationListQueryToggleDesc}
        queryToggleIcon={<FaSearch aria-hidden />}
        queryCollapseLabel={c.federationListQueryCollapse}
        queryCollapseDescription={c.federationListQueryCollapseDesc}
        search={{
          value: query,
          onChange: setQuery,
          placeholder: c.mcpSearchPlaceholder,
          ariaLabel: c.mcpSearchPlaceholder,
        }}
        filters={{
          value: filter,
          onChange: (key) =>
            setFilter(
              key as 'all' | 'enabled' | 'disabled' | 'unhealthy',
            ),
          ariaLabel: c.mcpFilterAria,
          options: [
            { key: 'all', label: c.mcpFilterAll, count: servers.length },
            {
              key: 'enabled',
              label: c.mcpFilterEnabled,
              count: enabledCount,
            },
            {
              key: 'disabled',
              label: c.mcpFilterDisabled,
              count: servers.length - enabledCount,
            },
            {
              key: 'unhealthy',
              label: c.mcpFilterUnhealthy,
              count: servers.filter((s) => {
                const rt = runtime[s.id]
                return s.enabled && (!rt || !rt.healthy)
              }).length,
            },
          ],
        }}
        // 添加/编辑表单 chrome：对齐联邦「添加规则」
        formTitle={
          draft.originalId ? c.mcpFormEditTitle : c.mcpAddServer
        }
        formDescription={
          draft.originalId ? c.mcpFormEditTitleDesc : c.mcpAddServerDesc
        }
        formIcon={
          draft.originalId ? (
            <FaEdit aria-hidden />
          ) : (
            <FaPlus aria-hidden />
          )
        }
        formCollapseLabel={t.common.cancel}
        formCollapseDescription={c.federationListQueryCollapseDesc}
        formOpen={formOpen}
        onFormOpenChange={(open) => {
          if (!open) closeForm()
          else if (!formOpen) openAdd()
        }}
        form={formBody}
        items={items}
        emptyText={
          loading
            ? t.common.loading
            : servers.length === 0
              ? c.mcpEmpty
              : c.mcpEmptyFiltered
        }
        loading={loading && servers.length === 0}
        working={saving}
        maxHeight={null}
      />
    </SettingGroup>
  )
}

export default McpConfigPanel
