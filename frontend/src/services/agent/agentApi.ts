import type {
  AgentResponse,
  Capability,
  CreatePresetRequest,
  ExecutionTrace,
  HeartbeatTask,
  ManagedMemory,
  MeropeDoingResponse,
  MeropeHerResponse,
  MindSnapshot,
  MoodTransition,
  PerformanceDirective,
  ProcessContext,
  ProcessRequest,
  ProgressCallback,
  QueueStatus,
  RigStateSummary,
  SessionInfo,
  SessionMessage,
  SkillInfo,
  TaskDetail,
  TaskInfo,
  TaskPreset,
  TaskPresetListResponse,
} from './types'

import { currentCopy } from '../../i18n/localeCopy'
import { authSubject } from '../../utils/authSubject'
import { userFacingError } from '../../utils/userFacingError'
import { ApiError, apiService } from '../api'
import { abortSseSubscriptions, executeSSERequest } from './sseTransport'

/** Keep in sync with MEROPE_PROXY_TIMEOUT_MS. */
const PERSONA_GENERATION_TIMEOUT_MS = 15 * 60 * 1000
/** Must exceed backend NAME_CALL_TIMEOUT. */
const NAME_SUGGEST_TIMEOUT_MS = 6 * 60 * 1000

const personaGenerationInflight = new Map<string, Promise<unknown>>()

export type AgentIntentionStatus =
  | 'proposed'
  | 'accepted'
  | 'running'
  | 'waiting'
  | 'completed'
  | 'failed'
  | 'abandoned'
  | 'expired'

/** A device that syncs her with this site (`/api/agent/devices`). */
export interface SyncDevice {
  id: string
  name: string
  /** `mys_` and the first 8 characters, to tell tokens apart. */
  tokenHint: string
  createdAt: string
  lastSeenAt: string | null
  revokedAt: string | null
  revokedReason: 'owner_revoked' | 'owner_changed' | 'device_left' | null
  /** Live and not seen for 30 days. Never removed for it. */
  stale: boolean
  /** Its link to her is open right now. */
  connected?: boolean
}

/** A move of her between ends, in progress (her host lease). */
export interface HostMove {
  id: string
  /**
   * `move`: she goes to live on `to` (from `site`), or back to the site from
   * the device she lives on (`from`); `merge`: the her on `from` joins the
   * site's; `recover`: she is taken back onto the site from a daily backup
   * of `from` by another device (`via`).
   */
  kind: 'move' | 'merge' | 'recover'
  /** `site` or a device id; the names are the devices' as the owner gave them. */
  from: string
  fromName: string | null
  to: string
  toName: string | null
  phase:
    'proposed' | 'frozen' | 'transferring' | 'applied' | 'committed' | 'retired'
  /** 1–6, as both ends show it. */
  step: number
  /** Who asked: the owner here (confirmed already) or the device (waits for the owner). */
  by: 'owner' | 'device'
  startedAt: string
  confirmedAt: string | null
  /** A move to a device: the device agreed (the owner said yes there). */
  agreed?: boolean | null
  /** A reclaim: the device sending her. */
  via?: string | null
  viaName?: string | null
  /** The import that carries her back. */
  import?: string | null
}

/** Where she lives (`agent_host_lease`): one end, her lineage and how many times she moved. */
export interface HostLease {
  /** `site` or the id of the device she lives on. */
  holder: string
  holderName: string | null
  lineage: string
  /** Goes up by one each time she changes host; never down. */
  epoch: number
  /**
   * `frozen`: a move is under way and no end has her; `vacant`: she is not
   * here and the site does not know where; `moved_out`: exported to be
   * brought in elsewhere, stopped here until let go or undone.
   */
  state: 'live' | 'frozen' | 'vacant' | 'moved_out'
  move: HostMove | null
  updatedAt: string
}

/** Her on her way into this site, which has none (H3c): from a device, or a file. */
export interface HostImport {
  id: string
  source: 'device' | 'file'
  deviceId: string | null
  deviceName: string | null
  /** Her name as the device says it. */
  name: string | null
  lineage: string | null
  epoch: number | null
  /** `proposed` waits for the owner here; a file is `uploading` until it is opened. */
  phase: 'proposed' | 'confirmed' | 'transferring' | 'uploading' | 'committed'
  /** Entities by kind, as the device stated them. */
  counts: Record<string, number>
  embeddings: number | null
  files: number | null
  received: number
  total: number | null
  createdAt: string
  confirmedAt: string | null
  /**
   * `merge`: a device's her into the site's, the first time they meet (H3d);
   * `return`: back from the device she lives on; `recover`: taken back from
   * a daily backup by another device (H3e).
   */
  kind?: 'import' | 'merge' | 'return' | 'recover'
  /** Who she is when her names differ, as chosen on the device. */
  subject?: 'site' | 'device' | null
  /** The file the site sealed of its her when the owner confirmed a merge. */
  retired?: string | null
  /** What the owner is shown before confirming a merge. */
  merge?: HostMergePreview | null
}

/** A merge as the owner sees it before confirming (H3d). */
export interface HostMergePreview {
  sameName: boolean
  sameLineage: boolean
  names: { site: string | null; device: string | null }
  subject: 'site' | 'device' | null
  fields: {
    field: string
    from: 'host' | 'other' | null
    both: boolean
    toHistory: 'host' | 'other' | null
  }[]
  renamed: { kind: string; side: 'host' | 'other'; from: string; to: string }[]
  site: { memories: number; diary: number; affectEvents: number }
  unreadable?: boolean
}

/** A copy the site sealed of its her before a merge. */
export interface HostRetired {
  name: string
  bytes: number
  at: string | null
}

/** What landed when she came in. */
export interface HostImportLanded {
  host: HostLease | null
  counts: Record<string, number>
  vectors: number
  /** Her switch on the site, which an import does not touch. */
  herOn: boolean
}

export interface SyncDeviceList {
  devices: SyncDevice[]
  /** Null while the site has no her. */
  host: HostLease | null
  /** Her on her way in, if any. */
  import?: HostImport | null
  /** The site has no her and could take her in from a file. */
  empty?: boolean
  /** Messages that came by chat app while she was away, held until she is back. */
  held?: number
  /** What the site sealed of its her before a merge or a move. */
  retired?: HostRetired[]
  /** Her daily backups the site keeps and cannot open (D4). */
  backups?: HostBackup[]
  /** While she is away: how much is left here of others (sealed, Q2). */
  sealed?: number | null
  /** Her living on a device is open here (only the acceptance entry before H4, Q8). */
  deviceHostOpen?: boolean
  /** The site's last check of its own clock; null while off or not run yet. */
  serverClock?: ServerClock | null
  limit: number
}

