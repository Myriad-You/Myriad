export interface ReattachMessageRef {
  id: string
  role: string
  taskId?: string
  runId?: string
}

export interface ReattachCandidate {
  messageId: string
  taskId?: string
  runId?: string
}

export interface ReattachHints {
  runId?: string
  taskId?: string
}

export function collectReattachCandidates(
  messages: ReattachMessageRef[],
  hints?: ReattachHints,
): ReattachCandidate[] {
  const byTask = new Map<string, ReattachCandidate>()
  const byRun = new Map<string, ReattachCandidate>()

  for (const m of messages) {
    if (m.role !== 'assistant') continue
    const taskId =
      m.taskId && !m.taskId.startsWith('confirmation:') ? m.taskId : undefined
    const runId = m.runId || undefined

    if (taskId) {
      const prev = byTask.get(taskId)
      byTask.set(taskId, {
        messageId: m.id,
        taskId,
        runId: runId || prev?.runId,
      })
    }
    if (runId) {
      const prev = byRun.get(runId)
      byRun.set(runId, {
        messageId: m.id,
        runId,
        taskId: taskId || prev?.taskId,
      })
      if (taskId) {
        const t = byTask.get(taskId)
        if (t && !t.runId) t.runId = runId
      }
    }
  }

  for (const m of messages) {
    if (!m.taskId || !m.runId) continue
    if (m.taskId.startsWith('confirmation:')) continue
    const t = byTask.get(m.taskId)
    if (t && !t.runId) t.runId = m.runId
  }

  const out: ReattachCandidate[] = []
  const pushUnique = (c: ReattachCandidate, preferMessageId = false) => {
    if (!c.messageId && !c.runId && !c.taskId) return
    const existing = out.find(
      (x) =>
        (c.taskId && x.taskId === c.taskId) ||
        (c.runId && x.runId === c.runId),
    )
    if (existing) {
      // Do not clobber a newer host messageId.
      if (!existing.runId && c.runId) existing.runId = c.runId
      if (!existing.taskId && c.taskId) existing.taskId = c.taskId
      if (preferMessageId && c.messageId) existing.messageId = c.messageId
      return
    }
    out.push({ ...c })
  }

  const lastAssistant =
    messages.findLast((m) => m.role === 'assistant')?.id ?? ''

  if (hints?.taskId) {
    const fromTask = byTask.get(hints.taskId)
    const fromRun = hints.runId ? byRun.get(hints.runId) : undefined
    pushUnique(
      {
        messageId:
          fromTask?.messageId ||
          fromRun?.messageId ||
          lastAssistant ||
          `hint_task_${hints.taskId}`,
        taskId: hints.taskId,
        runId: hints.runId || fromTask?.runId || fromRun?.runId,
      },
      true,
    )
  } else if (hints?.runId) {
    // Early task_progress may omit taskId.
    const fromRun = byRun.get(hints.runId)
    pushUnique(
      {
        messageId:
          fromRun?.messageId || lastAssistant || `hint_run_${hints.runId}`,
        taskId: fromRun?.taskId,
        runId: hints.runId,
      },
      true,
    )
  }

  for (const c of byTask.values()) {
    pushUnique(c)
    if (out.length >= 5) break
  }

  if (out.length < 5) {
    for (const c of byRun.values()) {
      pushUnique(c)
      if (out.length >= 5) break
    }
  }

  return out
}

export function isNonTerminalTaskStatus(status: string): boolean {
  return (
    status === 'pending' ||
    status === 'running' ||
    status === 'waiting_for_input' ||
    status === 'paused'
  )
}
