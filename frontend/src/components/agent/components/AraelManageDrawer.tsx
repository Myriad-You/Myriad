/**
 * AraelManageDrawer - 管理面板（左侧栏布局）
 *
 * 左侧图标导航 + 右侧内容区：
 * - 定时任务 (Heartbeat) → AraelHeartbeatSection
 * - 技能 (Skills)
 * - 记忆 (Memory)
 */

import type {
  HeartbeatTask,
  MemoryEntry,
  SkillInfo,
} from '../../../services/agent'
import React, { useCallback, useEffect, useRef, useState } from 'react'
import { useAuth } from '../../../contexts/AuthContext'
import { useI18n } from '../../../contexts/I18nContext'
import { agentService } from '../../../services/agent'
import { isImeComposing } from '../../../utils/ime'
import { Spinner } from '../../Spinner'
import { AraelHeartbeatSection } from './AraelHeartbeatSection'
import { AraelPersonaSection } from './AraelPersonaSection'

type ManageTab = 'persona' | 'heartbeat' | 'skills' | 'memory'

export interface AraelManageDrawerProps {
  /** BE heartbeat/skill writes require admin; hide write UI for non-admin */
  isAdmin?: boolean
  /** Agent manage APIs are JWT-only — guests must not call them */
  isAuthenticated?: boolean
}

const TAB_KEYS: ManageTab[] = ['persona', 'heartbeat', 'skills', 'memory']

const TAB_ICONS: Record<ManageTab, React.ReactNode> = {
  persona: (
    <svg
      width="16"
      height="16"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      <path d="M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2" />
      <circle cx="12" cy="7" r="4" />
    </svg>
  ),
  heartbeat: (
    <svg
      width="16"
      height="16"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      <circle cx="12" cy="12" r="10" />
      <polyline points="12 6 12 12 16 14" />
    </svg>
  ),
  skills: (
    <svg
      width="16"
      height="16"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      <polygon points="12 2 15.09 8.26 22 9.27 17 14.14 18.18 21.02 12 17.77 5.82 21.02 7 14.14 2 9.27 8.91 8.26 12 2" />
    </svg>
  ),
  memory: (
    <svg
      width="16"
      height="16"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      <path d="M12 2a10 10 0 0 1 10 10c0 5.52-4.48 10-10 10S2 17.52 2 12" />
      <path d="M12 2C6.48 2 2 6.48 2 12" />
      <circle cx="12" cy="12" r="3" />
    </svg>
  ),
}

/** Inline edit component for memory content */
const MemoryEditInput: React.FC<{
  value: string
  onSave: (v: string) => void
  onCancel: () => void
}> = ({ value, onSave, onCancel }) => {
  const [text, setText] = useState(value)
  const inputRef = useRef<HTMLTextAreaElement>(null)

  useEffect(() => {
    inputRef.current?.focus()
    inputRef.current?.select()
  }, [])

  return (
    <div className="arael-mem-edit">
      <textarea
        ref={inputRef}
        className="arael-mem-edit-input"
        value={text}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Enter' && !e.shiftKey && !isImeComposing(e)) {
            e.preventDefault()
            if (text.trim()) onSave(text.trim())
          }
          if (e.key === 'Escape') onCancel()
        }}
        rows={2}
      />
      <div className="arael-mem-edit-actions">
        <button
          className="arael-mem-edit-save"
          onClick={() => text.trim() && onSave(text.trim())}
        >
          ✓
        </button>
        <button className="arael-mem-edit-cancel" onClick={onCancel}>
          ✕
        </button>
      </div>
    </div>
  )
}