/** How far the site's clock is off, as it last measured (`serverClock`). */
export interface ServerClock {
  /** Milliseconds since the epoch, by the site's clock. */
  checkedAt: number
  /** The host's clock minus real time; null when nothing answered. */
  offsetMs: number | null
  source: 'sntp' | 'https' | null
  /** An HTTPS Date, or SNTP servers that agree. */
  trusted: boolean
  thresholdMs: number
  /** The largest offset the site corrects. */
  maxCorrectionMs: number
  /** The host's clock is off by more than thresholdMs. */
  wrong: boolean
  /** The site's clock is real time by this measurement. */
  corrected: boolean
}

/** One daily backup a device she lives on pushed. */
export interface HostBackup {
  id: string
  deviceId: string
  deviceName: string | null
  day: string
  epoch: number
  bytes: number
  at: string
}

export interface AgentWorkProposal {
  title: string
  instruction: string
  expected_outcome: string
  source_event_id: string
  /** Where accepting runs it: the site (absent), or the owner's Mac. */
  place?: 'site' | 'mac'
  device_name?: string
}

export interface AgentIntention {
  id: string
  summary: string
  reason_code: string
  status: AgentIntentionStatus
  proposal: AgentWorkProposal
  created_at: string
  updated_at: string
  expires_at?: string
}

export interface QqPairingStatus {
  paired: boolean
  identityId: number | null
  openidMasked: string | null
  linkedAt: string | null
  pendingCode: string | null
  pendingExpiresAt: string | null
}

export type QqBotPhase =
  'offline' | 'connecting' | 'online' | 'rejected' | 'reconnecting'

export interface QqBotStatus {
  phase: QqBotPhase
  enabled: boolean
  hasAppId: boolean
  hasSecret: boolean
  appId?: string | null
  lastInboundAt?: string | null
}

export type TelegramBotPhase = QqBotPhase

export interface TelegramBotStatus {
  phase: TelegramBotPhase
  enabled: boolean
  hasToken: boolean
  botUsername?: string | null
  botName?: string | null
  lastInboundAt?: string | null
}

export type TelegramPairingStatus = QqPairingStatus

export type DiscordBotPhase = QqBotPhase

export interface DiscordBotStatus {
  phase: DiscordBotPhase
  enabled: boolean
  hasToken: boolean
  botUsername?: string | null
  botName?: string | null
  botUserId?: string | null
  lastInboundAt?: string | null
}

export type DiscordPairingStatus = QqPairingStatus

export type FeishuBotPhase = QqBotPhase

export interface FeishuBotStatus {
  phase: FeishuBotPhase
  enabled: boolean
  hasAppId: boolean
  hasSecret: boolean
  appId?: string | null
  lastInboundAt?: string | null
}

export type FeishuPairingStatus = QqPairingStatus

export type OneBotPhase = QqBotPhase

export interface OneBotStatus {
  phase: OneBotPhase
  enabled: boolean
  hasUrl: boolean
  hasToken: boolean
  wsUrl?: string | null
  lastInboundAt?: string | null
  /** SnowLuma's last heartbeat: whether the QQ account itself is logged in. */
  accountOnline?: boolean | null
}

export type OneBotPairingStatus = QqPairingStatus

function sharePersonaGeneration<T>(
  key: string,
  start: () => Promise<T>,
): Promise<T> {
  const existing = personaGenerationInflight.get(key)
  if (existing) return existing as Promise<T>
  const promise = start().finally(() => {
    if (personaGenerationInflight.get(key) === promise) {
      personaGenerationInflight.delete(key)
    }
  })
  personaGenerationInflight.set(key, promise)
  return promise
}

export interface McpServerConfig {
  id: string
  transport?: 'stdio' | 'gateway'
  command: string
  args: string[]
  env: Record<string, string>
  enabled: boolean
  auto_restart: boolean
  max_restart_attempts: number
  trust_annotations?: boolean
}

export interface McpRuntimeServer {
  id: string
  healthy: boolean
  tool_count: number
  auto_restart: boolean
}

export interface McpConfigSnapshot {
  servers: McpServerConfig[]
  configPath: string
  runtimeServers: McpRuntimeServer[]
  toolCount: number
  runtimePolicy?: { localStdioAllowed: boolean; gatewayConfigured: boolean }
}

/** A value a persona field held until an edit replaced it. */
export interface PersonaFieldHistoryEntry {
  id: number
  field: string
  value: unknown
  rev: string
  /** `site`, or the device that wrote it. */
  origin: string
  replacedAt: string
  reason: string
}

export interface PublicPersonaProfile {
  summary: string
  temperament: string[]
  likes: string[]
  drives: string[]
  socialStyle: string
  speechStyle: string
}

export interface PublicPersona {
  name: string
  profile: PublicPersonaProfile | null
}

export interface AgentPersona {
  name: string
  portraitAssetId: string | null
  avatarAssetId: string | null
  hasCustomPersona: boolean
  personality?: string
  persona?: Record<string, unknown> | null
  visualProfile?: Record<string, unknown> | null
  portraitGeneration?: Record<string, unknown> | null
  /** The sticker avatar's generation record; `pending` while one runs. Owner only. */
  avatarGeneration?: Record<string, unknown> | null
  /** Her zone, an IANA name; empty is the site's own. Owner only. */
  zone?: string
  mood?: number
  moodRevision?: number
  arousal?: number
  activity?: string
  doNotDisturb?: boolean
  doNotDisturbActive?: boolean
  dndStart?: string | null
  dndEnd?: string | null
  reportCount?: number
}

function parseMcpRuntimeServers(
  raw: Array<Record<string, unknown>> | undefined,
): McpRuntimeServer[] {
  return (Array.isArray(raw) ? raw : [])
    .map((row) => {
      const id = typeof row.id === 'string' ? row.id.trim() : ''
      if (!id) return null
      return {
        id,
        healthy: row.healthy === true,
        tool_count:
          typeof row.tool_count === 'number' && Number.isFinite(row.tool_count)
            ? Math.max(0, Math.floor(row.tool_count))
            : 0,
        auto_restart: row.auto_restart === true,
      }
    })
    .filter((s): s is McpRuntimeServer => s != null)
}

