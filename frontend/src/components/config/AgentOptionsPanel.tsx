import type { ReactNode } from 'react'
import type {
  HeartbeatTask,
  MemoryEntry,
  SkillInfo,
} from '../../services/agent'
import type { SchedulePreset } from '../agent-panel/agentSchedule'
import type {
  ManagedListItem,
  ManagedListStat,
  ManagedListTone,
} from '../settings'
import { FaSearch, LuRefreshCw } from '@lib/icons'
import React, { useCallback, useEffect, useMemo, useState } from 'react'
import { useAuth } from '../../contexts/AuthContext'
import { useI18n } from '../../contexts/I18nContext'
import { agentService } from '../../services/agent'
import { userFacingError } from '../../utils/userFacingError'
import { relativeTimeBucket } from '../agent-panel/agentRelativeTime'
import {
  describeSchedule,
  isPlausibleCron,
  matchSchedulePreset,
  SCHEDULE_PRESETS,
} from '../agent-panel/agentSchedule'
import {
  guideDomProps,
  InputItem,
  ManagedList,
  SegmentedControl,
  SettingFieldErrorTag,
  SettingsButton,
  SettingTitleGuideEntry,
  ToggleSwitch,
  useSettingGuide,
} from '../settings'
import { agentOptionsListWindow } from './agentOptionsList'

export function AgentNestedSection({
  title,
  description,
  badge,
  error,
  toggle,
  guide,
  guidePath,
  tourAnchor,
  toggleTourAnchor,
  children,
}: {
  title: string
  description?: string
  badge?: ReactNode
  error?: ReactNode
  toggle?: {
    checked: boolean
    onChange: (checked: boolean) => void
    disabled?: boolean
    ariaLabel?: string
    title?: string
  }
  guide?: ReactNode
  guidePath?: string
  tourAnchor?: string
  toggleTourAnchor?: string
  children?: ReactNode
}) {
  return (
    <div
      className={`ai-llm-tier${guidePath ? ' has-guide-anchor' : ''}`}
      data-tour={tourAnchor}
      {...guideDomProps(guidePath)}
    >
      <div className="ai-llm-tier-head" data-tour={toggleTourAnchor}>
        <div className="ai-llm-tier-copy">
          <h3 className="ai-llm-tier-title">
            {title}
            {badge}
            <SettingTitleGuideEntry title={title} guide={guide} />
          </h3>
          {description ? (
            <p className="ai-llm-tier-desc">{description}</p>
          ) : null}
          {error}
        </div>
        {toggle ? (
          <div className="ai-llm-tier-switch">
            <ToggleSwitch
              checked={toggle.checked}
              onChange={toggle.onChange}
              disabled={toggle.disabled}
              aria-label={toggle.ariaLabel ?? title}
              title={toggle.title}
            />
          </div>
        ) : null}
      </div>
      {children}
    </div>
  )
}

interface TaskDraft {
  id: string | null
  name: string
  schedule: string
  action: string
}

const EMPTY_DRAFT: TaskDraft = {
  id: null,
  name: '',
  schedule: '0 9 * * *',
  action: '',
}

function presetLabel(
  id: Exclude<SchedulePreset, 'custom'> | 'custom',
  m: {
    preset15m: string
    preset30m: string
    preset1h: string
    preset6h: string
    presetDaily9: string
    presetCustom: string
  },
): string {
  switch (id) {
    case '15m':
      return m.preset15m
    case '30m':
      return m.preset30m
    case '1h':
      return m.preset1h
    case '6h':
      return m.preset6h
    case 'daily9':
      return m.presetDaily9
    default:
      return m.presetCustom
  }
}

function skillOriginBadge(
  origin: SkillInfo['origin'],
  m: {
    originManual: string
    originLearned: string
    originImproved: string
  },
): { label: string; tone: ManagedListTone } {
  if (origin === 'manual') return { label: m.originManual, tone: 'default' }
  if (origin === 'agent_improved') {
    return { label: m.originImproved, tone: 'active' }
  }
  return { label: m.originLearned, tone: 'success' }
}

