# Rebuilding a highlight in Blender (issue #403)

HLAE's `mirv_agr` records every drawn model's bones and the camera, every
frame, into an `.agr` file. These three scripts turn one into a rendered clip:
the recorded players, weapons and camera, inside the real map, with DoD
Studio's HD textures, the map's own sun, lights and skybox, rendered with
Blender's EEVEE or Cycles.

DoD Studio's **Blender** page runs them for you, one button per step. You can
also run them from a Command Prompt (not PowerShell: it needs `& "path"` for a
quoted program), as below.

## One-time setup

1. **Blender 4.4** (the portable zip is fine). The import add-ons below break
   on Blender 5.0.
2. **Blender Source Tools 3.4.3** and **afx-blender-scripts 1.14.6**: in
   Blender, Edit > Preferences > Add-ons > Install from Disk, then tick both.
3. **The game's models, decompiled with Crowbar**, into one folder
   (for example `C:\agr\assets`). Use Crowbar's Decompile tab:
   - MDL input: a folder;
   - "Folder for each model": on;
   - bone-animation SMDs: off.

   Crowbar's folder mode doesn't go into subfolders, so run it four times,
   keeping the path under `models`:
   - `dod\models` into `...\assets\models`;
   - `dod\models\player\axis-inf` into `...\assets\models\player\axis-inf`;
   - `dod\models\player\us-inf` into `...\assets\models\player\us-inf`;
   - `dod\models\mapmodels` into `...\assets\models\mapmodels`.

   The importer finds each model at
   `<assets>\models\<path>\<name>\<name>.qc`.

## Recording a take

Launch Preview from DoD Studio. Then in the console:

```
host_framerate 0.0333333
mirv_agr start "C:\agr\take.agr"
```

Play the highlight, then:

```
mirv_agr stop
host_framerate 0
```

`host_framerate` makes the recording an exact 30 fps.

- **Quote the path.** Unquoted, HLAE says it started but writes nothing.
- **The folder must already exist.**

## The steps

Each step writes into one work folder beside the take, `<take>_blender\`.

| Step | Script | Output |
| --- | --- | --- |
| 1. Import | `agr_import.py` | `imported.blend` |
| 2. Build scene | `agr_scene.py` | `textured.blend`, plus previews in `preview\` |
| 3. Render | `agr_render.py --mode quick` or `render` | `frames_720\` (720p) or `frames\` (1080p) PNGs |
| 4. Encode | `agr_render.py --mode encode` | `<take>_720.mp4` or `<take>.mp4` |

From a Command Prompt, with `B` set to your `blender.exe`:

```
"%B%" --python agr_import.py -- --agr C:\agr\take.agr --assets C:\agr\assets --out C:\agr\take_blender\imported.blend --quit
"%B%" -b C:\agr\take_blender\imported.blend --python agr_scene.py -- --game "C:\...\Half-Life - PRE-Anniversary for Movies" --agr C:\agr\take.agr --assets C:\agr\assets --map dod_anzio --out C:\agr\take_blender\textured.blend --preview C:\agr\take_blender\preview
"%B%" -b C:\agr\take_blender\textured.blend --python agr_render.py -- --mode quick --frames-dir C:\agr\take_blender\frames_720
"%B%" -b --python agr_render.py -- --mode encode --frames-dir C:\agr\take_blender\frames_720 --mp4 C:\agr\take_blender\take_720.mp4
```

Some notes on each step:

- **Import** needs Blender's window, because the importer wants a UI context. `--quit` closes Blender when it's done.
- **Build scene** options:
  - `--style <name>` picks an HD style from `dod\dodstudio_hd`. `none` uses the original textures. Without `--style`, it uses the first style built.
  - `--engine cycles` renders one 1080p Cycles frame instead of four EEVEE previews.
  - `--frames 180 510` picks which frames to preview.
- **Render** skips frames already on disk, so a stopped render carries on where it left off. `--start`, `--end` and `--samples` narrow it down.
- **Timing:** on an RTX 3070 Ti, a 1080p Cycles frame at 64 samples takes about 2 to 11 s, depending on the scene.

Each script prints `@@`-prefixed lines for DoD Studio (`@@saved`, `@@image`,
`@@frame`, `@@failed`), and exits non-zero on failure.

## Known limits

- **Body groups:** the recording has no body values, so every player shows the default kit. The scene step hides the other bodygroup meshes the importer brings in.
- **The viewmodel** is recorded as an ordinary model. The scene step pulls it toward the camera so walls can't cut into it.
- **Crowbar decompiles** of a few props (`dod_tree1`, `mill`) have stretched UVs.
- **Lighting:** lightmaps aren't imported. The map is lit by its `light_environment` sun, its `light` entities, and ambient light.

Issue #403 has the full notes and the follow-ups.
