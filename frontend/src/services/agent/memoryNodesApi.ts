import { apiService } from '../api'

/** One node in a list of the site owner's node page. */
export interface MemoryNodeLine {
  id: string
  name?: string | null
  /** `topic`, `task` or `person`. */
  kind?: string | null
  /** `active`, `ongoing`, `resting`, `done`, `dropped`. */
  status: string
  /** 「关于你」 in the owner's talk, the character's own node in its own audience. */
  own: boolean
  entries: number
  spans: number
  waiting: number
  /** Unix microseconds. */
  latest: number
}

/** One audience of the page: the owner's private talk, the character's own, or a group. */
export interface MemoryNodeAudience {
  audience: string
  place: 'private' | 'own' | 'group'
  /** A group's id (`qq:5`). */
  group?: string | null
  /** A group's name as its platform gives it. */
  groupName?: string | null
  nodes: MemoryNodeLine[]
}

export interface MemoryNodeList {
  herName?: string | null
  audiences: MemoryNodeAudience[]
}

export interface MemoryNodeHeadItem {
  id: string
  text: string
  at: number
}

export interface MemoryNodeEntry {
  row: string
  text: string
  at: number
  saidBy: string[]
  /** Only seen elsewhere: never taken as a fact. */
  seen: boolean
  quoteOnly: boolean
}

export interface MemoryNodeSpan {
  row: string
  text: string
  at: number
  saidBy: string[]
  /** The original words were deleted; only the note is kept. */
  wordsGone: boolean
}

export interface MemoryNodeIntent {
  id: string
  text: string
  /** `waiting`, `said`, `done`, `dropped`. */
  status: string
  origin?: 'partner' | 'her' | null
  action?: 'say' | 'work' | null
  at: number
  when?: {
    at?: number | null
    zone?: string | null
    node?: string | null
    change?: 'changed' | 'done' | null
  } | null
  instruction?: {
    echo: string
    /** The owner's message; none once deleted. */
    message?: string | null
    way?: 'ran' | 'raised' | 'cancelled' | null
    line?: string | null
  } | null
}

export interface MemoryNodePage {
  id: string
  audience: string
  gone: boolean
  movedTo?: string | null
  name?: string | null
  kind?: string | null
  status: string
  own: boolean
  namedByOwner: boolean
  aliases: string[]
  summary: string[]
  views: MemoryNodeHeadItem[]
  open: MemoryNodeHeadItem[]
  entries: MemoryNodeEntry[]
  heard: MemoryNodeEntry[]
  spans: MemoryNodeSpan[]
  intents: MemoryNodeIntent[]
}

const BASE = '/agent/memory/nodes'

/** The site owner's page of the character's nodes (site owner only). */
export const memoryNodesApi = {
  list(): Promise<MemoryNodeList> {
    return apiService.get<MemoryNodeList>(BASE)
  },

  node(audience: string, node: string): Promise<MemoryNodePage> {
    const query = new URLSearchParams({ audience, node })
    return apiService.get<MemoryNodePage>(`${BASE}/one?${query.toString()}`)
  },

  async rename(audience: string, node: string, name: string): Promise<void> {
    await apiService.post(`${BASE}/rename`, { audience, node, name })
  },

  /** Deletes a node, or one entry on it when `row` is given. */
  async forget(audience: string, node: string, row?: string): Promise<void> {
    await apiService.post(
      `${BASE}/forget`,
      row ? { audience, node, row } : { audience, node },
    )
  },

  async cancelIntent(
    audience: string,
    node: string,
    intent: string,
  ): Promise<void> {
    await apiService.post(`${BASE}/cancel`, { audience, node, intent })
  },

  async answerHeard(
    audience: string,
    node: string,
    row: string,
    answer: 'confirmed' | 'denied',
  ): Promise<void> {
    await apiService.post(`${BASE}/heard`, { audience, node, row, answer })
  },
}