export function parseMcpServerConfig(raw: unknown): McpServerConfig | null {
  if (!raw || typeof raw !== 'object') return null
  const o = raw as Record<string, unknown>
  const id = typeof o.id === 'string' ? o.id.trim() : ''
  const command = typeof o.command === 'string' ? o.command.trim() : ''
  if (
    o.transport != null &&
    o.transport !== 'stdio' &&
    o.transport !== 'gateway'
  ) {
    return null
  }
  const transport = o.transport === 'gateway' ? 'gateway' : 'stdio'
  if (!id || (transport === 'stdio' && !command)) return null
  const args = Array.isArray(o.args)
    ? o.args.filter((a): a is string => typeof a === 'string')
    : []
  const env: Record<string, string> = {}
  if (o.env && typeof o.env === 'object' && !Array.isArray(o.env)) {
    for (const [k, v] of Object.entries(o.env as Record<string, unknown>)) {
      if (typeof k === 'string' && typeof v === 'string') env[k] = v
    }
  }
  return {
    id,
    transport,
    command,
    args,
    env,
    enabled: o.enabled === true,
    auto_restart: o.auto_restart !== false,
    max_restart_attempts:
      typeof o.max_restart_attempts === 'number' &&
      Number.isFinite(o.max_restart_attempts)
        ? Math.max(0, Math.min(50, Math.floor(o.max_restart_attempts)))
        : 3,
    // Must parse; defaulting false revokes tool-annotation opt-in on save.
    trust_annotations: o.trust_annotations === true,
  }
}

function normalizeMcpConfigSnapshot(response: {
  config?: { servers?: unknown }
  config_path?: string
  runtime_policy?: {
    local_stdio_allowed?: boolean
    gateway_configured?: boolean
  }
  runtime?: {
    servers?: Array<Record<string, unknown>>
    tool_count?: number
  }
}): McpConfigSnapshot {
  const rawServers = response.config?.servers
  const servers = (Array.isArray(rawServers) ? rawServers : [])
    .map(parseMcpServerConfig)
    .filter((s): s is McpServerConfig => s != null)
  return {
    servers,
    runtimePolicy: {
      localStdioAllowed: response.runtime_policy?.local_stdio_allowed === true,
      gatewayConfigured: response.runtime_policy?.gateway_configured === true,
    },
    configPath:
      typeof response.config_path === 'string' ? response.config_path : '',
    runtimeServers: parseMcpRuntimeServers(response.runtime?.servers),
    toolCount:
      typeof response.runtime?.tool_count === 'number'
        ? response.runtime.tool_count
        : 0,
  }
}

/** IntentAction is a snake_case string; coerce objects. */
export function normalizeCapabilityActions(raw: unknown): string[] {
  if (!Array.isArray(raw)) return []
  const out: string[] = []
  for (const item of raw) {
    if (typeof item === 'string' && item.trim()) {
      out.push(item.trim())
      continue
    }
    if (item && typeof item === 'object') {
      const entries = Object.entries(item as Record<string, unknown>)
      if (entries.length === 1) {
        const [k, v] = entries[0]
        out.push(typeof v === 'string' && v ? `${k}:${v}` : k)
        continue
      }
    }
  }
  return out
}

class AgentService {
  private baseUrl = '/agent'

  /** Chat abort must not drop an in-flight Work run. */
  private activeAbortControllersByMode: Record<
    'work' | 'chat',
    Set<AbortController>
  > = {
    work: new Set<AbortController>(),
    chat: new Set<AbortController>(),
  }

  /** Rejects; does not re-subscribe the same run. */
  abortCurrentRequest(mode?: 'work' | 'chat'): void {
    const lanes: Array<'work' | 'chat'> = mode ? [mode] : ['work', 'chat']
    for (const lane of lanes) {
      abortSseSubscriptions(this.activeAbortControllersByMode[lane], 'user')
    }
  }

  async listIntentions(): Promise<AgentIntention[]> {
    const response = await apiService.get<{ intentions: AgentIntention[] }>(
      `${this.baseUrl}/intentions`,
    )
    return response.intentions
  }

  /** A proposal for the owner's Mac is handed to that Mac (`act`), not run here. */
  async acceptIntention(intentionId: string): Promise<{
    intention: AgentIntention
    work?: { mode: 'work'; input: string }
    act?: { actId: string; deviceName?: string }
  }> {
    return apiService.post(
      `${this.baseUrl}/intentions/${encodeURIComponent(intentionId)}/accept`,
    )
  }

  async dismissIntention(
    intentionId: string,
  ): Promise<{ intention: AgentIntention }> {
    return apiService.post(
      `${this.baseUrl}/intentions/${encodeURIComponent(intentionId)}/dismiss`,
    )
  }

  async getAutonomyGrant(): Promise<{
    grant: {
      userId: number
      allowedPermissions: string[]
      revoked: boolean
    } | null
  }> {
    return apiService.get(`${this.baseUrl}/autonomy`)
  }

  async putAutonomyGrant(allowedPermissions: string[] = []): Promise<{
    grant: { userId: number; allowedPermissions: string[]; revoked: boolean }
  }> {
    return apiService.put(`${this.baseUrl}/autonomy`, { allowedPermissions })
  }

  async revokeAutonomyGrant(): Promise<{
    grant: { userId: number; allowedPermissions: string[]; revoked: boolean }
  }> {
    return apiService.delete(`${this.baseUrl}/autonomy`)
  }

  async getQqPairing(): Promise<{ pairing: QqPairingStatus }> {
    return apiService.get(`${this.baseUrl}/qq/pairing`)
  }

  async issueQqPairingCode(): Promise<{
    code: string
    expiresAt: string
    pairing: QqPairingStatus
  }> {
    return apiService.post(`${this.baseUrl}/qq/pairing`)
  }

  async listSyncDevices(): Promise<SyncDeviceList> {
    return apiService.get(`${this.baseUrl}/devices`)
  }

  /** The token comes back this once only. */
  async addSyncDevice(
    name: string,
  ): Promise<{ device: SyncDevice; token: string }> {
    return apiService.post(`${this.baseUrl}/devices`, { name })
  }

  async removeSyncDevice(id: string): Promise<{ device: SyncDevice }> {
    return apiService.delete(
      `${this.baseUrl}/devices/${encodeURIComponent(id)}`,
    )
  }

  /** Forgets a device already removed: it leaves the list. */
  async forgetSyncDevice(id: string): Promise<SyncDeviceList> {
    return apiService.delete(
      `${this.baseUrl}/devices/${encodeURIComponent(id)}/record`,
    )
  }