export const AraelManageDrawer: React.FC<AraelManageDrawerProps> = ({
  isAdmin = false,
  isAuthenticated = false,
}) => {
  const { t: i18n } = useI18n()
  const { user } = useAuth()
  const isOwner = user?.is_owner === true
  const [tab, setTab] = useState<ManageTab>('persona')

  // Skill/heartbeat notifications pass tab via arael-open-manage detail
  useEffect(() => {
    const onOpen = (e: Event) => {
      const raw = (e as CustomEvent<{ tab?: string }>).detail?.tab
      if (
        raw === 'skills' ||
        raw === 'memory' ||
        raw === 'heartbeat' ||
        raw === 'persona'
      ) {
        setTab(raw)
      }
    }
    window.addEventListener('arael-open-manage', onOpen)
    return () => window.removeEventListener('arael-open-manage', onOpen)
  }, [])

  const tabLabels: Record<ManageTab, string> = {
    persona: i18n.arael.tabPersona,
    heartbeat: i18n.arael.tabHeartbeat,
    skills: i18n.arael.tabSkills,
    memory: i18n.arael.tabMemory,
  }
  const [heartbeatTasks, setHeartbeatTasks] = useState<HeartbeatTask[]>([])
  const [skills, setSkills] = useState<SkillInfo[]>([])
  const [memories, setMemories] = useState<MemoryEntry[]>([])
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [editingMemoryId, setEditingMemoryId] = useState<string | null>(null)

  const loadTabData = useCallback(
    async (currentTab: ManageTab) => {
      // All manage endpoints require JWT — skip for guests (no 401 spam)
      if (!isAuthenticated) {
        setHeartbeatTasks([])
        setSkills([])
        setMemories([])
        setError(i18n.arael.loginRequiredHint ?? i18n.arael.manageLoadError)
        setLoading(false)
        return
      }
      setLoading(true)
      setError(null)
      try {
        switch (currentTab) {
          case 'persona':
            setLoading(false)
            return
          case 'heartbeat': {
            // GET /api/agent/heartbeat is admin-only on BE — skip for non-admin
            // so the drawer does not spam 403 noise.
            if (!isAdmin) {
              setHeartbeatTasks([])
              setError(i18n.arael.manageAdminOnly)
              break
            }
            const tasks = await agentService.getHeartbeatTasks()
            setHeartbeatTasks(tasks)
            break
          }
          case 'skills': {
            const s = await agentService.getSkills()
            setSkills(s)
            break
          }
          case 'memory': {
            const m = await agentService.getMemories()
            setMemories(m)
            break
          }
        }
      } catch (e) {
        const msg =
          e instanceof Error ? e.message : i18n.arael.manageLoadError
        setError(msg)
      } finally {
        setLoading(false)
      }
    },
    [
      isAdmin,
      isAuthenticated,
      i18n.arael.manageLoadError,
      i18n.arael.manageAdminOnly,
      i18n.arael.loginRequiredHint,
    ],
  )

  useEffect(() => {
    loadTabData(tab)
  }, [tab, loadTabData])

  const handleDeleteMemory = useCallback(
    async (memoryId: string) => {
      const snapshot = memories
      setMemories((prev) => prev.filter((m) => m.id !== memoryId))
      try {
        await agentService.deleteMemory(memoryId)
      } catch (e) {
        setMemories(snapshot)
        setError(
          e instanceof Error ? e.message : i18n.arael.manageActionError,
        )
      }
    },
    [memories, i18n.arael.manageActionError],
  )

  const handleUpdateMemory = useCallback(
    async (memoryId: string, newContent: string) => {
      try {
        await agentService.updateMemory(memoryId, newContent)
        setEditingMemoryId(null)
        // Reload to get updated data (id may change)
        const m = await agentService.getMemories()
        setMemories(m)
      } catch (e) {
        setError(
          e instanceof Error ? e.message : i18n.arael.manageActionError,
        )
      }
    },
    [i18n.arael.manageActionError],
  )

  const handleDeleteSkill = useCallback(
    async (skillId: string) => {
      const snapshot = skills
      setSkills((prev) => prev.filter((s) => s.id !== skillId))
      try {
        await agentService.deleteSkill(skillId)
      } catch (e) {
        setSkills(snapshot)
        setError(
          e instanceof Error ? e.message : i18n.arael.manageActionError,
        )
      }
    },
    [skills, i18n.arael.manageActionError],
  )

  const memoryTypeLabel = (type: string) => {
    switch (type) {
      case 'preference':
        return i18n.arael.memPreference
      case 'fact':
        return i18n.arael.memFact
      case 'decision':
        return i18n.arael.memDecision
      case 'entity_knowledge':
        return i18n.arael.memKnowledge
      case 'execution_lesson':
        return i18n.arael.memLesson
      case 'effective_pattern':
        return i18n.arael.memPattern
      case 'session_insight':
        return i18n.arael.memInsight
      case 'session_summary':
        return i18n.arael.memSession
      default:
        return i18n.arael.memNote
    }
  }

  return (
    <div className="arael-manage">
      {/* Left sidebar */}
      <div className="arael-manage-sidebar">
        <div className="arael-manage-nav">
          {TAB_KEYS.map((key) => (
            <button
              key={key}
              className={`arael-manage-nav-item${tab === key ? ' active' : ''}`}
              onClick={() => setTab(key)}
              title={tabLabels[key]}
            >
              {TAB_ICONS[key]}
              <span className="arael-manage-nav-label">{tabLabels[key]}</span>
            </button>
          ))}
        </div>
      </div>

      {/* Right content */}
      <div className="arael-manage-main">
        <div className="arael-manage-main-content">
          {!isAdmin && tab !== 'persona' && (
            <div className="arael-manage-error" role="status">
              {i18n.arael.manageAdminOnly}
            </div>
          )}
          {loading && tab !== 'persona' && (
            <div className="arael-manage-loading">
              <Spinner size="xs" color="primary" />
            </div>
          )}

          {!loading && error && (
            <div className="arael-manage-error" role="alert">
              <span>{error}</span>
              <button
                type="button"
                className="arael-manage-error-retry"
                onClick={() => loadTabData(tab)}
              >
                {i18n.common.retry}
              </button>
            </div>
          )}

          {tab === 'persona' &&
            (isAuthenticated ? (
              <AraelPersonaSection isOwner={isOwner} />
            ) : (
              <div className="arael-manage-empty">
                {i18n.arael.statusNeedLogin}
              </div>
            ))}

          {/* Heartbeat */}
          {!loading && !error && tab === 'heartbeat' && (
            <AraelHeartbeatSection
              tasks={heartbeatTasks}
              isAdmin={isAdmin}
              onTasksChange={setHeartbeatTasks}
            />
          )}

          {/* Skills */}
          {!loading && !error && tab === 'skills' && (
            <div className="arael-manage-section">
              {skills.length === 0 ? (
                <div className="arael-manage-empty">
                  {i18n.arael.emptySkills}
                </div>
              ) : (
                skills.map((skill) => (
                  <div key={skill.id} className="arael-skill-item">
                    <div className="arael-skill-header">
                      <span className="arael-skill-name">{skill.name}</span>
                      <div className="arael-skill-header-right">
                        <span
                          className={`arael-skill-origin arael-skill-origin-${skill.origin}`}
                        >
                          {skill.origin === 'manual'
                            ? i18n.arael.originManual
                            : skill.origin === 'agent_generated'
                              ? i18n.arael.originAuto
                              : i18n.arael.originImproved}
                        </span>
                        {isAdmin && skill.origin !== 'manual' && (
                          <button
                            className="arael-manage-delete-btn"
                            onClick={() => handleDeleteSkill(skill.id)}
                            title={i18n.arael.deleteSkill}
                          >
                            <svg
                              width="12"
                              height="12"
                              viewBox="0 0 24 24"
                              fill="none"
                              stroke="currentColor"
                              strokeWidth="2"
                              strokeLinecap="round"
                              strokeLinejoin="round"
                            >
                              <polyline points="3 6 5 6 21 6" />
                              <path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2" />
                            </svg>
                          </button>
                        )}
                      </div>
                    </div>
                    <div className="arael-skill-desc">{skill.description}</div>
                    <div className="arael-skill-stats">
                      <span className="arael-skill-stat-ok">
                        {skill.successCount ?? 0} {i18n.arael.statSuccess}
                      </span>
                      <span className="arael-skill-stat-fail">
                        {skill.failureCount ?? 0} {i18n.arael.statFail}
                      </span>
                      {skill.tierHint && (
                        <span
                          className={`arael-tier-badge arael-tier-${skill.tierHint}`}
                        >
                          {skill.tierHint === 'pro' ? 'Pro' : 'Std'}
                        </span>
                      )}
                    </div>
                  </div>
                ))
              )}
            </div>
          )}

          {/* Memory */}
          {!loading && !error && tab === 'memory' && (
            <div className="arael-manage-section">
              {memories.length === 0 ? (
                <div className="arael-manage-empty">
                  {i18n.arael.emptyMemory}
                </div>
              ) : (
                memories.map((mem) => (
                  <div key={mem.id} className="arael-mem-item">
                    <div className="arael-mem-header">
                      <span className="arael-mem-type">
                        {memoryTypeLabel(mem.memoryType)}
                      </span>
                      <div className="arael-mem-actions">
                        <button
                          className="arael-manage-action-btn"
                          onClick={() =>
                            setEditingMemoryId(
                              editingMemoryId === mem.id ? null : mem.id,
                            )
                          }
                          title={i18n.arael.editMemory}
                        >
                          <svg
                            width="11"
                            height="11"
                            viewBox="0 0 24 24"
                            fill="none"
                            stroke="currentColor"
                            strokeWidth="2"
                            strokeLinecap="round"
                            strokeLinejoin="round"
                          >
                            <path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7" />
                            <path d="M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4 9.5-9.5z" />
                          </svg>
                        </button>
                        <button
                          className="arael-manage-delete-btn"
                          onClick={() => handleDeleteMemory(mem.id)}
                          title={i18n.arael.deleteMemory}
                        >
                          <svg
                            width="11"
                            height="11"
                            viewBox="0 0 24 24"
                            fill="none"
                            stroke="currentColor"
                            strokeWidth="2"
                            strokeLinecap="round"
                            strokeLinejoin="round"
                          >
                            <polyline points="3 6 5 6 21 6" />
                            <path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2" />
                          </svg>
                        </button>
                      </div>
                    </div>
                    {editingMemoryId === mem.id ? (
                      <MemoryEditInput
                        value={mem.content}
                        onSave={(v) => handleUpdateMemory(mem.id, v)}
                        onCancel={() => setEditingMemoryId(null)}
                      />
                    ) : (
                      <span className="arael-mem-content">{mem.content}</span>
                    )}
                    {mem.importance != null && (
                      <div className="arael-mem-meta">
                        <span className="arael-mem-importance">
                          {'★'.repeat(Math.round(mem.importance * 5))}
                        </span>
                        {mem.tier && (
                          <span className="arael-mem-tier">
                            {mem.tier === 'long_term'
                              ? i18n.arael.memTierLong
                              : mem.tier === 'medium_term'
                                ? i18n.arael.memTierMid
                                : i18n.arael.memTierShort}
                          </span>
                        )}
                      </div>
                    )}
                  </div>
                ))
              )}
            </div>
          )}
        </div>
      </div>
    </div>
  )
}

export default AraelManageDrawer
