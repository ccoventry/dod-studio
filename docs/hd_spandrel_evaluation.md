# HD textures: newer upscaling models through spandrel, evaluated

*2026-09-26. R&D for the second upscaler backend in `goldsrc-hooks/tools/hd` (`styles.py`'s `spandrel` kind, `spandrel_run.py`, `setup_tools.py --spandrel`).*

## Why a second backend

Real-ESRGAN ncnn-vulkan, which builds every HD style today, runs only ESRGAN-shaped networks. The community catalogue at [OpenModelDB](https://openmodeldb.info/) has stronger current models built on newer architectures (SPAN, RealPLKSR, DAT, HAT, ATD). Those load through [spandrel](https://github.com/chaiNNer-org/spandrel), the PyTorch loader chaiNNer uses. The backend runs one of those models under its own venv, on the same padded RGB input the scripts already make, and returns the 4x PNG the scripts expect. Everything else -- file names (name + FNV-1a-32 of pixels and palette), wrap padding, masked-texture alpha, the hook's matching -- is untouched, and the results below confirm that.

## Models chosen

Picked for photographic surfaces (stone, wood, plaster, cloth), not anime, and for a direct download link (several OpenModelDB entries are Google Drive only: 4x-RealisticRescaler, 4xNomosUni_span_multijpg):

| Style | Model | Arch | Size | Licence |
|---|---|---|---|---|
| `ultrasharpv2` | 4x-UltraSharpV2 (Kim2091) | DAT2 | 140 MB | CC BY-NC-SA 4.0 |
| `pbrify` | 4x-PBRify_UpscalerV4 (Kim2091), made for old game textures | DAT2 | 140 MB | CC0 |
| `webphoto` | 4xNomosWebPhoto_RealPLKSR (Phhofm) | RealPLKSR | 30 MB | CC BY 4.0 |

Reference: `ultrasharp` (4x-UltraSharp through ncnn), the default style.

## What was built

dod_anzio's 124 map textures (`hd_maps.txt` narrowed to that map through `HD_MAPS`) and the 22 skins of `player/us-inf`, `player/axis-inf`, `p_k98` and `p_garand`, in all three styles, on an RTX 3070 Ti (8 GB).

- Every new style's world folder has exactly the 124 file names the ultrasharp folder has for that map: the names come from the originals, not the upscaler.
- Every file is 1024x1024 (or the texture's own power-of-two shape), 24-bit TGA, 32-bit with alpha for masked ones, as ultrasharp's.

## Build time

| Style | 124 wall textures | 22 skins (mostly 512x512) | Per 256px texture |
|---|---|---|---|
| ultrasharp (ncnn) | 39 s | 25 s | 0.3 s |
| webphoto (RealPLKSR) | 307 s | 201 s | 2.5 s |
| pbrify (DAT2) | 2283 s | 1604 s | 18 s |
| ultrasharpv2 (DAT2) | 2243 s | 1694 s | 18 s |

The DAT2 models are about 60x slower than ncnn ultrasharp per texture, RealPLKSR about 8x. Scaled to the user's usual set (133 maps, about 1,900 unique wall textures, plus models, sprites and detail), a DAT2 style is roughly 10-12 hours; webphoto about 1.5 hours; ultrasharp under 15 minutes. The runner reported no out-of-memory retries at a 512 tile with fp16.

## Tiling seams

Seam score = how much the wrap seam stands out against neighbouring columns/rows (1 = invisible; `local/hd-spandrel-eval/analyse.py`). Over the 63 anzio textures whose originals tile:

| | median | over 2.5 |
|---|---|---|
| original | 1.01 | -- |
| ultrasharp | 1.12 | 8 of 61 |
| webphoto | 1.14 | 6 of 61 |
| pbrify | 1.14 | 6 of 61 |
| ultrasharpv2 | 1.14 | 7 of 61 |

The wrap padding works the same for every backend: the new styles tile as well as ultrasharp does. The handful over 2.5 are the same textures in every style (ones the padding can't fully help, such as `manifesto`, a poster).

## Masked textures

For all 24 `{` textures, the alpha channel is identical across the four styles: no partial alpha values anywhere, and the mask edge moved by the same fraction (0.0-1.4 % of pixels, from the bilinear resize + threshold of the original mask). That is expected: the scripts take the alpha from the original mask, not from the upscaler's output, so the backend cannot change it.

## Look

`local/hd-spandrel-eval/spandrel_vs_ultrasharp.png` (original, ultrasharp, webphoto, pbrify, ultrasharpv2; 384 px patches at 1:1) and `compare.png` (compare.py's full sheet, every style).

- All four are close. The differences are in texture grain, not in what is drawn.
- `ultrasharpv2` and `pbrify` (both DAT2) are a little cleaner and smoother than ultrasharp on plaster and stone, with less invented grain; on the brick wall and the poster they are near-identical to it.
- `webphoto` sits between the two: close to ultrasharp's sharpness with slightly less crunch.
- Skins: all three keep cloth folds and the face as ultrasharp does; none paints new detail.

## Verdict

The backend works and costs nothing when unused. None of the three models is a clear step up over ultrasharp on these textures, and the two DAT2 ones cost 60x the build time for a subtle smoothing. If a second style is wanted, `webphoto` is the reasonable one to keep built (1.5 h for the whole set); the DAT2 pair are for a single map someone wants to compare by eye, or for an overnight build.

The real limit is the 4x itself, not the model: a 256-pixel original gives every model the same amount to work from. The route to visibly different textures is a generative one (#424), not a better single-image upscaler. Diffusion-based restorers (SUPIR, StableSR, DiffBIR) are the "hallucinated detail" family: they invent texture rather than sharpen it, need far more GPU, and are noted here as future work, not wired in.

## Still to check live

- The game loads a spandrel style: `dodstudio_hd_style pbrify` in `movie.cfg`, play a dod_anzio demo, `dodstudio_debug_status` should count replacements as with ultrasharp. The files are the same names and format, so nothing in the hook changes, but it hasn't been watched.
