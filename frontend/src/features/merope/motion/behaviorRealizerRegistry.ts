import type {
  BehaviorPlan,
  BehaviorRealizerReport,
  ScheduledBehavior,
} from './behavior'

export interface BehaviorRealizerContext {
  nowMs: number
  originMs: number
  pegTimes: ReadonlyMap<string, number>
}

export type BehaviorFormRealizer<Output> = (
  behavior: ScheduledBehavior,
  context: BehaviorRealizerContext,
) => Output | null

export class BehaviorRealizerRegistry<Output> {
  private readonly forms = new Map<string, BehaviorFormRealizer<Output>>()

  register(family: string, realizer: BehaviorFormRealizer<Output>): this {
    this.forms.set(family, realizer)
    return this
  }

  realize(
    plan: BehaviorPlan,
    nowMs: number,
  ): { outputs: Output[]; reports: BehaviorRealizerReport[] } {
    const pegTimes = new Map(plan.pegs.map((peg) => [peg.id, peg.atMs]))
    const context = { nowMs, originMs: plan.originMs, pegTimes }
    const outputs: Output[] = []
    const reports: BehaviorRealizerReport[] = []
    for (const behavior of plan.behaviors) {
      const realizer = this.forms.get(behavior.form.family)
      if (!realizer) {
        reports.push({
          behaviorId: behavior.id,
          result: 'rejected',
          atMs: nowMs,
          reason: 'unsupported-form',
        })
        continue
      }
      const output = realizer(behavior, context)
      if (!output) {
        reports.push({
          behaviorId: behavior.id,
          result: 'rejected',
          atMs: nowMs,
          reason: 'invalid-timing',
        })
        continue
      }
      outputs.push(output)
      reports.push({ behaviorId: behavior.id, result: 'accepted', atMs: nowMs })
    }
    return { outputs, reports }
  }
}
