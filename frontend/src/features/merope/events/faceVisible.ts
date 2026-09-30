/** Fail closed until a live retain exists. */
let visible = false

export function setLiveFaceVisible(value: boolean): void {
  visible = value
}

export function liveFaceVisible(): boolean {
  return visible
}