  /**
   * Confirms, logged in, a move of her a device asked for: she is frozen
   * until it is done or called off. A move to a device takes the passphrase
   * the site seals the copy it keeps with (used once, kept nowhere).
   */
  async confirmHostMove(
    id: string,
    passphrase?: string,
  ): Promise<{ host: HostLease | null }> {
    return apiService.post(
      `${this.baseUrl}/host/moves/${encodeURIComponent(id)}/confirm`,
      passphrase === undefined ? undefined : { passphrase },
      { timeout: 600000 },
    )
  }

  /**
   * Starts a move of her with a device: `move` (选主机: she goes to live on
   * it; the device agrees, then the owner confirms) or `back` (she comes
   * back from the device she lives on: frozen there at once).
   */
  async startHostMove(
    kind: 'move' | 'back',
    device: string,
  ): Promise<{ host: HostLease | null }> {
    return apiService.post(`${this.baseUrl}/host/moves`, { kind, device })
  }

  /** 「导出并搬走」: her as one sealed file; she moves out of the site. */
  async moveHerOut(passphrase: string): Promise<Blob> {
    return apiService.request<Blob>(`${this.baseUrl}/host/move-out`, {
      method: 'POST',
      body: JSON.stringify({ passphrase }),
      responseType: 'blob',
      timeout: 300000,
    })
  }

  /** Undoes her moving out: she lives here again, as she was. */
  async undoMoveOut(): Promise<SyncDeviceList> {
    return apiService.post(`${this.baseUrl}/host/move-out/undo`)
  }

  /** She moved out and is in her new place: the site lets its copy go. */
  async letHerGo(): Promise<SyncDeviceList> {
    return apiService.post(`${this.baseUrl}/host/move-out/let-go`)
  }

  /** Lets go of what is left here of others while she is away. */
  async clearSealed(): Promise<SyncDeviceList> {
    return apiService.delete(`${this.baseUrl}/host/sealed`)
  }

  /** One daily backup, as it was pushed (only the app that made it opens it). */
  async downloadBackup(id: string): Promise<Blob> {
    return apiService.request<Blob>(
      `${this.baseUrl}/host/backups/${encodeURIComponent(id)}`,
      { method: 'GET', responseType: 'blob', timeout: 300000 },
    )
  }

  async deleteBackup(id: string): Promise<SyncDeviceList> {
    return apiService.delete(
      `${this.baseUrl}/host/backups/${encodeURIComponent(id)}`,
    )
  }

  /** Where she lives (owner only). */
  async getHost(): Promise<{ host: HostLease | null }> {
    return apiService.get(`${this.baseUrl}/host`)
  }

  /** Calls a move off before its commit (or turns down one a device asked for): she is back as she was. */
  async cancelHostMove(id: string): Promise<{ host: HostLease | null }> {
    return apiService.post(
      `${this.baseUrl}/host/moves/${encodeURIComponent(id)}/cancel`,
    )
  }

  /** Confirms, logged in, that the her on a device comes to live here (the site has none). */
  async confirmHostImport(
    id: string,
    passphrase?: string,
    replace?: boolean,
  ): Promise<SyncDeviceList> {
    const body =
      passphrase === undefined && !replace
        ? undefined
        : {
            ...(passphrase === undefined ? {} : { passphrase }),
            ...(replace ? { replace: true } : {}),
          }
    return apiService.post(
      `${this.baseUrl}/host/imports/${encodeURIComponent(id)}/confirm`,
      body,
      { timeout: 300000 },
    )
  }

  /** One copy the site sealed of its her before a merge, as it was written. */
  async downloadRetired(name: string): Promise<Blob> {
    return apiService.request<Blob>(
      `${this.baseUrl}/host/retired/${encodeURIComponent(name)}`,
      { method: 'GET', responseType: 'blob', timeout: 300000 },
    )
  }

  /** Lets one such copy go. */
  async deleteRetired(name: string): Promise<SyncDeviceList> {
    return apiService.delete(
      `${this.baseUrl}/host/retired/${encodeURIComponent(name)}`,
    )
  }

  /** Turns down or calls off an import: the site is as it was. */
  async cancelHostImport(id: string): Promise<SyncDeviceList> {
    return apiService.post(
      `${this.baseUrl}/host/imports/${encodeURIComponent(id)}/cancel`,
    )
  }

  /** Starts bringing her in from an export file of `total` bytes. */
  async beginFileImport(
    total: number,
  ): Promise<{ import: HostImport; pieceBytes: number }> {
    return apiService.post(`${this.baseUrl}/host/imports/file`, { total })
  }

  /** One piece of the file, at where the upload stands. */
  async sendFileImportPiece(
    id: string,
    offset: number,
    piece: Blob,
  ): Promise<{ import: HostImport }> {
    return apiService.request(
      `${this.baseUrl}/host/imports/${encodeURIComponent(id)}/file`,
      {
        method: 'PUT',
        body: piece,
        params: { offset },
        timeout: 300000,
      },
    )
  }

  /**
   * Opens the whole file with the passphrase and brings her in. The site
   * uses the passphrase once, for the key, and keeps it nowhere.
   */
  async openFileImport(
    id: string,
    passphrase: string,
    replace?: boolean,
  ): Promise<HostImportLanded> {
    return apiService.request(
      `${this.baseUrl}/host/imports/${encodeURIComponent(id)}/open`,
      {
        method: 'POST',
        body: JSON.stringify(replace ? { passphrase, replace: true } : { passphrase }),
        timeout: 600000,
      },
    )
  }

  /** Lets go of what came by chat app while she was away: she will not read it. */
  async clearHeld(): Promise<{ cleared: number; held: number }> {
    return apiService.delete(`${this.baseUrl}/host/held`)
  }

  /**
   * 「导出她」: everything the site has of her as one file, sealed with this
   * passphrase. The site uses it once for the key and keeps it nowhere.
   */
  async exportHer(passphrase: string): Promise<Blob> {
    return apiService.request<Blob>(`${this.baseUrl}/host/export`, {
      method: 'POST',
      body: JSON.stringify({ passphrase }),
      responseType: 'blob',
      timeout: 300000,
    })
  }

  async unpairQq(): Promise<{ success: boolean }> {
    return apiService.delete(`${this.baseUrl}/qq/pairing`)
  }

  async getQqBotStatus(): Promise<QqBotStatus> {
    return apiService.get(`${this.baseUrl}/qq/status`)
  }

  async testQqBot(): Promise<{ success: boolean; phase?: QqBotPhase }> {
    return apiService.post(`${this.baseUrl}/qq/test`)
  }

  async getTelegramPairing(): Promise<{ pairing: TelegramPairingStatus }> {
    return apiService.get(`${this.baseUrl}/telegram/pairing`)
  }

