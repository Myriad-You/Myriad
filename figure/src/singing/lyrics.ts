export interface LyricLine {
  time: number
  text: string
  translation?: string
}

export interface WordLyricToken {
  time: number
  duration: number
  text: string
}

export interface WordLyricLine {
  time: number
  duration: number
  text: string
  words: WordLyricToken[]
  translation?: string
}
