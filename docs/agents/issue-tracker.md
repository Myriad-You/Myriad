# Issue tracker: Local Markdown

Issues and specs for this repository live as Markdown files under `.scratch/`.

## Conventions

- One feature per directory: `.scratch/<feature-slug>/`
- The feature specification is `.scratch/<feature-slug>/spec.md`
- Implementation tickets are stored one per file:
  `.scratch/<feature-slug>/issues/<NN>-<slug>.md`
- Tickets are numbered from `01` in dependency order
- Ticket state is recorded in a `Status:` line
- Comments and conversation history are appended under `## Comments`

## Publishing

When an engineering skill says to publish a specification, create:

`.scratch/<feature-slug>/spec.md`

When an engineering skill says to publish tickets, create one file per ticket under:

`.scratch/<feature-slug>/issues/`

Do not combine all tickets into one file.

## Fetching work

When an engineering skill says to fetch a ticket, read the referenced file under `.scratch/`.

## Blocking relationships

Each ticket records its blockers in a `Blocked by:` line. A ticket is ready to start when every listed blocker is resolved.

## Wayfinding operations

- Map: `.scratch/<effort>/map.md`
- Child ticket: `.scratch/<effort>/issues/<NN>-<slug>.md`
- Ticket type: recorded in a `Type:` line
- Ticket state: recorded in a `Status:` line
- Frontier: the first open, unblocked, and unclaimed ticket by number
- Claim: change `Status:` to `claimed` before starting work
- Resolve: append an `## Answer`, change `Status:` to `resolved`, and update the map
