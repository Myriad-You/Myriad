export type SpeechViseme =
  | 'rest'
  | 'closed'
  | 'open'
  | 'wide'
  | 'round'
  | 'narrow'

export interface SpeechArticulation {
  energy: number | null
  viseme: SpeechViseme
  amount: number
}
