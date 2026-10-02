"""Step 3 of the AGR pipeline (issue #403): render the textured scene to a PNG
sequence with Cycles, then encode it to an MP4 with Blender's own FFmpeg.

    blender -b <textured.blend> --python agr_render.py -- --mode render
        --frames-dir <folder> [--start N --end N] [--samples 64]
    blender -b <textured.blend> --python agr_render.py -- --mode quick
        --frames-dir <folder>                      (720p, 32 samples)
    blender -b --python agr_render.py -- --mode encode
        --frames-dir <folder> --mp4 <file.mp4> [--fps 30]

A frame whose PNG already exists is skipped, so a cancelled render picks up
where it stopped. `-b` (no window) renders faster.

Progress lines for DoD Studio: `@@frame <frame> <done> <total> <seconds>`,
then `@@saved <file>`.
"""
import argparse, bpy, os, sys, time, struct

_parser = argparse.ArgumentParser(prog="agr_render.py")
_parser.add_argument("--mode", choices=("render", "quick", "encode"), default="render")
_parser.add_argument("--frames-dir", required=True, help="where the PNG sequence goes (or is read from)")
_parser.add_argument("--mp4", default=None, help="encode: the video to write")
_parser.add_argument("--start", type=int, default=None, help="first frame (default: the take's first)")
_parser.add_argument("--end", type=int, default=None, help="last frame (default: the take's last)")
_parser.add_argument("--samples", type=int, default=None, help="Cycles samples (render: 64, quick: 32)")
_parser.add_argument("--fps", type=int, default=30, help="encode: frame rate (the take is recorded at 30)")
ARGS = _parser.parse_args(sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else [])

scene = bpy.context.scene


def setup_cycles(res, samples):
    scene.render.engine = 'CYCLES'
    scene.cycles.samples = samples
    scene.cycles.use_denoising = True
    scene.cycles.use_adaptive_sampling = True
    scene.cycles.adaptive_threshold = 0.05
    scene.cycles.max_bounces = 6
    scene.cycles.caustics_reflective = False
    scene.cycles.caustics_refractive = False
    scene.cycles.device = 'GPU'
    try:
        prefs = bpy.context.preferences.addons['cycles'].preferences
        for ct in ('OPTIX', 'CUDA'):
            try:
                prefs.compute_device_type = ct
                prefs.get_devices()
                for d in prefs.devices:
                    d.use = True
                print(f"cycles device: {ct}")
                break
            except Exception:
                continue
    except Exception as e:
        print(f"GPU setup failed, CPU: {e}")
    scene.render.resolution_x, scene.render.resolution_y = res
    scene.render.resolution_percentage = 100
    scene.render.image_settings.file_format = 'PNG'
    scene.render.image_settings.color_mode = 'RGB'
    scene.render.image_settings.color_depth = '8'
    scene.render.image_settings.compression = 15
    scene.render.use_persistent_data = True   # keep the scene on the GPU between frames
    scene.render.film_transparent = False


def render_frames(frames_dir, f0, f1, res, samples):
    os.makedirs(frames_dir, exist_ok=True)
    setup_cycles(res, samples)
    total = f1 - f0 + 1
    done = 0
    t_start = time.time()
    for f in range(f0, f1 + 1):
        out = os.path.join(frames_dir, f"frame_{f:04d}.png")
        if os.path.exists(out) and os.path.getsize(out) > 0:
            done += 1
            continue
        scene.frame_set(f)
        scene.render.filepath = out
        t = time.time()
        # silence Blender's per-sample console spam for the duration of the render call
        sys.stdout.flush()
        saved = os.dup(1); devnull = os.open(os.devnull, os.O_WRONLY)
        os.dup2(devnull, 1)
        try:
            bpy.ops.render.render(write_still=True)
        finally:
            os.dup2(saved, 1); os.close(saved); os.close(devnull)
        done += 1
        dt = time.time() - t
        elapsed = time.time() - t_start
        remaining = (total - done) * dt
        print(f"frame {f} ({done}/{total}) {dt:.1f}s  elapsed {elapsed/60:.1f} min  eta {remaining/60:.1f} min", flush=True)
        print(f"@@frame {f} {done} {total} {dt:.1f}", flush=True)
    print(f"@@saved {frames_dir}", flush=True)
    print("render done")


def encode(frames_dir, mp4, fps):
    files = sorted(f for f in os.listdir(frames_dir) if f.startswith("frame_") and f.endswith(".png"))
    if not files:
        raise SystemExit(f"no frames to encode in {frames_dir}")
    # fresh scene with a sequencer strip of the PNGs
    bpy.ops.wm.read_factory_settings(use_empty=True)
    sc = bpy.context.scene
    sc.render.fps = fps; sc.render.fps_base = 1.0
    sc.sequence_editor_create()
    first = os.path.join(frames_dir, files[0])
    strip = sc.sequence_editor.sequences.new_image("frames", first, channel=1, frame_start=1)
    for f in files[1:]:
        strip.elements.append(f)
    with open(first, "rb") as fh:
        fh.read(16); w, h = struct.unpack(">II", fh.read(8))
    sc.render.resolution_x, sc.render.resolution_y = w, h
    sc.render.resolution_percentage = 100
    sc.frame_start = 1; sc.frame_end = len(files)
    sc.render.image_settings.file_format = 'FFMPEG'
    sc.render.ffmpeg.format = 'MPEG4'
    sc.render.ffmpeg.codec = 'H264'
    sc.render.ffmpeg.constant_rate_factor = 'HIGH'
    sc.render.ffmpeg.ffmpeg_preset = 'GOOD'
    sc.render.ffmpeg.gopsize = 15
    sc.render.ffmpeg.audio_codec = 'NONE'
    os.makedirs(os.path.dirname(os.path.abspath(mp4)), exist_ok=True)
    sc.render.filepath = mp4
    bpy.ops.render.render(animation=True)
    print(f"encoded {len(files)} frames -> {mp4}")
    print(f"@@saved {mp4}", flush=True)


try:
    if ARGS.mode == "encode":
        if not ARGS.mp4:
            raise SystemExit("--mode encode needs --mp4")
        encode(ARGS.frames_dir, ARGS.mp4, ARGS.fps)
    else:
        quick = ARGS.mode == "quick"
        f0 = ARGS.start if ARGS.start is not None else scene.frame_start
        f1 = ARGS.end if ARGS.end is not None else scene.frame_end
        samples = ARGS.samples if ARGS.samples is not None else (32 if quick else 64)
        render_frames(ARGS.frames_dir, f0, f1, (1280, 720) if quick else (1920, 1080), samples)
except SystemExit as e:
    print(f"ERROR: {e}", flush=True)
    print("@@failed", flush=True)
    os._exit(1)
except Exception:
    import traceback
    print("ERROR:\n" + traceback.format_exc(), flush=True)
    print("@@failed", flush=True)
    os._exit(1)
