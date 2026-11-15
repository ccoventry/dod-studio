"""Step 1 of the AGR pipeline (issue #403): import a recorded .agr into a new
Blender scene with afx-blender-scripts and save it.

    blender --python agr_import.py -- --agr <take.agr> --assets <Crowbar assets>
        --out <imported.blend> [--quit]

Needs Blender 4.4 with Blender Source Tools 3.4.3 and afx-blender-scripts
1.14.6 installed (Edit > Preferences > Add-ons > Install from Disk). Runs
with a window: the importer needs a UI context, so no `-b`. `--quit` closes
Blender once the file is saved, which is how DoD Studio runs it.

Models resolve through Crowbar's decompile at
`<assets>/models/<path>/<name>/<name>.qc` (see README.md).

Progress lines for DoD Studio: `@@saved <file>`, or `@@failed`.
"""
import argparse, bpy, os, sys, time, traceback

_parser = argparse.ArgumentParser(prog="agr_import.py")
_parser.add_argument("--agr", required=True, help="the recorded .agr")
_parser.add_argument("--assets", required=True, help="Crowbar's decompile folder (holds models/)")
_parser.add_argument("--out", required=True, help="the .blend to save")
_parser.add_argument("--fps", type=int, default=30, help="the frame rate the take was recorded at")
_parser.add_argument("--quit", action="store_true", help="close Blender when done")
ARGS = _parser.parse_args(sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else [])

ADDONS = ("io_scene_valvesource", "advancedfx")


def log(msg):
    print(msg, flush=True)


def main():
    scene = bpy.context.scene
    # clean default scene
    for o in list(bpy.data.objects):
        bpy.data.objects.remove(o, do_unlink=True)

    scene.render.fps = ARGS.fps
    scene.render.fps_base = 1.0
    log(f"Blender {bpy.app.version_string}, scene fps {scene.render.fps}")

    import addon_utils
    missing = []
    for name in ADDONS:
        if not addon_utils.check(name)[1]:
            try:
                addon_utils.enable(name, default_set=True, persistent=True)
            except Exception as e:
                log(f"enable {name} failed: {e}")
        enabled = addon_utils.check(name)[1]
        log(f"addon {name}: {'enabled' if enabled else 'NOT ENABLED'}")
        if not enabled:
            missing.append(name)
    if missing:
        log(f"user addons dir: {bpy.utils.user_resource('SCRIPTS', path='addons')}")
        raise RuntimeError(f"add-ons not installed in this Blender: {missing} -- install Blender Source Tools "
                           "3.4.3 and afx-blender-scripts 1.14.6 (Edit > Preferences > Add-ons > Install from "
                           "Disk), tick them, then run this again")
    bpy.ops.wm.save_userpref()

    t0 = time.time()
    res = bpy.ops.advancedfx.agrimporter(
        filepath=ARGS.agr,
        assetPath=ARGS.assets,
        interKey=False,
        global_scale=0.0254,
        scaleInvisibleZero=True,
        bSkip=True,
        aSkip=True,
        onlyBones=False,
        modelInstancing=True,
        keyframeInterpolation='CONSTANT',
    )
    log(f"import result: {res} in {time.time()-t0:.1f}s")
    log(f"frame range: {scene.frame_start}..{scene.frame_end}")

    arm = [o for o in bpy.data.objects if o.type == 'ARMATURE']
    mesh = [o for o in bpy.data.objects if o.type == 'MESH']
    cams = [o for o in bpy.data.objects if o.type == 'CAMERA']
    log(f"objects: {len(bpy.data.objects)} total, {len(arm)} armatures, {len(mesh)} meshes, {len(cams)} cameras")

    cam = bpy.data.objects.get("afxCam") or (cams[0] if cams else None)
    if cam:
        scene.camera = cam
        log(f"scene camera: {cam.name}")
    else:
        log("WARNING: no camera imported")

    os.makedirs(os.path.dirname(os.path.abspath(ARGS.out)), exist_ok=True)
    bpy.ops.wm.save_as_mainfile(filepath=ARGS.out)
    log(f"saved {ARGS.out}")
    print(f"@@saved {ARGS.out}", flush=True)


failed = False
try:
    main()
except Exception:
    log("ERROR:\n" + traceback.format_exc())
    print("@@failed", flush=True)
    failed = True
if ARGS.quit:
    sys.stdout.flush()
    os._exit(1 if failed else 0)
