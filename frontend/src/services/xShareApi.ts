/** Intent URL only; never posts. */

import { apiService } from './api'

export interface XShareStatus {
  success: boolean
  mode: 'intent'
  can_intent: boolean
  can_post: boolean
  hint?: string
}

export interface ComposeXShareRequest {
  text?: string
  title?: string
  summary?: string
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

export const xShareApi = {
  getStatus(): Promise<XShareStatus> {
    return apiService.get<XShareStatus>('/x/share/status')
  },

  compose(req: ComposeXShareRequest): Promise<ComposeXShareResponse> {
    return apiService.post<ComposeXShareResponse>('/x/share', req)
  },
}