  async issueTelegramPairingCode(): Promise<{
    code: string
    expiresAt: string
    pairing: TelegramPairingStatus
  }> {
    return apiService.post(`${this.baseUrl}/telegram/pairing`)
  }

  async unpairTelegram(): Promise<{ success: boolean }> {
    return apiService.delete(`${this.baseUrl}/telegram/pairing`)
  }

  async getTelegramBotStatus(): Promise<TelegramBotStatus> {
    return apiService.get(`${this.baseUrl}/telegram/status`)
  }

  async testTelegramBot(): Promise<{
    success: boolean
    phase?: TelegramBotPhase
    botUsername?: string | null
    botName?: string | null
  }> {
    return apiService.post(`${this.baseUrl}/telegram/test`)
  }

  async getDiscordPairing(): Promise<{ pairing: DiscordPairingStatus }> {
    return apiService.get(`${this.baseUrl}/discord/pairing`)
  }

  async issueDiscordPairingCode(): Promise<{
    code: string
    expiresAt: string
    pairing: DiscordPairingStatus
  }> {
    return apiService.post(`${this.baseUrl}/discord/pairing`)
  }

  async unpairDiscord(): Promise<{ success: boolean }> {
    return apiService.delete(`${this.baseUrl}/discord/pairing`)
  }

  async getDiscordBotStatus(): Promise<DiscordBotStatus> {
    return apiService.get(`${this.baseUrl}/discord/status`)
  }

  async testDiscordBot(): Promise<{
    success: boolean
    phase?: DiscordBotPhase
    botUsername?: string | null
    botName?: string | null
    botUserId?: string | null
  }> {
    return apiService.post(`${this.baseUrl}/discord/test`)
  }

  async getFeishuPairing(): Promise<{ pairing: FeishuPairingStatus }> {
    return apiService.get(`${this.baseUrl}/feishu/pairing`)
  }

  async issueFeishuPairingCode(): Promise<{
    code: string
    expiresAt: string
    pairing: FeishuPairingStatus
  }> {
    return apiService.post(`${this.baseUrl}/feishu/pairing`)
  }

  async unpairFeishu(): Promise<{ success: boolean }> {
    return apiService.delete(`${this.baseUrl}/feishu/pairing`)
  }

  async getFeishuBotStatus(): Promise<FeishuBotStatus> {
    return apiService.get(`${this.baseUrl}/feishu/status`)
  }

  async testFeishuBot(): Promise<{ success: boolean; phase?: FeishuBotPhase }> {
    return apiService.post(`${this.baseUrl}/feishu/test`)
  }

  async getOneBotPairing(): Promise<{ pairing: OneBotPairingStatus }> {
    return apiService.get(`${this.baseUrl}/onebot/pairing`)
  }

  async issueOneBotPairingCode(): Promise<{
    code: string
    expiresAt: string
    pairing: OneBotPairingStatus
  }> {
    return apiService.post(`${this.baseUrl}/onebot/pairing`)
  }

  async unpairOneBot(): Promise<{ success: boolean }> {
    return apiService.delete(`${this.baseUrl}/onebot/pairing`)
  }

  async getOneBotStatus(): Promise<OneBotStatus> {
    return apiService.get(`${this.baseUrl}/onebot/status`)
  }

  /**
   * 重新订阅一个已存在的后端 run（页面刷新 / 通知打开后恢复进度）。
   * 不会创建新任务。
   */
  async subscribeRun(
    runId: string,
    onProgress: ProgressCallback,
    mode: 'chat' | 'work' = 'work',
    signal = authSubject.signal,
  ): Promise<AgentResponse> {
    return this.executeSSERequest(
      `/api${this.baseUrl}/runs/${encodeURIComponent(runId)}/stream`,
      'GET',
      undefined,
      onProgress,
      false,
      mode,
      signal,
    )
  }

  async processWithProgress(
    input: string,
    onProgress: ProgressCallback,
    context?: Partial<ProcessContext>,
    signal = authSubject.signal,
  ): Promise<AgentResponse> {
    console.log('[AgentService] processWithProgress called with input:', input)

    const request: ProcessRequest = {
      input,
      context: {
        currentRoute: window.location.pathname,
        ...context,
      },
    }

    const lane = context?.mode === 'chat' ? 'chat' : 'work'
    return this.executeSSERequest(
      `/api${this.baseUrl}/process/stream`,
      'POST',
      request,
      onProgress,
      lane === 'chat',
      lane,
      signal,
    )
  }

  async getTask(
    taskId: string,
    signal = authSubject.signal,
  ): Promise<TaskDetail> {
    const response = await apiService.get<{
      success: boolean
      task: TaskInfo & {
        pendingQuestion?: TaskInfo['pendingQuestion']
      }
      results: Record<string, unknown>
      startedAt: string
      completedAt?: string
    }>(`${this.baseUrl}/tasks/${taskId}`, { signal })

    return {
      taskId: response.task.taskId,
      recipeId: '',
      status: response.task.status,
      progress: response.task.progress,
      startedAt: response.startedAt,
      completedAt: response.completedAt,
      results: response.results,
      pendingQuestion: response.task.pendingQuestion,
      stepHistory: response.task.stepHistory,
    }
  }

  async listTasks(): Promise<TaskDetail[]> {
    const response = await apiService.get<{
      success: boolean
      tasks: TaskDetail[]
      total: number
    }>(`${this.baseUrl}/tasks`)
    return response.tasks
  }

  async cancelTask(
    taskId: string,
  ): Promise<{ success: boolean; message: string }> {
    return apiService.post<{
      success: boolean
      message: string
      taskId: string
    }>(`${this.baseUrl}/tasks/${taskId}/cancel`)
  }

  async submitFrontendAck(
    taskId: string,
    stepId: string,
    payload: {
      musicStatus?: unknown
      windowState?: unknown
    },
    signal?: AbortSignal,
  ): Promise<void> {
    try {
      await apiService.post(
        `${this.baseUrl}/tasks/${taskId}/frontend-ack`,
        {
          stepId,
          musicStatus: payload.musicStatus ?? null,
          windowState: payload.windowState ?? null,
        },
        { signal },
      )
    } catch {}
  }

  async answerQuestion(
    taskId: string,
    questionId: string,
    answer: string,
  ): Promise<AgentResponse> {
    return apiService.post<AgentResponse>(
      `${this.baseUrl}/tasks/${taskId}/answer`,
      { questionId, answer },
    )
  }

