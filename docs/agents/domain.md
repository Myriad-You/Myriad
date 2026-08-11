# Domain Docs

This repository uses a single-context domain documentation layout.

## Before exploring

Engineering skills should read these sources when they exist:

- `CONTEXT.md` at the repository root
- Relevant ADRs under `docs/adr/`

If either source does not exist, continue silently. Domain documentation is created lazily when terminology or architectural decisions need to be recorded.

## Layout

```text
/
├── CONTEXT.md
├── docs/
│   └── adr/
└── ...
```

## Vocabulary

Use the domain terms defined in `CONTEXT.md` in specifications, tickets, tests, and implementation discussions.

Do not introduce synonyms for concepts whose preferred names are already defined.

If a required concept is missing, note the gap for domain modeling rather than silently inventing inconsistent terminology.

## Architecture decisions

Before proposing changes, inspect relevant ADRs.

If a specification or ticket conflicts with an existing ADR, state the conflict explicitly instead of silently overriding the decision.
