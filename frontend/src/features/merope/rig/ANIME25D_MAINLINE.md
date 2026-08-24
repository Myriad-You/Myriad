# Anime2.5DRig mainline regression

Source of truth: [`852wa/Anime2.5DRig`](https://github.com/852wa/Anime2.5DRig),
specifically its `README.md`, `lib/rigger.js`, and the WebGL runtime in
`index.html`. Myriad keeps its own Merope shell and asset transaction flow;
only the layered rig behavior is adopted.

The upstream project is MIT licensed (Copyright © 2026 hakoniwa); this isolated
profile retains that attribution while integrating with Myriad's existing
compiler, renderer, motion clocks, and Merope UI.

## Mainline gap comparison

| Anime2.5DRig behavior                                                                                                                                                                                                 | Merope before this regression                                                                                                   | Mainline gap                                                                                                                                |
| --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------- |
| Accepts a flat see-through PSD with `face` as the only hard requirement                                                                                                                                               | Requires a humanoid body, head, separate upper arms, forearms, hands, thighs, calves, and feet                                  | Add a first-class layered-portrait import path; do not manufacture a 29-bone humanoid                                                       |
| Keeps `face / eyewhite / irides / eyelash / eye_close / eyebrow / mouth_open / mouth_close / nose / ears / earwear / neck / topwear / bottomwear / handwear / headwear / front hair / back hair` as distinct drawings | Collapses face features into two eye parts, clothing into `body`, and accessories into one generic part                         | Preserve source identity and numbered groups, then render with upstream semantic depth and stable PSD-order ties                            |
| Normalizes see-through names, including `mouth` to `mouth_open`, copy suffixes, NFKC text, and numbered hair layers                                                                                                   | Normalizes spelling, but strips numbered layers into one semantic part and treats `mouth_open` as a generic mouth variant       | Match upstream naming while retaining independent numbered hair groups                                                                      |
| Removes low-alpha dust and splits paired eye/eyebrow layers by connected-component centroid around the face center                                                                                                    | Crops at a fixed canvas midpoint and retains low-alpha pixels in the packed atlas                                               | Use content-aware cleanup and side masks anchored to the detected face                                                                      |
| Detects face, eye, iris, mouth, and neck anchors from pixels                                                                                                                                                          | Uses rectangular part centers and humanoid pivots                                                                               | Derive layered-portrait anchors and bind each feature to the correct local pivot                                                            |
| Crossfades open/closed eyelash and mouth drawings while deforming the opening                                                                                                                                         | Presentation slots can crossfade, but the importer discards the separate open-eye stack and requires authored fallback variants | Map `eyelash/eye_close` and `mouth_open/mouth_close` directly to stable slots; synthesize missing close drawings only as a bounded fallback |
| Keeps irises inside eyewhites with stencil clipping                                                                                                                                                                   | Draws all parts with one ordinary alpha pass                                                                                    | Add per-eye stencil masks for `eyewhite` then clip `irides` during the same sorted draw                                                     |
| Uses a depth table plus head-relative parallax/shear for pseudo-3D turns                                                                                                                                              | Rotates/translates a head bone with no layer-depth separation                                                                   | Apply name-based head depth offsets without changing the Merope UI into the upstream demo                                                     |
| Detects up to six hair strands per hair layer and applies a stiff-root/soft-tip double spring                                                                                                                         | Uses one generic secondary spring per imported hair part                                                                        | Build independent root/tip strand chains from each numbered front/back hair layer and feed the existing bounded spring solver               |
| Breath raises/scales `topwear`, the head follows with phase lag, and the chest has a damped bounce                                                                                                                    | Breath is primarily a torso transform and does not know the see-through clothing roles                                          | Add layered topwear/chest weighting and preserve the existing speech/rest damping envelopes                                                 |
| `handwear` may contain separate left/right sleeve, partial-forearm, and hand drawings                                                                                                                                 | Import and regression expect upper-arm/forearm/hand chains and optional relaxed/open-palm/salute/point drawings                 | Attach rigid left/right drawings below one semantic `handwear` parent, clamp final rotation to ±15°, and add no shoulder/elbow/wrist chain |
| Idle combines small head turns, gaze, breath, blink, mouth motion, and secondary hair                                                                                                                                 | Idle exists, but its visible result depends on the humanoid mapping                                                             | Make the layered portrait path consume the same bounded clocks directly                                                                     |

## Acceptance order

1. PSD naming, cleanup, side split, anchors, and independent layer packing.
2. Blink and mouth crossfades, iris clipping, and speech close timing.
3. Hair root/tip spring response and stable resume at expanded/collapsed cadence.
4. Topwear breathing/chest response and depth-separated head turn.
5. rigid left/right `handwear` follow, quiet idle preview, and ±15° action limit.

Humanoid clip/IK/gesture coverage is **outside Rig IR v4**, not a dormant
alternate profile. Existing assets must pass the v4 migration/diagnostic path
or fall back to their portrait; new imports cannot declare limb chains. In
particular, no work in this mainline adds salute, pointing, open-palm, hand
contact, wrist rotation, locomotion, or gesture constraints.

## Implemented path

- `anime25dImporter.ts` owns detection, normalization, connected-component eye
  splitting, synthetic close fallbacks, atlas packing, anchors, semantic depth,
  explicit interior grids, root/tip hair chains, chest weighting, and optional
  rigid left/right `handwear` fragments below one semantic parent.
- `../anime25drig` owns live playback: bind, deform, blink/mouth crossfade,
  depth parallax, hair springs, chest follow, and ±15° handwear composition.
- `diagnostics.ts` identifies the profile from `a25d-*` parts and excludes the
  explicitly abandoned articulated gates while retaining all layered-portrait
  quality gates.

The importer regression fixture proves that See-through-style layers compile
blink, mouth, gaze, chest, hair-strand, depth-turn, and rigid side-handwear paths
without creating shoulder, elbow, wrist, leg, foot, contact, or hand-pose chains. Real PSD
acceptance still requires a generated-material preflight and rendered motion
review; the synthetic fixture is only an architectural gate.