  async answerQuestionWithProgress(
    taskId: string,
    questionId: string,
    answer: string,
    onProgress: ProgressCallback,
  ): Promise<AgentResponse> {
    // abortPrevious=false: answer + run subscriptions may coexist.
    return this.executeSSERequest(
      `/api${this.baseUrl}/tasks/${taskId}/answer/stream`,
      'POST',
      { questionId, answer },
      onProgress,
      false,
    )
  }

  async getCapabilities(): Promise<Capability[]> {
    const response = await apiService.get<{
      success: boolean
      capabilities: {
        capabilities?: Array<{
          id: string
          name: string
          description?: string
          category?: string
          /** IntentAction is a snake_case string; coerce objects. */
          actions?: unknown
          requiresAi?: boolean
          requires_ai?: boolean
        }>
        totalCount?: number
        total?: number
        byCategory?: Record<
          string,
          Array<{ id: string; name: string; hint?: string; ai?: boolean }>
        >
      }
    }>(`${this.baseUrl}/capabilities`)
    const body = response.capabilities
    const capNames = currentCopy().agentCaps as Record<string, string>
    const capName = (id: string, fallback: string) =>
      capNames[id] || (fallback ? userFacingError(fallback) : '')
    const capDesc = (id: string, fallback: string) =>
      capNames[`${id}.desc`] || fallback || ''
    if (Array.isArray(body?.capabilities)) {
      return body.capabilities.map((cap) => ({
        id: cap.id,
        name: capName(cap.id, cap.name),
        description: capDesc(cap.id, cap.description || ''),
        category: cap.category ? userFacingError(cap.category) : '',
        actions: normalizeCapabilityActions(cap.actions),
        requiresAi: Boolean(cap.requiresAi ?? cap.requires_ai),
      }))
    }
    const byCat = body?.byCategory
    if (byCat && typeof byCat === 'object') {
      return Object.entries(byCat).flatMap(([category, items]) =>
        (items || []).map((item) => ({
          id: item.id,
          name: capName(item.id, item.name),
          description: capDesc(item.id, item.hint || ''),
          category: category ? userFacingError(category) : '',
          actions: [],
          requiresAi: Boolean(item.ai),
        })),
      )
    }
    return []
  }

  async health(): Promise<{
    status: string
    service: string
    version: string
  }> {
    return apiService.get<{ status: string; service: string; version: string }>(
      `${this.baseUrl}/health`,
    )
  }

  async pollTaskUntilComplete(
    taskId: string,
    options: {
      intervalMs?: number
      timeoutMs?: number
      onProgress?: (task: TaskDetail) => void
      signal?: AbortSignal
    } = {},
  ): Promise<TaskDetail> {
    const {
      intervalMs = 1000,
      timeoutMs = 300000,
      onProgress,
      signal = authSubject.signal,
    } = options
    const startTime = Date.now()

    while (Date.now() - startTime < timeoutMs) {
      signal.throwIfAborted()
      const task = await this.getTask(taskId, signal)
      signal.throwIfAborted()

      if (onProgress) {
        onProgress(task)
      }

      if (
        task.status === 'completed' ||
        task.status === 'failed' ||
        task.status === 'cancelled' ||
        task.status === 'waiting_for_input'
      ) {
        return task
      }

      await new Promise((resolve) => setTimeout(resolve, intervalMs))
    }

    throw new Error(currentCopy().errors.agentStepTimeout)
  }

  async getPresets(): Promise<TaskPresetListResponse> {
    return apiService.get<TaskPresetListResponse>(`${this.baseUrl}/presets`)
  }

  async createPreset(preset: CreatePresetRequest): Promise<TaskPreset> {
    return apiService.post<TaskPreset>(`${this.baseUrl}/presets`, preset)
  }

  async addToFavorites(
    input: string,
    parsedSteps?: unknown,
    intentSummary?: string,
  ): Promise<TaskPreset> {
    return this.createPreset({
      input,
      presetType: 'favorite',
      parsedSteps,
      intentSummary,
      // Favorites never store chat; always rerun.
    })
  }

  async deletePreset(presetId: number): Promise<{ success: boolean }> {
    return apiService.delete<{ success: boolean }>(
      `${this.baseUrl}/presets/${presetId}`,
    )
  }

  async toggleFavorite(presetId: number): Promise<TaskPreset> {
    return apiService.post<TaskPreset>(
      `${this.baseUrl}/presets/${presetId}/toggle-favorite`,
    )
  }

  async usePreset(presetId: number): Promise<TaskPreset> {
    return apiService.post<TaskPreset>(
      `${this.baseUrl}/presets/${presetId}/use`,
    )
  }

  async executePreset(
    presetId: number,
    onProgress?: ProgressCallback,
  ): Promise<AgentResponse> {
    return this.executeSSERequest(
      `/api${this.baseUrl}/presets/${presetId}/execute`,
      'POST',
      undefined,
      onProgress,
    )
  }

  async getQueueStatus(): Promise<QueueStatus> {
    return apiService.get<QueueStatus>(`${this.baseUrl}/queue/status`)
  }

  async interruptSession(input: string): Promise<{
    success: boolean
    cancelled_tasks: number
    response: AgentResponse
  }> {
    return apiService.post(`${this.baseUrl}/session/interrupt`, { input })
  }

  /** Her reply was read aloud here and talked over, `heardChars` in. */
  async markHeardCutOff(
    sessionId: string,
    runId: string,
    heardChars: number,
  ): Promise<{ success: boolean }> {
    return apiService.post(`${this.baseUrl}/session/heard-cut-off`, {
      sessionId,
      runId,
      heardChars,
    })
  }

  async cancelChatTurn(sessionId: string): Promise<{ success: boolean }> {
    return apiService.post(`${this.baseUrl}/session/cancel-chat`, { sessionId })
  }

  async steerSession(
    instruction: string,
    taskId?: string,
    signal = authSubject.signal,
  ): Promise<{
    success: boolean
    message: string
    taskId: string
    queued: boolean
  }> {
    return apiService.post(
      `${this.baseUrl}/session/steer`,
      {
        instruction,
        ...(taskId ? { taskId } : {}),
      },
      { signal },
    )
  }

  async getHeartbeatTasks(): Promise<HeartbeatTask[]> {
    const response = await apiService.get<{ tasks: HeartbeatTask[] }>(
      `${this.baseUrl}/heartbeat`,
    )
    return response.tasks
  }

  async toggleHeartbeat(
    taskId: string,
  ): Promise<{ task_id: string; enabled: boolean }> {
    return apiService.post(`${this.baseUrl}/heartbeat/${taskId}/toggle`)
  }

