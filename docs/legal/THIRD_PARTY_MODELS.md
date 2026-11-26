# Third-party models

Models that Myriad's character figure pipeline uses, with their licences. Myriad
ships none of these weights.

## Character layering (See-through)

Character layering splits a portrait into layers and estimates their depth. The
backend does not load the models: it sends the portrait to a See-through Space
on Hugging Face (`backend/src/services/see_through.rs`; `DEFAULT_SPACE` unless
the site owner names another Space), and the models run there. The default
Space loads the two models below (`REPO_LAYERDIFF` and `REPO_DEPTH` in its
`app.py`).

The layering and depth models are subject to the use-based restrictions of the
CreativeML Open RAIL++-M License and the CreativeML Open RAIL-M License; see
[MODEL_USE_RESTRICTIONS.md](MODEL_USE_RESTRICTIONS.md).

| Model | Role | Licence | Licence text |
| --- | --- | --- | --- |
| [See-through LayerDiff 3D](https://huggingface.co/layerdifforg/seethroughv0.0.2_layerdiff3d) | Layer decomposition | Apache-2.0 for the See-through Authors' contributions; CreativeML Open RAIL++-M (derived from Animagine XL 4.0 and Stable Diffusion XL 1.0); CreativeML Open RAIL-M (includes weights derived from LayerDiffuse); MIT (the VAE, SDXL-VAE-FP16-Fix) | [`LICENSES/LicenseRef-See-through-LayerDiff3D-weights.txt`](../../LICENSES/LicenseRef-See-through-LayerDiff3D-weights.txt) |
| [See-through Marigold Depth](https://huggingface.co/24yearsold/seethroughv0.0.1_marigold) | Depth of each layer | Apache-2.0 for the See-through Authors' contributions; Open RAIL++-M (derived from Marigold Depth v1.1); CreativeML Open RAIL++-M (Stable Diffusion 2, the base of Marigold) | [`LICENSES/LicenseRef-See-through-Marigold-weights.txt`](../../LICENSES/LicenseRef-See-through-Marigold-weights.txt) |

Each weight licence states that the use-based restrictions in paragraph 5 and
Attachment A of its Open RAIL licences apply to every use of the model.

The models they derive from:

| Model | Licence |
| --- | --- |
| [Animagine XL 4.0](https://huggingface.co/cagliostrolab/animagine-xl-4.0) (Cagliostro Labs) | CreativeML Open RAIL++-M |
| [Stable Diffusion XL base 1.0](https://huggingface.co/stabilityai/stable-diffusion-xl-base-1.0) (Stability AI) | CreativeML Open RAIL++-M, [`LICENSES/LicenseRef-CreativeML-OpenRAIL-PlusPlus-M.txt`](../../LICENSES/LicenseRef-CreativeML-OpenRAIL-PlusPlus-M.txt) |
| [LayerDiffuse](https://huggingface.co/lllyasviel/LayerDiffuse_Diffusers) (Lvmin Zhang) | CreativeML Open RAIL-M, [`LICENSES/CreativeML-OpenRAIL-M.txt`](../../LICENSES/CreativeML-OpenRAIL-M.txt), as stated by the See-through weight licence; the model card itself declares only `license: openrail` |
| [SDXL-VAE-FP16-Fix](https://huggingface.co/madebyollin/sdxl-vae-fp16-fix) (madebyollin) | MIT |
| [Marigold Depth v1.1](https://huggingface.co/prs-eth/marigold-depth-v1-1) (PRS, ETH Zürich) | Open RAIL++-M |
| Stable Diffusion 2 (Stability AI) | CreativeML Open RAIL++-M |

The See-through code ([shitagaki-lab/see-through](https://github.com/shitagaki-lab/see-through))
is licensed under Apache-2.0.

## Other models in the figure pipeline

These are not under an Open RAIL licence.

| Model | Where it runs | Licence |
| --- | --- | --- |
| [Real-ESRGAN](https://github.com/xinntao/Real-ESRGAN) `RealESRGAN_x4plus_anime_6B` | The default See-through Space, to enlarge a full-body picture before layering | BSD-3-Clause (the Real-ESRGAN repository, which publishes the weights in its releases) |
| [DWPose](https://huggingface.co/yzd-v/DWPose) `dw-ll_ucoco_384` | The Myriad backend, to find joints on the portrait (`backend/src/services/pose_estimation.rs`) | Apache-2.0 |

## Licence text sources

| File | Source |
| --- | --- |
| `LICENSES/LicenseRef-CreativeML-OpenRAIL-PlusPlus-M.txt` | [`LICENSE.md`](https://huggingface.co/stabilityai/stable-diffusion-xl-base-1.0/blob/462165984030d82259a11f4367a4eed129e94a7b/LICENSE.md) of Stable Diffusion XL base 1.0 |
| `LICENSES/CreativeML-OpenRAIL-M.txt` | [`LICENSE`](https://github.com/CompVis/stable-diffusion/blob/69ae4b35e0a0f6ee1af8bb9a5d0016ccb27e36dc/LICENSE) of CompVis/stable-diffusion |
| `LICENSES/LicenseRef-See-through-LayerDiff3D-weights.txt` | [`LICENSE`](https://huggingface.co/layerdifforg/seethroughv0.0.2_layerdiff3d/blob/4477e6ce529bc6a141732e1a55a4932176db3b89/LICENSE) of See-through LayerDiff 3D |
| `LICENSES/LicenseRef-See-through-Marigold-weights.txt` | [`LICENSE`](https://huggingface.co/24yearsold/seethroughv0.0.1_marigold/blob/4f4ffc46050b6feb764b628859968a26e76e6b6a/LICENSE) of See-through Marigold Depth |

Each file is the source byte for byte.
