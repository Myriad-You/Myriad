# Merope 3D pipeline

## Goal

Merope may use a fully 3D character, but the product target is not the
highest possible mesh detail. The target is the best visible result per unit of
generation cost, download size and WebGL frame time.

The 3D backend is independent from image generation. An image provider creates
or repairs reference views; Tripo consumes those views and produces geometry,
rigs and animation clips. Neither provider owns the character definition.

## Boundaries

```text
Character design / reference views
                 |
                 v
        Merope 3D API
        (validation + defaults)
                 |
                 v
             Tripo v3
  upload -> generate -> rig-check -> rig -> retarget
                 |
                 v
       immediate GLB persistence
       validation + budget report
                 |
                 v
      content-addressed local asset
                 |
                 v
       shared WebGL scene/runtime
```

Tripo credentials never leave the host outbound client. `/api/merope/3d`
task creation and uploads stay admin-only. TAPPs and the Agent call the same
Tripo service through `/api/tapp/3d` and `model3d.*` capabilities, gated by the
elevated `3d:generate` permission (not delegated by default). Persisted model
files are public, immutable, content-addressed resources so a guest-facing
Merope scene can render without receiving any provider credential or
expiring URL.
Provider calls inherit Myriad's outbound proxy and bypass configuration; Tripo
does not open a separate network path that behaves differently from the other
external services.

## Default cost/performance profile

- Model: `P1-20260311`, selected for low-poly output and clean topology.
- Face limit: 5,000; callers may explicitly request 50–20,000.
- Texture: enabled; P1 uses its provider default quality rather than receiving
  v3.0+-only quality options.
- PBR: disabled by default; it adds texture memory and is not required for the
  intended stylized character.
- Geometry: no implicit `compress` option for P1 because Tripo documents that
  parameter for v3.0+ models. Explicit caller choices remain preserved.
- Rig: biped v1 with Mixamo-compatible names, because the current character is
  humanoid and the v1 rig exposes the broad preset library.
- Locomotion clips: baked GLB, in-place by default. World translation remains a
  frontend responsibility, which makes eight-direction movement deterministic.
- Polling: every 2 seconds, never faster than the documented provider guidance.
- Task timeout: 15 minutes.
- Persisted result limit: 64 MB.

These are request defaults, not silent overrides. An explicit caller choice is
preserved and the persisted GLB report shows when it leaves the Web budget.

## Supported API flow

All management routes are under `/api/merope/3d`:

1. `POST /files` uploads one PNG/JPEG image or supported model and returns a
   Tripo `file_token`.
2. `POST /tasks` accepts one of `image_to_model`, `multiview_to_model`,
   `rig_check`, `rig`, or `retarget` plus its provider payload.
3. `GET /tasks/{task_id}` queries once. Every successful `model_url` or batch
   `model_urls` result is immediately downloaded and persisted because Tripo
   result URLs expire quickly.
4. `POST /tasks/{task_id}/await` performs bounded server-side polling and the
   same persistence step.
5. `GET /assets/{sha256}` serves the stable GLB; `/metadata` serves its report.

The browser client uses repeated `GET /tasks/{task_id}` requests rather than
holding `/await` open. This avoids Axios and deployment-proxy idle timeouts;
the successful query may take longer because it persists every returned GLB.
Responses expose `assets` in provider order and retain `asset` as the first-item
compatibility field for single-model consumers.

Multiview requests preserve all three Tripo v3 input contracts: direction-keyed
views (including nested URL, file-token, or object-storage sources), the legacy
four positional slots, and reuse of a completed multiview task. Validation still
requires a front view and at least two supplied images when raw views are used.

The recommended character pipeline is four consistent cardinal reference views
(`front`, `left`, `back`, `right`), then P-series multiview generation,
`rig-check`, biped rigging, and in-place `idle` / `walk` / `run` retargeting.
Eight-direction movement does not require eight separately generated models:
the runtime rotates one rigged model in yaw and blends the locomotion clips.

## Persisted GLB checks

Before a model is accepted, the backend validates the GLB magic, version, file
length and JSON chunk. It records:

- byte size;
- scenes, nodes, meshes and primitives;
- materials, textures and images;
- skins and joint count;
- animation count;
- estimated triangle count for triangle-list primitives.

The report marks a model for review when it exceeds the initial Web targets
(currently 16 MB, 12k estimated triangles, eight primitives, or four materials).
Missing skin or animation is reported as a pipeline warning rather than corrupt
input, because raw generation legitimately precedes rigging.

## Configuration

Tripo has its own settings section and environment namespace:

- `TRIPO_ENABLED`
- `TRIPO_API_KEY`
- `TRIPO_BASE_URL`
- `TRIPO_MODEL`
- `TRIPO_FACE_LIMIT`
- `TRIPO_POLL_INTERVAL_SECONDS`
- `TRIPO_TASK_TIMEOUT_SECONDS`
- `TRIPO_MAX_DOWNLOAD_MB`

The API key is masked in the admin UI and encrypted through the existing
configuration storage path. Disabling Tripo prevents all paid task creation but
does not remove already persisted Merope assets.

For image generation the canonical request field is `input`. The API adapter
also normalizes the `file_token` alias shown by Tripo's game-ready quick-start
example, while rejecting conflicting values when both fields are supplied.

## Deliberately deferred

- A paid live generation run: it requires an explicit key, credits and chosen
  source views.
- Automatic texture transcoding to KTX2: add after real Tripo outputs show that
  texture download/GPU memory is the bottleneck.
- A database job ledger: current provider task IDs can be polled directly; add a
  durable pipeline entity when onboarding owns end-to-end retries and recovery.
- Renderer integration: the frontend client contract is ready, but the preview
  branch does not yet contain the independent Merope rig/viewer worktree.

## Provider references

- [Tripo v3 quick start](https://developers.tripo3d.ai/en/docs/quick-start)
- [P-series multiview generation](https://developers.tripo3d.ai/en/docs/generation-multiview-to-model/p)
- [Task query](https://developers.tripo3d.ai/en/docs/task-query)
- [File upload](https://developers.tripo3d.ai/en/docs/files)
- [Rig check](https://developers.tripo3d.ai/en/docs/animations-rig-check)
- [Auto rig](https://developers.tripo3d.ai/en/docs/animations-rig)
- [Animation retarget](https://developers.tripo3d.ai/en/docs/animations-retarget)