  async updateHeartbeat(
    taskId: string,
    patch: {
      name?: string
      schedule?: string
      action?: string
      enabled?: boolean
    },
  ): Promise<HeartbeatTask> {
    const response = await apiService.put<{ task: HeartbeatTask }>(
      `${this.baseUrl}/heartbeat/${encodeURIComponent(taskId)}`,
      patch,
    )
    return response.task
  }

  async createHeartbeat(body: {
    name: string
    schedule: string
    action: string
    enabled?: boolean
    id?: string
  }): Promise<HeartbeatTask> {
    const response = await apiService.post<{ task: HeartbeatTask }>(
      `${this.baseUrl}/heartbeat`,
      body,
    )
    return response.task
  }

  async deleteHeartbeat(
    taskId: string,
  ): Promise<{ deleted: boolean; task_id: string }> {
    return apiService.delete(
      `${this.baseUrl}/heartbeat/${encodeURIComponent(taskId)}`,
    )
  }

  async getTraces(
    limit: number = 20,
  ): Promise<{ traces: ExecutionTrace[]; total: number }> {
    return apiService.get(`${this.baseUrl}/traces?limit=${limit}`)
  }

  /** All of her memory, sorted by whose it is (site admin). */
  /** Looking into her (admin): herself, each person, each group. */
  async getMind(): Promise<MindSnapshot> {
    return apiService.get<MindSnapshot>(`${this.baseUrl}/mind`)
  }

  async getAllMemories(): Promise<ManagedMemory[]> {
    const response = await apiService.get<{ memories: ManagedMemory[] }>(
      `${this.baseUrl}/memory/all`,
    )
    return response.memories
  }

  async updateAnyMemory(memoryId: string, content: string): Promise<void> {
    await apiService.put(
      `${this.baseUrl}/memory/all/${encodeURIComponent(memoryId)}`,
      { content },
    )
  }

  /** Confirm a Work lesson still on trial: Work goes by it from now on. */
  async confirmTriedMemory(memoryId: string): Promise<void> {
    await apiService.post(
      `${this.baseUrl}/memory/all/${encodeURIComponent(memoryId)}/confirm`,
      {},
    )
  }

  async deleteAnyMemory(memoryId: string): Promise<void> {
    await apiService.delete(
      `${this.baseUrl}/memory/all/${encodeURIComponent(memoryId)}`,
    )
  }

  async getMcpStatus(): Promise<{
    servers: Array<{
      id: string
      healthy: boolean
      tool_count: number
      auto_restart: boolean
    }>
    tool_count: number
  }> {
    const response = await apiService.get<{
      servers?: Array<Record<string, unknown>>
      tool_count?: number
    }>(`${this.baseUrl}/mcp/status`)
    return {
      servers: parseMcpRuntimeServers(response.servers),
      tool_count:
        typeof response.tool_count === 'number' ? response.tool_count : 0,
    }
  }

  async getMcpConfig(): Promise<McpConfigSnapshot> {
    const response = await apiService.get<{
      config?: { servers?: unknown }
      config_path?: string
      runtime?: {
        servers?: Array<Record<string, unknown>>
        tool_count?: number
      }
    }>(`${this.baseUrl}/mcp/config`)
    return normalizeMcpConfigSnapshot(response)
  }

  async putMcpConfig(servers: McpServerConfig[]): Promise<McpConfigSnapshot> {
    const response = await apiService.put<{
      config?: { servers?: unknown }
      config_path?: string
      runtime?: {
        servers?: Array<Record<string, unknown>>
        tool_count?: number
      }
      error?: string
    }>(`${this.baseUrl}/mcp/config`, { servers })
    return normalizeMcpConfigSnapshot(response)
  }

  async reloadMcp(): Promise<{ reloaded: boolean; tool_count: number }> {
    const response = await apiService.post<{
      reloaded?: boolean
      tool_count?: number
    }>(`${this.baseUrl}/mcp/reload`, {})
    return {
      reloaded: response.reloaded === true,
      tool_count:
        typeof response.tool_count === 'number' ? response.tool_count : 0,
    }
  }

  async getSkills(): Promise<SkillInfo[]> {
    try {
      const response = await apiService.get<{ skills: SkillInfo[] }>(
        `${this.baseUrl}/skills`,
      )
      return response.skills
    } catch {
      return []
    }
  }

  async deleteSkill(skillId: string): Promise<void> {
    await apiService.delete(
      `${this.baseUrl}/skills/${encodeURIComponent(skillId)}`,
    )
  }

  async getPersonaSignals(body: {
    language: string
    regenerate?: boolean
  }): Promise<{
    reportCount: number
    tags: string[]
    aiDistilled: boolean
  }> {
    return sharePersonaGeneration(
      `signals:${body.language}:${body.regenerate === true}`,
      () =>
        apiService.post(
          `${this.baseUrl}/persona/signals`,
          {
            consent: true,
            language: body.language,
            regenerate: body.regenerate === true,
          },
          { timeout: PERSONA_GENERATION_TIMEOUT_MS },
        ),
    )
  }

  async importPersona(body: {
    source: string
    name?: string
    gender?: string
    language: string
  }): Promise<{
    persona: Record<string, unknown>
  }> {
    return sharePersonaGeneration(
      `import:${body.language || ''}:${body.name || ''}:${body.gender || ''}:${body.source.length}:${body.source.slice(0, 80)}`,
      () =>
        apiService.post(`${this.baseUrl}/persona/import`, body, {
          timeout: PERSONA_GENERATION_TIMEOUT_MS,
        }),
    )
  }

  async draftPersona(body: {
    name: string
    tags: string[]
    gender?: string
    extraRequirements?: string
    language: string
  }): Promise<{
    persona: Record<string, unknown>
  }> {
    return sharePersonaGeneration(
      `draft:${body.language || ''}:${body.name}:${body.gender || ''}:${body.extraRequirements || ''}:${body.tags.join(',')}`,
      () =>
        apiService.post(`${this.baseUrl}/persona/draft`, body, {
          timeout: PERSONA_GENERATION_TIMEOUT_MS,
        }),
    )
  }

  async suggestPersonaName(body: {
    gender?: string
    avoidName?: string
    nameStyle?: string
    language: string
  }): Promise<{ name: string }> {
    return sharePersonaGeneration(
      `name:${body.language || ''}:${body.nameStyle || ''}:${body.gender || ''}:${body.avoidName || ''}`,
      () =>
        apiService.post(`${this.baseUrl}/persona/name`, body, {
          timeout: NAME_SUGGEST_TIMEOUT_MS,
        }),
    )
  }

