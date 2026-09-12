export class BrewRevisionChain {
  private changes = new Map<number, Map<number, number>>()

  record(itemId: number, previous: number | undefined, revision: number | undefined): void {
    if (previous === undefined || revision === undefined || !Number.isSafeInteger(previous) || !Number.isSafeInteger(revision) || previous < 0 || revision !== previous + 1) return
    let chain = this.changes.get(itemId)
    if (!chain) this.changes.set(itemId, chain = new Map())
    chain.set(previous, revision)
  }

  advance(itemId: number, observed: number | undefined): number | undefined {
    if (observed === undefined) return undefined
    const chain = this.changes.get(itemId)
    let revision = observed
    while (chain?.has(revision)) revision = chain.get(revision)!
    return revision
  }
}
