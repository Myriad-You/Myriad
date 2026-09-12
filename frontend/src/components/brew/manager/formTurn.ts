/** 表单关闭后，进行中的保存不能再切模式。 */
export class FormTurn {
  private generation = 0

  begin() {
    return this.generation
  }

  abandon() {
    this.generation += 1
  }

  isCurrent(token: number) {
    return token === this.generation
  }
}