  async suggestPersonaVisualDesign(body: {
    gender: string
    language: string
    visualRequirements?: string
    clothingStyle: string
    keepCharacter?: boolean
    regenerate?: boolean
    existingVisualIdentity?: object
  }): Promise<{ visualIdentity: Record<string, unknown> }> {
    return sharePersonaGeneration(
      `visual-design:${body.language}:${body.gender}:${body.regenerate === true}:${body.keepCharacter === true}:${body.clothingStyle}:${body.visualRequirements || ''}:${JSON.stringify(body.existingVisualIdentity ?? null)}`,
      () =>
        apiService.post(`${this.baseUrl}/persona/visual-design`, body, {
          timeout: PERSONA_GENERATION_TIMEOUT_MS,
        }),
    )
  }

  async observeVisualFromPortrait(body: {
    gender: string
    language: string
  }): Promise<{
    visualIdentity: Record<string, unknown>
    clothingStyle: string
  }> {
    // Do not reuse sharePersonaGeneration across main-image changes.
    return apiService.post(
      `${this.baseUrl}/persona/visual-from-portrait`,
      body,
      {
        timeout: PERSONA_GENERATION_TIMEOUT_MS,
      },
    )
  }

  /** 访客看得到的那份：名字和人设正文里能给人看的几项。人设关着时 profile 是 null。 */
  async getPublicPersona(): Promise<PublicPersona> {
    return apiService.get(`${this.baseUrl}/persona/public`)
  }

  async getPersona(): Promise<AgentPersona | null> {
    try {
      return await apiService.get(`${this.baseUrl}/persona`)
    } catch (error) {
      if (error instanceof ApiError && error.code === 'merope_disabled') {
        return null
      }
      throw error
    }
  }

  async putPersona(body: {
    name: string
    personality: string
    portraitAssetId?: string | null
    persona?: Record<string, unknown> | null
    visualProfile?: Record<string, unknown> | null
    /** Absent keeps it; null or empty is the site's own zone. */
    zone?: string | null
  }): Promise<AgentPersona> {
    return apiService.put(`${this.baseUrl}/persona`, body)
  }

  /** Values one field of her persona held before edits replaced them, newest first. */
  async getPersonaHistory(field: string): Promise<PersonaFieldHistoryEntry[]> {
    const body = await apiService.get<{ entries: PersonaFieldHistoryEntry[] }>(
      `${this.baseUrl}/persona/history?field=${encodeURIComponent(field)}`,
    )
    return Array.isArray(body.entries) ? body.entries : []
  }

  async deletePersona(): Promise<void> {
    await apiService.delete(`${this.baseUrl}/persona`)
  }

  async putAddressee(body: {
    doNotDisturb?: boolean
    dndStart?: string | null
    dndEnd?: string | null
  }): Promise<{
    mood: number
    arousal?: number
    activity: string
    doNotDisturb: boolean
    doNotDisturbActive?: boolean
    dndStart?: string | null
    dndEnd?: string | null
  }> {
    return apiService.put(`${this.baseUrl}/addressee`, body)
  }

  async getHer(): Promise<MeropeHerResponse> {
    return apiService.get<MeropeHerResponse>(`${this.baseUrl}/her`)
  }

  async getDoing(): Promise<MeropeDoingResponse> {
    return apiService.get<MeropeDoingResponse>(`${this.baseUrl}/doing`)
  }

  /** How she acts what she is doing on her own, as the director decides it. */
  async getDoingActing(body: {
    rigState: RigStateSummary
    together: boolean
  }): Promise<{ performance: PerformanceDirective | null }> {
    return apiService.post<{ performance: PerformanceDirective | null }>(
      `${this.baseUrl}/doing/acting`,
      body,
    )
  }

  async creditMusicListening(listenedSeconds: number): Promise<{
    credited: boolean
    nextCreditInSeconds: number
    mood: MoodTransition
    activity: string
  }> {
    return apiService.post(`${this.baseUrl}/addressee/music-listening`, {
      listenedSeconds: Math.max(0, Math.floor(listenedSeconds)),
    })
  }

  async createSession(): Promise<SessionInfo> {
    return apiService.post<SessionInfo>(`${this.baseUrl}/sessions`)
  }

  async listSessions(
    page: number = 1,
    limit: number = 20,
  ): Promise<SessionInfo[]> {
    const response = await apiService.get<{ sessions: SessionInfo[] }>(
      `${this.baseUrl}/sessions?page=${page}&limit=${limit}`,
    )
    return response.sessions
  }

  async getSessionMessages(
    sessionId: string,
    page: number = 1,
    limit: number = 50,
    signal = authSubject.signal,
  ): Promise<SessionMessage[]> {
    const response = await apiService.get<{ messages: SessionMessage[] }>(
      `${this.baseUrl}/sessions/${sessionId}/messages?page=${page}&limit=${limit}`,
      { signal },
    )
    return response.messages
  }

  /** The run still writing a reply in this session, for a page opened or reloaded meanwhile; null when none. */
  async getLiveSessionRun(
    sessionId: string,
    signal = authSubject.signal,
  ): Promise<string | null> {
    const response = await apiService.get<{ runId?: unknown }>(
      `${this.baseUrl}/sessions/${encodeURIComponent(sessionId)}/live-run`,
      { signal },
    )
    return typeof response.runId === 'string' && response.runId
      ? response.runId
      : null
  }

  async archiveSession(sessionId: string): Promise<{ success: boolean }> {
    return apiService.delete<{ success: boolean }>(
      `${this.baseUrl}/sessions/${sessionId}`,
    )
  }

  async updateSessionTitle(
    sessionId: string,
    title: string,
  ): Promise<SessionInfo> {
    return apiService.patch<SessionInfo>(
      `${this.baseUrl}/sessions/${sessionId}`,
      { title },
    )
  }

  async generateSessionTitle(sessionId: string): Promise<{ title: string }> {
    return apiService.post<{ title: string }>(
      `${this.baseUrl}/sessions/${sessionId}/generate-title`,
      {},
    )
  }

  private async executeSSERequest(
    url: string,
    method: 'GET' | 'POST',
    body?: unknown,
    onProgress?: ProgressCallback,
    abortPrevious = true,
    lane: 'work' | 'chat' = 'work',
    signal = authSubject.signal,
  ): Promise<AgentResponse> {
    return executeSSERequest({
      signal,
      url,
      method,
      body,
      onProgress,
      abortPrevious,
      activeControllers: this.activeAbortControllersByMode[lane],
      pollTaskUntilComplete: (taskId, options) =>
        this.pollTaskUntilComplete(taskId, options),
    })
  }
}

export const agentService = new AgentService()
