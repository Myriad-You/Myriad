/**
 * X share intent API — compose text + intent URL only.
 * Never posts on behalf of the user (no OAuth, no server-side tweet).
 * Backend: GET /api/x/share/status, POST /api/x/share
 */

import { apiService } from './api'

export interface XShareStatus {
  success: boolean
  mode: 'intent'
  can_intent: boolean
  can_post: boolean
  hint?: string
}

export interface ComposeXShareRequest {
  /** Direct body text (preferred). */
  text?: string
  title?: string
  summary?: string
  /** Optional link appended / used as Intent url param. */
  url?: string
  hashtags?: string[]
  max_length?: number
}

export interface ComposeXShareResponse {
  success: boolean
  mode: 'intent'
  text: string
  char_count: number
  max_length: number
  intent_url: string
  message?: string
}

/**
 * Host client for X Web Intent share (read + compose only).
 * Callers open `intent_url` in a browser; Myriad never posts.
 */
export const xShareApi = {
  getStatus(): Promise<XShareStatus> {
    return apiService.get<XShareStatus>('/x/share/status')
  },

  compose(req: ComposeXShareRequest): Promise<ComposeXShareResponse> {
    return apiService.post<ComposeXShareResponse>('/x/share', req)
  },
}