export const AgentOptionsPanel: React.FC = () => {
  const { t, format, locale } = useI18n()
  const { isAuthenticated } = useAuth()
  const { catalog: g, bindGuide } = useSettingGuide()
  const m = t.agentPanel.manage
  const [tasks, setTasks] = useState<HeartbeatTask[]>([])
  const [skills, setSkills] = useState<SkillInfo[]>([])
  const [memories, setMemories] = useState<MemoryEntry[]>([])
  const [note, setNote] = useState<{
    area: 'heartbeat' | 'skills' | 'memory' | 'all'
    text: string
  } | null>(null)
  const [loading, setLoading] = useState(false)
  const [busyKey, setBusyKey] = useState<string | null>(null)
  const [draft, setDraft] = useState<TaskDraft | null>(null)
  const [expandedTaskId, setExpandedTaskId] = useState<string | null>(null)
  const [expandedMemoryId, setExpandedMemoryId] = useState<string | null>(null)
  const [memoryDraft, setMemoryDraft] = useState('')
  const [taskQuery, setTaskQuery] = useState('')
  const [taskFilter, setTaskFilter] = useState('all')
  const [skillQuery, setSkillQuery] = useState('')
  const [skillFilter, setSkillFilter] = useState('all')
  const [memoryQuery, setMemoryQuery] = useState('')

  const load = useCallback(async () => {
    setNote(null)
    if (!isAuthenticated) {
      setTasks([])
      setSkills([])
      setMemories([])
      return
    }
    setLoading(true)
    try {
      const nextSkills = await agentService.getSkills()
      const nextMemories = await agentService.getMemories()
      setSkills(nextSkills)
      setMemories(nextMemories)
      setTasks(await agentService.getHeartbeatTasks())
    } catch (error) {
      setNote({
        area: 'all',
        text: userFacingError(error, m.loadFailed),
      })
    } finally {
      setLoading(false)
    }
  }, [isAuthenticated, m.loadFailed])

  useEffect(() => {
    void load()
  }, [load])

  const describe = useCallback(
    (task: HeartbeatTask): string => {
      const shape = describeSchedule(task.schedule)
      switch (shape.kind) {
        case 'everyMinutes':
          return format(m.everyMinutes, { value: shape.value })
        case 'everyHours':
          return format(m.everyHours, { value: shape.value })
        case 'hourly':
          return m.hourly
        case 'dailyAt':
          return format(m.dailyAt, { time: shape.time })
        default:
          return shape.cron
      }
    },
    [format, m],
  )

  const lastRunLabel = useCallback(
    (task: HeartbeatTask): string => {
      const bucket = relativeTimeBucket(task.lastRun, Date.now())
      if (!bucket) return m.neverRun
      const time =
        bucket.kind === 'justNow'
          ? t.agentPanel.sessions.justNow
          : bucket.kind === 'minutes'
            ? format(t.agentPanel.sessions.minutesAgo, { value: bucket.value })
            : bucket.kind === 'hours'
              ? format(t.agentPanel.sessions.hoursAgo, { value: bucket.value })
              : bucket.kind === 'days'
                ? format(t.agentPanel.sessions.daysAgo, { value: bucket.value })
                : bucket.date.toLocaleDateString(locale || undefined)
      return format(m.lastRun, { time })
    },
    [format, locale, m, t.agentPanel.sessions],
  )

  const guard = useCallback(
    async (key: string, run: () => Promise<unknown>): Promise<boolean> => {
      setBusyKey(key)
      setNote(null)
      try {
        await run()
        await load()
        return true
      } catch (error) {
        setNote({
          area: key.startsWith('skill:')
            ? 'skills'
            : key.startsWith('memory:')
              ? 'memory'
              : 'heartbeat',
          text: userFacingError(error, m.actionFailed),
        })
        return false
      } finally {
        setBusyKey(null)
      }
    },
    [load, m.actionFailed],
  )

  const saveDraft = async () => {
    if (!draft) return
    if (!isPlausibleCron(draft.schedule)) {
      setNote({ area: 'heartbeat', text: m.badCron })
      return
    }
    const body = {
      name: draft.name.trim(),
      schedule: draft.schedule.trim(),
      action: draft.action.trim(),
    }
    if (!body.name || !body.action) return
    const key = draft.id ? `task:${draft.id}` : 'task:new'
    const ok = await guard(key, () =>
      draft.id
        ? agentService.updateHeartbeat(draft.id, body)
        : agentService.createHeartbeat({ ...body, enabled: true }),
    )
    if (!ok) return
    setDraft(null)
    setExpandedTaskId(null)
  }

  const adding = draft != null && draft.id == null
  const editingTask =
    draft != null && draft.id != null ? draft : null
  const preset: SchedulePreset = draft
    ? matchSchedulePreset(draft.schedule)
    : 'custom'

  const taskFields = draft ? (
    <>
      <InputItem
        itemKey="agent-heartbeat-name"
        label={m.taskName}
        value={draft.name}
        onChange={(value) => setDraft({ ...draft, name: value })}
        layout="vertical"
      />
      <InputItem
        itemKey="agent-heartbeat-action"
        label={m.taskAction}
        hint={m.taskActionHint}
        value={draft.action}
        onChange={(value) => setDraft({ ...draft, action: value })}
        layout="vertical"
        multiline
        rows={3}
      />
      <SegmentedControl
        size="sm"
        ariaLabel={m.taskSchedule}
        value={preset}
        columns="auto"
        options={[
          ...SCHEDULE_PRESETS.map((option) => ({
            value: option.id,
            label: presetLabel(option.id, m),
          })),
          { value: 'custom' as const, label: m.presetCustom },
        ]}
        onChange={(id) => {
          if (id === 'custom') return
          const cron = SCHEDULE_PRESETS.find((option) => option.id === id)?.cron
          if (cron) setDraft({ ...draft, schedule: cron })
        }}
      />
      <InputItem
        itemKey="agent-heartbeat-cron"
        label={m.taskSchedule}
        hint={m.taskScheduleHint}
        value={draft.schedule}
        onChange={(value) => setDraft({ ...draft, schedule: value })}
        layout="vertical"
        error={
          draft.schedule.trim() && !isPlausibleCron(draft.schedule)
            ? m.badCron
            : undefined
        }
      />
      <div className="managed-list-form-actions">
        {editingTask ? (
          <SettingsButton
            variant="secondary"
            size="sm"
            onClick={() => {
              setDraft(null)
              setExpandedTaskId(null)
            }}
          >
            {m.cancel}
          </SettingsButton>
        ) : null}
        <SettingsButton
          variant="primary"
          size="sm"
          disabled={!draft.name.trim() || !draft.action.trim()}
          loading={busyKey === (draft.id ? `task:${draft.id}` : 'task:new')}
          onClick={() => void saveDraft()}
        >
          {m.save}
        </SettingsButton>
      </div>
    </>
  ) : null

  const enabledCount = tasks.filter((task) => task.enabled).length
  const heartbeatStats: ManagedListStat[] = useMemo(
    () => [
      {
        key: 'tasks',
        label: m.tabs.heartbeat,
        value: tasks.length,
        tone: tasks.length === 0 ? 'muted' : 'default',
      },
      {
        key: 'enabled',
        label: m.enabled,
        value: enabledCount,
        tone: enabledCount > 0 ? 'success' : 'muted',
      },
    ],
    [enabledCount, m.enabled, m.tabs.heartbeat, tasks.length],
  )

  const filteredTasks = useMemo(() => {
    const q = taskQuery.trim().toLowerCase()
    return tasks.filter((task) => {
      if (taskFilter === 'enabled' && !task.enabled) return false
      if (taskFilter === 'disabled' && task.enabled) return false
      if (!q) return true
      return (
        task.name.toLowerCase().includes(q) ||
        userFacingError(task.name).toLowerCase().includes(q) ||
        task.action.toLowerCase().includes(q)
      )
    })
  }, [taskFilter, taskQuery, tasks])

  const heartbeatItems: ManagedListItem[] = useMemo(
    () =>
      filteredTasks.map((task) => {
        const expanded = expandedTaskId === task.id
        const result = task.lastResult?.trim()
        const displayName = userFacingError(task.name)
        return {
          id: task.id,
          title: displayName,
          subtitle: describe(task),
          meta: result
            ? `${lastRunLabel(task)} · ${result}`
            : lastRunLabel(task),
          badge: task.enabled
            ? { label: m.enabled, tone: 'success' }
            : { label: m.disabled, tone: 'muted' },
          trailing: (
            <ToggleSwitch
              checked={task.enabled}
              onChange={() =>
                void guard(`task:${task.id}`, () =>
                  agentService.toggleHeartbeat(task.id),
                )
              }
              disabled={busyKey === `task:${task.id}`}
              aria-label={format(m.enableTask, { name: displayName })}
            />
          ),
          actions: [
            {
              key: 'remove',
              label: m.remove,
              variant: 'ghost',
              confirm: format(m.confirmRemove, { name: displayName }),
              onClick: () =>
                void guard(`task:${task.id}`, () =>
                  agentService.deleteHeartbeat(task.id),
                ),
            },
          ],
          expanded,
          onToggleExpand: () => {
            if (expanded) {
              setExpandedTaskId(null)
              setDraft(null)
              return
            }
            setExpandedTaskId(task.id)
            setDraft({
              id: task.id,
              name: task.name,
              schedule: task.schedule,
              action: task.action,
            })
          },
          expandContent: expanded ? taskFields : null,
          busy: busyKey === `task:${task.id}`,
        }
      }),
    [
      busyKey,
      describe,
      expandedTaskId,
      format,
      guard,
      lastRunLabel,
      m,
      taskFields,
      filteredTasks,
    ],
  )

  const learnedCount = skills.filter(
    (skill) => skill.origin === 'agent_generated',
  ).length
  const manualCount = skills.filter((skill) => skill.origin === 'manual').length
  const improvedCount = skills.filter(
    (skill) => skill.origin === 'agent_improved',
  ).length

  const skillStats: ManagedListStat[] = useMemo(
    () => [
      {
        key: 'skills',
        label: m.tabs.skills,
        value: skills.length,
        tone: skills.length === 0 ? 'muted' : 'default',
      },
      {
        key: 'learned',
        label: m.originLearned,
        value: learnedCount,
        tone: learnedCount > 0 ? 'success' : 'muted',
      },
    ],
    [learnedCount, m.originLearned, m.tabs.skills, skills.length],
  )

  const memoryStats: ManagedListStat[] = useMemo(
    () => [
      {
        key: 'memory',
        label: m.tabs.memory,
        value: memories.length,
        tone: memories.length === 0 ? 'muted' : 'default',
      },
    ],
    [m.tabs.memory, memories.length],
  )

  const filteredSkills = useMemo(() => {
    const q = skillQuery.trim().toLowerCase()
    return skills.filter((skill) => {
      if (skillFilter === 'learned' && skill.origin !== 'agent_generated') {
        return false
      }
      if (skillFilter === 'manual' && skill.origin !== 'manual') return false
      if (skillFilter === 'improved' && skill.origin !== 'agent_improved') {
        return false
      }
      if (!q) return true
      return (
        skill.name.toLowerCase().includes(q) ||
        skill.description.toLowerCase().includes(q) ||
        skill.category.toLowerCase().includes(q)
      )
    })
  }, [skillFilter, skillQuery, skills])

  const skillItems: ManagedListItem[] = useMemo(
    () =>
      filteredSkills.map((skill) => ({
        id: skill.id,
        title: skill.name,
        subtitle: skill.description,
        meta:
          skill.successCount != null || skill.failureCount != null
            ? format(m.skillRecord, {
                ok: skill.successCount ?? 0,
                fail: skill.failureCount ?? 0,
              })
            : undefined,
        badge: skillOriginBadge(skill.origin, m),
        actions: [
          {
            key: 'remove',
            label: m.remove,
            variant: 'ghost',
            confirm: format(m.confirmRemove, { name: skill.name }),
            onClick: () =>
              void guard(`skill:${skill.id}`, () =>
                agentService.deleteSkill(skill.id),
              ),
          },
        ],
        busy: busyKey === `skill:${skill.id}`,
      })),
    [busyKey, filteredSkills, format, guard, m],
  )

  const filteredMemories = useMemo(() => {
    const q = memoryQuery.trim().toLowerCase()
    if (!q) return memories
    return memories.filter((memory) =>
      memory.content.toLowerCase().includes(q),
    )
  }, [memories, memoryQuery])

  const memoryItems: ManagedListItem[] = useMemo(
    () =>
      filteredMemories.map((memory) => {
        const expanded = expandedMemoryId === memory.id
        return {
          id: memory.id,
          title: memory.content,
          meta: memory.createdAt
            ? new Date(memory.createdAt).toLocaleString(locale || undefined)
            : undefined,
          expanded,
          onToggleExpand: () => {
            if (expanded) {
              setExpandedMemoryId(null)
              setMemoryDraft('')
              return
            }
            setExpandedMemoryId(memory.id)
            setMemoryDraft(memory.content)
          },
          expandContent: expanded ? (
            <>
              <InputItem
                itemKey={`agent-memory-${memory.id}`}
                label={m.memoryContent}
                value={memoryDraft}
                onChange={setMemoryDraft}
                layout="vertical"
                multiline
                rows={4}
              />
              <div className="managed-list-form-actions">
                <SettingsButton
                  variant="secondary"
                  size="sm"
                  onClick={() => {
                    setExpandedMemoryId(null)
                    setMemoryDraft('')
                  }}
                >
                  {m.cancel}
                </SettingsButton>
                <SettingsButton
                  variant="primary"
                  size="sm"
                  disabled={!memoryDraft.trim()}
                  loading={busyKey === `memory:${memory.id}`}
                  onClick={() => {
                    const text = memoryDraft.trim()
                    void guard(`memory:${memory.id}`, () =>
                      agentService.updateMemory(memory.id, text),
                    ).then((ok) => {
                      if (!ok) return
                      setExpandedMemoryId(null)
                      setMemoryDraft('')
                    })
                  }}
                >
                  {m.save}
                </SettingsButton>
              </div>
            </>
          ) : null,
          actions: [
            {
              key: 'remove',
              label: m.remove,
              variant: 'ghost',
              confirm: m.confirmRemoveMemory,
              onClick: () =>
                void guard(`memory:${memory.id}`, () =>
                  agentService.deleteMemory(memory.id),
                ),
            },
          ],
          busy: busyKey === `memory:${memory.id}`,
        }
      }),
    [
      busyKey,
      expandedMemoryId,
      filteredMemories,
      guard,
      locale,
      m,
      memoryDraft,
    ],
  )

  const errorFor = (
    area: 'heartbeat' | 'skills' | 'memory',
  ): React.ReactNode => {
    if (!note) return null
    if (note.area !== 'all' && note.area !== area) return null
    return (
      <SettingFieldErrorTag title={note.text}>{note.text}</SettingFieldErrorTag>
    )
  }

  const queryChrome = {
    queryToggleLabel: t.config.federationListQueryToggle,
    queryToggleDescription: t.config.federationListQueryToggleDesc,
    queryToggleIcon: <FaSearch aria-hidden />,
    queryCollapseLabel: t.config.federationListQueryCollapse,
    queryCollapseDescription: t.config.federationListQueryCollapseDesc,
  }

  const refreshAction = {
    key: 'refresh',
    label: t.common.refresh,
    description: m.refreshDesc,
    icon: <LuRefreshCw aria-hidden />,
    onClick: () => void load(),
    loading,
    disabled: loading,
    variant: 'secondary' as const,
  }

  return (
    <>
      <AgentNestedSection
        title={t.config.agentHeartbeatTitle}
        description={t.config.agentHeartbeatDesc}
        error={errorFor('heartbeat')}
        {...bindGuide('ai.heartbeat', g.ai.heartbeat)}
      >
        <ManagedList
          stats={heartbeatStats}
          loading={loading}
          {...agentOptionsListWindow(filteredTasks.length)}
          emptyText={
            tasks.length === 0 ? m.emptyHeartbeat : m.noneMatch
          }
          toolbar={[refreshAction]}
          {...queryChrome}
          search={
            tasks.length > 0
              ? {
                  value: taskQuery,
                  onChange: setTaskQuery,
                  placeholder: m.searchTasks,
                  ariaLabel: m.searchTasks,
                }
              : undefined
          }
          filters={
            tasks.length > 0
              ? {
                  value: taskFilter,
                  onChange: setTaskFilter,
                  ariaLabel: t.config.federationListQueryToggle,
                  options: [
                    {
                      key: 'all',
                      label: t.config.mcpFilterAll,
                      count: tasks.length,
                    },
                    {
                      key: 'enabled',
                      label: m.enabled,
                      count: enabledCount,
                    },
                    {
                      key: 'disabled',
                      label: m.disabled,
                      count: tasks.length - enabledCount,
                    },
                  ],
                }
              : undefined
          }
          form={taskFields ?? <></>}
          formTitle={m.newTask}
          formDescription={m.newTaskDesc}
          formOpen={adding}
          onFormOpenChange={(open) => {
            if (open) {
              setExpandedTaskId(null)
              setDraft({ ...EMPTY_DRAFT })
              return
            }
            if (adding) setDraft(null)
          }}
          items={heartbeatItems}
        />
      </AgentNestedSection>

      <AgentNestedSection
        title={t.config.agentSkillsTitle}
        description={t.config.agentSkillsDesc}
        error={errorFor('skills')}
        {...bindGuide('ai.skills', g.ai.skills)}
      >
        <ManagedList
          loading={loading}
          {...agentOptionsListWindow(filteredSkills.length)}
          emptyText={skills.length === 0 ? m.emptySkills : m.noneMatch}
          stats={skillStats}
          toolbar={[refreshAction]}
          {...queryChrome}
          search={
            skills.length > 0
              ? {
                  value: skillQuery,
                  onChange: setSkillQuery,
                  placeholder: m.searchSkills,
                  ariaLabel: m.searchSkills,
                }
              : undefined
          }
          filters={
            skills.length > 0
              ? {
                  value: skillFilter,
                  onChange: setSkillFilter,
                  ariaLabel: t.config.federationListQueryToggle,
                  options: [
                    {
                      key: 'all',
                      label: t.config.mcpFilterAll,
                      count: skills.length,
                    },
                    {
                      key: 'learned',
                      label: m.originLearned,
                      count: learnedCount,
                    },
                    {
                      key: 'manual',
                      label: m.originManual,
                      count: manualCount,
                    },
                    {
                      key: 'improved',
                      label: m.originImproved,
                      count: improvedCount,
                    },
                  ],
                }
              : undefined
          }
          items={skillItems}
        />
      </AgentNestedSection>

      <AgentNestedSection
        title={t.config.agentMemoryTitle}
        description={t.config.agentMemoryDesc}
        error={errorFor('memory')}
        {...bindGuide('ai.memory', g.ai.memory)}
      >
        <ManagedList
          loading={loading}
          {...agentOptionsListWindow(filteredMemories.length)}
          emptyText={memories.length === 0 ? m.emptyMemory : m.noneMatch}
          stats={memoryStats}
          toolbar={[refreshAction]}
          {...queryChrome}
          search={
            memories.length > 0
              ? {
                  value: memoryQuery,
                  onChange: setMemoryQuery,
                  placeholder: m.searchMemory,
                  ariaLabel: m.searchMemory,
                }
              : undefined
          }
          items={memoryItems}
        />
      </AgentNestedSection>
    </>
  )
}

export default AgentOptionsPanel
