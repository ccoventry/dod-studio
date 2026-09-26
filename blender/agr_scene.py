"""Step 2 of the AGR pipeline (issue #403): texture the imported scene, build the
map from its BSP, light it and render previews.

Takes the .blend `agr_import.py` saved and adds:

- model textures: HD from `dod/dodstudio_hd/models/<style>/` (the hook's
  `<name>_<fnv1a32>.tga` naming), else Crowbar's BMPs, else the .mdl's own
  palette;
- the map: every brush model of the BSP, with HD world textures when built,
  its lights, its `light_environment` sun, and its skybox (HD when built);
- viewmodels pulled toward the camera so walls can't cut into them, and extra
  bodygroup meshes hidden (the recording carries no body values).

Then it saves a new .blend and renders either EEVEE previews or one Cycles
frame. Run it from DoD Studio's Blender page, or by hand:

    blender -b <imported.blend> --python agr_scene.py -- --game <Half-Life folder>
        --agr <take.agr> --assets <Crowbar assets> --map dod_anzio
        --out <textured.blend> --preview <folder> [--style ultrasharp]
        [--engine eevee|cycles] [--frames 180 510] [--samples 128]

Coordinates follow afx-blender-scripts: (x, y, z) -> (-y, x, z) * 0.0254.
HD naming follows goldsrc-hooks/tools/hd (file_stem_name + FNV-1a-32 of
pixels + palette).
"""
import argparse, bpy, bmesh, os, re, struct, sys, math, traceback, datetime
RUN_TAG = datetime.datetime.now().strftime('%Y%m%d-%H%M')
from mathutils import Vector

_parser = argparse.ArgumentParser(prog="agr_scene.py")
_parser.add_argument("--game", required=True, help="the Half-Life folder holding dod/")
_parser.add_argument("--agr", required=True, help="the recorded .agr")
_parser.add_argument("--assets", required=True, help="Crowbar's decompile folder (holds models/)")
_parser.add_argument("--map", required=True, help="map name, e.g. dod_anzio")
_parser.add_argument("--out", required=True, help="the textured .blend to save")
_parser.add_argument("--preview", required=True, help="folder for the preview renders")
_parser.add_argument("--style", default=None, help="HD style folder; default: the first one built")
_parser.add_argument("--engine", choices=("eevee", "cycles"), default="eevee")
_parser.add_argument("--frames", type=int, nargs="*", default=[],
                     help="frames to render; default: 4 spread over the take (eevee) or its last (cycles)")
_parser.add_argument("--samples", type=int, default=128, help="Cycles samples")
_parser.add_argument("--log", default=None, help="also write the log here")
ARGS = _parser.parse_args(sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else [])

GAME    = ARGS.game
AGR     = ARGS.agr
ASSETS  = ARGS.assets
OUT     = ARGS.out
PREVIEW = ARGS.preview
LOG     = ARGS.log
SCALE   = 0.0254
N_PREVIEW = 4

MAP    = ARGS.map
STYLE  = ARGS.style
ENGINE = ARGS.engine
_ints = ARGS.frames
CYCLES_FRAME = _ints[0] if _ints else None   # None: the take's last frame
CYCLES_SAMPLES = ARGS.samples

# GoldSrc skybox faces, as seen from inside the cube, in GAME coordinates (+X forward at yaw 0,
# +Y left, +Z up). Each entry: face suffix -> (top-left, top-right, bottom-right, bottom-left).
# If a rendered sky face looks rotated or swapped, fix it here.
S_ = 1.0
SKY_FACES = {
    "rt": ((S_,  S_,  S_), (S_, -S_,  S_), (S_, -S_, -S_), (S_,  S_, -S_)),   # +X
    "lf": ((-S_, -S_, S_), (-S_, S_,  S_), (-S_, S_, -S_), (-S_, -S_, -S_)),  # -X
    "bk": ((-S_, S_,  S_), (S_,  S_,  S_), (S_,  S_, -S_), (-S_, S_, -S_)),   # +Y
    "ft": ((S_, -S_,  S_), (-S_, -S_, S_), (-S_, -S_, -S_), (S_, -S_, -S_)),  # -Y
    "up": ((S_,  S_,  S_), (S_, -S_,  S_), (-S_, -S_, S_), (-S_, S_,  S_)),   # +Z
    "dn": ((S_,  S_, -S_), (S_, -S_, -S_), (-S_, -S_, -S_), (-S_, S_, -S_)),  # -Z
}

log_lines = []
def log(m):
    print(m); log_lines.append(str(m))

# ── goldsrc helpers (ported from goldsrc-hooks/tools/hd) ─────────────────────
SKIP = {"aaatrigger", "clip", "origin", "null", "skip", "hint", "bevel", "sky", "black"}

def fnv1a32(*parts):
    h = 0x811C9DC5
    for p in parts:
        for b in p:
            h = ((h ^ b) * 0x01000193) & 0xFFFFFFFF
    return h

def file_stem_name(name):
    return "".join("_" if c in '<>:"/\\|?*' or ord(c) < 32 else c.lower() for c in name)

def read_miptex(buf, at):
    name = buf[at:at + 16].split(b"\0")[0].decode("latin1")
    w, h = struct.unpack_from("<II", buf, at + 16)
    pix = w * h
    idx = buf[at + 40: at + 40 + pix]
    pal_at = at + 40 + (pix >> 6) * 85 + 2
    return name, w, h, bytes(idx), bytes(buf[pal_at: pal_at + 768])

_wads = {}
def wad_lookup(path):
    if path not in _wads:
        entries = {}
        with open(path, "rb") as f:
            hdr = f.read(12)
            if hdr[:4] == b"WAD3":
                n, diro = struct.unpack_from("<ii", hdr, 4)
                f.seek(diro); d = f.read(32 * n)
                for i in range(n):
                    off, disk = struct.unpack_from("<ii", d, 32 * i)
                    nm = d[32 * i + 16: 32 * i + 32].split(b"\0")[0].decode("latin1").lower()
                    entries[nm] = (off, disk)
        _wads[path] = entries
    return _wads[path]

def mdl_textures(path):
    d = open(path, "rb").read()
    if d[:4] != b"IDST":
        return []
    count, index = struct.unpack_from("<ii", d, 0xB4)
    if count == 0 or index == 0:
        t = path[:-4] + "T.mdl"
        return mdl_textures(t) if os.path.exists(t) and t != path else []
    out = []
    for i in range(count):
        at = index + 80 * i
        name = d[at:at + 64].split(b"\0")[0].decode("latin1")
        flags, w, h, off = struct.unpack_from("<iiii", d, at + 64)
        if w <= 0 or h <= 0 or off + w * h + 768 > len(d):
            continue
        out.append((name, flags, w, h, bytes(d[off:off + w * h]), bytes(d[off + w * h:off + w * h + 768])))
    return out

def hd_root():
    return os.path.join(GAME, "dod", "dodstudio_hd")

def pick_style():
    global STYLE
    base = os.path.join(hd_root(), "models")
    if STYLE:
        return STYLE
    if os.path.isdir(base):
        for s in ("ultrasharp", "remacri", "siax", "generalv3", "x4plus", "plain"):
            if os.path.isdir(os.path.join(base, s)):
                STYLE = s; return s
        subs = [s for s in os.listdir(base) if os.path.isdir(os.path.join(base, s))]
        if subs:
            STYLE = subs[0]; return STYLE
    return None

def image_from_indexed(name, w, h, idx, pal, masked):
    """Build a Blender image from 8-bit indexed pixels (fallback when no HD/BMP file)."""
    img = bpy.data.images.new(name, w, h, alpha=True)
    px = [0.0] * (w * h * 4)
    for y in range(h):
        row = (h - 1 - y) * w  # flip vertically
        for x in range(w):
            i = idx[y * w + x]
            o = (row + x) * 4
            if masked and i == 255:
                px[o:o + 4] = (0, 0, 0, 0)
            else:
                px[o] = pal[i * 3] / 255; px[o + 1] = pal[i * 3 + 1] / 255; px[o + 2] = pal[i * 3 + 2] / 255; px[o + 3] = 1.0
    img.pixels = px
    img.pack()
    return img

_images = {}
def load_image(path):
    if path not in _images:
        img = bpy.data.images.load(path, check_existing=True)
        img.colorspace_settings.name = 'sRGB'
        _images[path] = img
    return _images[path]

def make_material(name, img, masked):
    mat = bpy.data.materials.new(name)
    mat.use_nodes = True
    nt = mat.node_tree
    bsdf = nt.nodes.get("Principled BSDF")
    tex = nt.nodes.new("ShaderNodeTexImage")
    tex.image = img
    tex.interpolation = 'Linear'
    tex.location = (-400, 300)
    nt.links.new(tex.outputs["Color"], bsdf.inputs["Base Color"])
    bsdf.inputs["Roughness"].default_value = 0.9
    bsdf.inputs["Specular IOR Level"].default_value = 0.2
    if masked:
        nt.links.new(tex.outputs["Alpha"], bsdf.inputs["Alpha"])
        mat.blend_method = 'CLIP' if hasattr(mat, "blend_method") else mat.blend_method
    return mat

# ── 1. model materials ───────────────────────────────────────────────────────
def agr_model_paths():
    """basename -> full model path, from the .agr's dictionary strings."""
    d = open(AGR, "rb").read()
    names = set(m.group(0).decode() for m in re.finditer(rb"models/[A-Za-z0-9_\-./]+\.mdl", d))
    return {os.path.basename(n).lower(): n for n in names}

def find_mdl(rel):
    for g in ("dod", "dod_downloads", "valve"):
        p = os.path.join(GAME, g, rel.replace("/", os.sep))
        if os.path.exists(p):
            return p
    return None

def hide_extra_bodygroups(arm, qc_dir, base):
    """The importer brings in every $bodygroup option; the game shows one (body 0 by default).
    Hide all but the first studio of each bodygroup until body values are recorded."""
    if not qc_dir:
        return
    qc = os.path.join(qc_dir, os.path.splitext(base)[0] + ".qc")
    if not os.path.exists(qc):
        return
    text = open(qc, encoding="latin1", errors="replace").read()
    hide = set()
    for m in re.finditer(r"\$bodygroup\s+\"?[^\s\"]+\"?\s*\{(.*?)\}", text, re.S | re.I):
        studios = re.findall(r"studio\s+\"?([^\s\"]+)\"?", m.group(1), re.I)
        for sname in studios[1:]:
            hide.add(os.path.splitext(os.path.basename(sname))[0].lower())
    if not hide:
        return
    n = 0
    for mesh in [c for c in arm.children if c.type == 'MESH']:
        dn = re.sub(r"\.\d{3}$", "", mesh.data.name).lower()
        on = re.sub(r"\.\d{3}$", "", mesh.name).lower()
        if dn in hide or on in hide or any(on.endswith(h) for h in hide):
            mesh.hide_render = True; mesh.hide_viewport = True; n += 1
    if n:
        log(f"  {base}: hid {n} extra bodygroup mesh(es)")

LIGHT_WATTS_PER_UNIT = 1.5   # GoldSrc light brightness (default 200) -> Blender watts (200 -> 300 W)
VIEWMODEL_SHRINK = 0.08   # scale v_ models toward the eye so walls can't intersect them (view-invariant)

def _fcurves(obj, path):
    ad = obj.animation_data
    if not ad or not ad.action:
        return [None, None, None]
    out = [None, None, None]
    for fc in ad.action.fcurves:
        if fc.data_path == path and 0 <= fc.array_index < 3:
            out[fc.array_index] = fc
    return out

def shrink_viewmodels():
    """Scale v_ models about the CAMERA position (view-invariant), so walls can't intersect them.
    location' = cam + (location - cam) * k, scale' = scale * k, per keyframe."""
    cam = bpy.data.objects.get("afxCam") or next((o for o in bpy.data.objects if o.type == 'CAMERA'), None)
    if not cam:
        log("no camera; viewmodels left alone"); return
    cam_fc = _fcurves(cam, "location")
    def cam_at(f):
        return [cam_fc[i].evaluate(f) if cam_fc[i] else cam.location[i] for i in range(3)]
    k = VIEWMODEL_SHRINK
    n = 0
    for arm in [o for o in bpy.data.objects if o.type == 'ARMATURE']:
        if not re.match(r"afx\.\d+ v_.*\.mdl$", arm.name):
            continue
        loc_fc = _fcurves(arm, "location")
        if all(loc_fc):
            frames = sorted({kp.co[0] for fc in loc_fc for kp in fc.keyframe_points})
            for f in frames:
                c = cam_at(f)
                old = [loc_fc[i].evaluate(f) for i in range(3)]
                new = [c[i] + (old[i] - c[i]) * k for i in range(3)]
                for i in range(3):
                    for kp in loc_fc[i].keyframe_points:
                        if kp.co[0] == f:
                            d = new[i] - kp.co[1]
                            kp.co[1] += d; kp.handle_left[1] += d; kp.handle_right[1] += d
            for fc in loc_fc:
                fc.update()
        else:
            c = cam.location
            arm.location = [c[i] + (arm.location[i] - c[i]) * k for i in range(3)]
        sc_fc = _fcurves(arm, "scale")
        if all(sc_fc):
            for fc in sc_fc:
                for kp in fc.keyframe_points:
                    kp.co[1] *= k; kp.handle_left[1] *= k; kp.handle_right[1] *= k
                fc.update()
        else:
            arm.scale = [v * k for v in arm.scale]
        n += 1
    log(f"viewmodels shrunk toward camera: {n}")

def texture_models():
    style = pick_style()
    hd_models = os.path.join(hd_root(), "models", style) if style else None
    log(f"HD style: {style}  ({hd_models if hd_models and os.path.isdir(hd_models) else 'no HD model folder, using BMP fallback'})")
    paths = agr_model_paths()
    stats = {"hd": 0, "bmp": 0, "indexed": 0, "missing": 0}
    mat_cache = {}   # (mdl basename, texname) -> material

    for arm in [o for o in bpy.data.objects if o.type == 'ARMATURE']:
        m = re.match(r"afx\.\d+ (.+\.mdl)$", arm.name)
        if not m:
            continue
        base = m.group(1).lower()
        rel = paths.get(base)
        mdl = find_mdl(rel) if rel else None
        texs = {t[0].lower(): t for t in mdl_textures(mdl)} if mdl else {}
        qc_dir = os.path.join(ASSETS, os.path.splitext(rel)[0].replace("/", os.sep)) if rel else None
        hide_extra_bodygroups(arm, qc_dir, base)

        for mesh in [c for c in arm.children if c.type == 'MESH']:
            for slot in mesh.material_slots:
                if not slot.material:
                    continue
                mname = slot.material.name
                if ".mdl:" in mname:
                    continue  # shared mesh data, already done
                key = mname.lower()
                key = re.sub(r"\.\d{3}$", "", key)          # Blender's .001 suffix
                if not key.endswith(".bmp"):
                    key_bmp = key + ".bmp"
                else:
                    key_bmp = key
                t = texs.get(key_bmp) or texs.get(key)
                ck = (base, key_bmp)
                if ck in mat_cache:
                    slot.material = mat_cache[ck]; continue

                img = None; masked = False
                if t:
                    name, flags, w, h, idx, pal = t
                    masked = bool(flags & 0x40)
                    if hd_models:
                        hp = os.path.join(hd_models, f"{file_stem_name(name)}_{fnv1a32(idx, pal):08x}.tga")
                        if os.path.exists(hp):
                            img = load_image(hp); stats["hd"] += 1
                if img is None and qc_dir:
                    bp = os.path.join(qc_dir, key_bmp)
                    if os.path.exists(bp):
                        img = load_image(bp); stats["bmp"] += 1
                if img is None and t:
                    img = image_from_indexed(f"{base}:{key_bmp}", w, h, idx, pal, masked); stats["indexed"] += 1
                if img is None:
                    stats["missing"] += 1
                    log(f"  no texture for {base} / {mname}")
                    continue
                mat = make_material(f"{base}:{key_bmp}", img, masked)
                mat_cache[ck] = mat
                slot.material = mat
    log(f"model materials: {stats}")

# ── 2. map import ────────────────────────────────────────────────────────────
def bsp_path(m):
    for g in ("dod", "dod_downloads"):
        p = os.path.join(GAME, g, "maps", m + ".bsp")
        if os.path.exists(p):
            return p
    return None

def parse_entities(text):
    ents = []
    for block in re.finditer(r"\{(.*?)\}", text, re.S):
        e = dict(re.findall(r'"([^"]*)"\s+"([^"]*)"', block.group(1)))
        ents.append(e)
    return ents

def import_bsp(m):
    p = bsp_path(m)
    if not p:
        # Fatal: a take rendered in empty space is not what anyone asked for.
        raise SystemExit(f"map not found: {m}.bsp is in neither dod/maps nor dod_downloads/maps under {GAME}")
    d = open(p, "rb").read()
    ver = struct.unpack_from("<i", d, 0)[0]
    lumps = [struct.unpack_from("<ii", d, 4 + 8 * i) for i in range(15)]
    L = lambda i: d[lumps[i][0]: lumps[i][0] + lumps[i][1]]
    log(f"BSP {p} version {ver}")

    ents = parse_entities(L(0).decode("latin1"))
    world = ents[0] if ents else {}
    wads = [os.path.basename(w.replace("\\", "/")) for w in world.get("wad", "").split(";") if w]
    skyname = world.get("skyname", "desert")

    # textures
    tl = L(2)
    ntex = struct.unpack_from("<i", tl, 0)[0]
    textures = []
    for i in range(ntex):
        off = struct.unpack_from("<i", tl, 4 + 4 * i)[0]
        if off < 0:
            textures.append(None); continue
        name = tl[off:off + 16].split(b"\0")[0].decode("latin1")
        w, h = struct.unpack_from("<II", tl, off + 16)
        if struct.unpack_from("<I", tl, off + 24)[0]:
            textures.append(read_miptex(tl, off)); continue
        found = None
        for wname in wads:
            for g in ("dod", "dod_downloads", "valve"):
                wp = os.path.join(GAME, g, wname)
                if os.path.exists(wp) and name.lower() in wad_lookup(wp):
                    o, disk = wad_lookup(wp)[name.lower()]
                    with open(wp, "rb") as f:
                        f.seek(o); found = read_miptex(f.read(disk), 0)
                    break
            if found: break
        textures.append(found or (name, w, h, b"", b""))

    verts = [struct.unpack_from("<fff", L(3), 12 * i) for i in range(lumps[3][1] // 12)]
    tib = L(6)
    texinfo = [struct.unpack_from("<8f i i", tib, 40 * i) for i in range(lumps[6][1] // 40)]
    fb = L(7)
    faces = [struct.unpack_from("<HHihh4Bi", fb, 20 * i) for i in range(lumps[7][1] // 20)]
    eb = L(12)
    edges = [struct.unpack_from("<HH", eb, 4 * i) for i in range(lumps[12][1] // 4)]
    seb = L(13)
    surfedges = [struct.unpack_from("<i", seb, 4 * i)[0] for i in range(lumps[13][1] // 4)]
    mb = L(14)
    models = [struct.unpack_from("<9f 4i i i i", mb, 64 * i) for i in range(lumps[14][1] // 64)]

    style = STYLE
    hd_world = os.path.join(hd_root(), "world", style) if style else None
    mats = {}
    stats = {"hd": 0, "indexed": 0, "skipped": 0}
    def material_for(ti):
        t = textures[ti] if ti < len(textures) else None
        if not t: return None
        name, w, h, idx, pal = t
        lname = name.lower()
        if lname in SKIP or lname.startswith("sky"):
            return None
        if ti in mats: return mats[ti]
        masked = name.startswith("{")
        img = None
        if hd_world and idx and pal:
            hp = os.path.join(hd_world, f"{file_stem_name(name)}_{fnv1a32(idx, pal):08x}.tga")
            if os.path.exists(hp):
                img = load_image(hp); stats["hd"] += 1
        if img is None and idx and pal:
            img = image_from_indexed(f"map:{name}", w, h, idx, pal, masked); stats["indexed"] += 1
        if img is None:
            mat = bpy.data.materials.new(f"map:{name}")
        else:
            mat = make_material(f"map:{name}", img, masked)
        mats[ti] = mat
        return mat

    coll = bpy.data.collections.new(f"map {m}")
    bpy.context.scene.collection.children.link(coll)

    # entity models: "*N" -> origin
    model_origin = {}
    for e in ents[1:]:
        mdl = e.get("model", "")
        if mdl.startswith("*"):
            o = [float(x) for x in e.get("origin", "0 0 0").split()]
            model_origin[int(mdl[1:])] = (o + [0, 0, 0])[:3]

    def xf(v):
        return Vector((-v[1], v[0], v[2])) * SCALE

    for mi, mdl in enumerate(models):
        firstface, numfaces = mdl[14], mdl[15]
        if numfaces == 0: continue
        origin = model_origin.get(mi, (0, 0, 0)) if mi else (0, 0, 0)
        bm = bmesh.new()
        uv_layer = bm.loops.layers.uv.new("UVMap")
        mesh = bpy.data.meshes.new(f"{m}_{mi}")
        mat_index = {}
        for fi in range(firstface, firstface + numfaces):
            f = faces[fi]
            firstedge, numedges, ti = f[2], f[3], f[4]
            tinfo = texinfo[ti]
            mi_tex = tinfo[8]
            mat = material_for(mi_tex)
            if mat is None:
                stats["skipped"] += 1; continue
            if mat.name not in mat_index:
                mesh.materials.append(mat); mat_index[mat.name] = len(mesh.materials) - 1
            tex = textures[mi_tex]
            tw, th = (tex[1] or 1), (tex[2] or 1)
            bverts = []
            for k in range(numedges):
                se = surfedges[firstedge + k]
                vi = edges[se][0] if se >= 0 else edges[-se][1]
                bverts.append(verts[vi])
            # drop duplicate consecutive verts
            uniq = []
            for v in bverts:
                if not uniq or uniq[-1] != v:
                    uniq.append(v)
            if len(uniq) > 1 and uniq[0] == uniq[-1]:
                uniq.pop()
            if len(uniq) < 3: continue
            bmv = [bm.verts.new(xf((v[0] + origin[0], v[1] + origin[1], v[2] + origin[2]))) for v in uniq]
            try:
                face = bm.faces.new(bmv)
            except ValueError:
                continue
            face.material_index = mat_index[mat.name]
            face.normal_flip()  # GoldSrc winding is clockwise from outside
            for loop in face.loops:
                # UV from the corner's own position (model-local game coords), so loop order doesn't matter
                co = loop.vert.co
                gx = co.y / SCALE - origin[0]
                gy = -co.x / SCALE - origin[1]
                gz = co.z / SCALE - origin[2]
                u = (gx * tinfo[0] + gy * tinfo[1] + gz * tinfo[2] + tinfo[3]) / tw
                vv = (gx * tinfo[4] + gy * tinfo[5] + gz * tinfo[6] + tinfo[7]) / th
                loop[uv_layer].uv = (u, 1.0 - vv)
        bmesh.ops.remove_doubles(bm, verts=bm.verts, dist=0.0001)
        bm.to_mesh(mesh); bm.free()
        ob = bpy.data.objects.new(mesh.name, mesh)
        coll.objects.link(ob)
    log(f"map materials: {stats}; brush models: {len(models)}; sky: {skyname}")
    # point / spot lights from the entity lump
    lcoll = bpy.data.collections.new(f"lights {m}")
    bpy.context.scene.collection.children.link(lcoll)
    nl = 0
    for e in ents:
        cn = e.get("classname", "")
        if cn not in ("light", "light_spot"):
            continue
        try:
            o = [float(x) for x in e.get("origin", "0 0 0").split()]
            col = [float(x) for x in e.get("_light", "255 255 255 200").split()]
        except ValueError:
            continue
        brightness = col[3] if len(col) > 3 else 200.0
        mx = max(col[:3]) or 255.0
        ld = bpy.data.lights.new(f"{cn}_{nl}", 'SPOT' if cn == "light_spot" else 'POINT')
        ld.color = (col[0] / mx, col[1] / mx, col[2] / mx)
        ld.energy = brightness * LIGHT_WATTS_PER_UNIT
        ld.shadow_soft_size = 0.15
        if cn == "light_spot":
            ld.spot_size = math.radians(float(e.get("_cone2", e.get("_cone", "60"))) * 2)
            ld.spot_blend = 0.5
        lo = bpy.data.objects.new(ld.name, ld)
        lo.location = xf(o)
        if cn == "light_spot":
            ang = [float(x) for x in e.get("angles", "0 0 0").split()]
            pitch = float(e.get("pitch", ang[0])); yaw = ang[1]
            rp, ry = math.radians(pitch), math.radians(yaw)
            dg = (math.cos(rp) * math.cos(ry), math.cos(rp) * math.sin(ry), math.sin(rp))
            lo.rotation_euler = Vector((-dg[1], dg[0], dg[2])).to_track_quat('-Z', 'Y').to_euler()
        lcoll.objects.link(lo)
        nl += 1
    log(f"map lights: {nl}")
    sun = None
    for e in ents:
        if e.get("classname") == "light_environment":
            ang = [float(x) for x in e.get("angles", "0 0 0").split()]
            pitch = float(e.get("pitch", ang[0]))
            yaw = ang[1]
            col = [float(x) for x in e.get("_light", "255 255 255 200").split()]
            amb = [float(x) for x in e.get("_diffuse_light", "0 0 0 0").split()]
            sun = (yaw, pitch, col, amb)
            log(f"light_environment: yaw {yaw} pitch {pitch} light {col}")
            break
    return skyname, sun

# ── 3. lighting + render ─────────────────────────────────────────────────────
def setup_lighting(sun_info=None):
    scene = bpy.context.scene
    for o in bpy.data.objects:
        if o.type == 'CAMERA':
            o.data.clip_start = 0.005
            o.data.clip_end = 6000.0
    sun = bpy.data.lights.new("Sun", 'SUN'); sun.energy = 4.0; sun.angle = math.radians(2)
    so = bpy.data.objects.new("Sun", sun)
    scene.collection.objects.link(so)
    if sun_info:
        yaw, pitch, col, amb = sun_info
        ry, rp = math.radians(yaw), math.radians(pitch)
        # direction the light travels, in game coords, then to Blender coords (-y, x, z)
        dg = (math.cos(rp) * math.cos(ry), math.cos(rp) * math.sin(ry), math.sin(rp))
        db = Vector((-dg[1], dg[0], dg[2]))
        so.rotation_euler = db.to_track_quat('-Z', 'Y').to_euler()
        mx = max(col[:3]) or 255.0
        sun.color = (col[0] / mx, col[1] / mx, col[2] / mx)
        sun.energy = 3.0 + 3.0 * min(col[3] if len(col) > 3 else 200, 400) / 400
    else:
        so.rotation_euler = (math.radians(50), 0, math.radians(35))
    world = scene.world or bpy.data.worlds.new("World")
    scene.world = world
    world.use_nodes = True
    bg = world.node_tree.nodes.get("Background")
    amb = sun_info[3] if sun_info and len(sun_info) > 3 else [0, 0, 0, 0]
    if bg:
        if amb and max(amb[:3]) > 0:
            mx = max(amb[:3])
            bg.inputs[0].default_value = (amb[0] / mx, amb[1] / mx, amb[2] / mx, 1)
            bg.inputs[1].default_value = 0.6 + 0.9 * min(amb[3] if len(amb) > 3 else 200, 400) / 400
        else:
            bg.inputs[0].default_value = (0.55, 0.65, 0.8, 1); bg.inputs[1].default_value = 0.8
    scene.view_settings.view_transform = 'AgX'
    scene.view_settings.exposure = 0.5
    for eng in ('BLENDER_EEVEE_NEXT', 'BLENDER_EEVEE'):
        try:
            scene.render.engine = eng; break
        except TypeError:
            continue

def render_previews():
    scene = bpy.context.scene
    os.makedirs(PREVIEW, exist_ok=True)
    scene.render.resolution_x, scene.render.resolution_y = 1280, 720
    scene.render.image_settings.file_format = 'PNG'
    f0, f1 = scene.frame_start, scene.frame_end
    frames = _ints if _ints else [int(f0 + (f1 - f0) * i / (N_PREVIEW - 1)) for i in range(N_PREVIEW)]
    for f in frames:
        scene.frame_set(f)
        scene.render.filepath = os.path.join(PREVIEW, f"frame_{f:04d}_{RUN_TAG}.png")
        bpy.ops.render.render(write_still=True)
        log(f"rendered {f}")
        print(f"@@image {scene.render.filepath}", flush=True)

def sky_face_path(skyname, face):
    """HD sky face if present, else the stock one from dod/ or valve/."""
    if STYLE:
        hp = os.path.join(hd_root(), "sky", STYLE, f"{skyname}{face}.tga")
        if os.path.exists(hp):
            return hp
    for g in ("dod", "dod_downloads", "valve"):
        for ext in (".tga", ".bmp"):
            p = os.path.join(GAME, g, "gfx", "env", f"{skyname}{face}{ext}")
            if os.path.exists(p):
                return p
    return None

def build_skybox(skyname):
    scene = bpy.context.scene
    cam = scene.camera
    size = 4000.0  # metres; well outside any map
    found = 0
    bm = bmesh.new()
    uv_layer = bm.loops.layers.uv.new("UVMap")
    mesh = bpy.data.meshes.new("skybox")
    for face, corners in SKY_FACES.items():
        p = sky_face_path(skyname, face)
        if not p:
            log(f"  sky face missing: {skyname}{face}"); continue
        img = load_image(p)
        mat = bpy.data.materials.new(f"sky:{skyname}{face}")
        mat.use_nodes = True
        nt = mat.node_tree
        for n in list(nt.nodes):
            nt.nodes.remove(n)
        out = nt.nodes.new("ShaderNodeOutputMaterial")
        em = nt.nodes.new("ShaderNodeEmission"); em.inputs["Strength"].default_value = 1.0
        tex = nt.nodes.new("ShaderNodeTexImage"); tex.image = img; tex.extension = 'EXTEND'
        nt.links.new(tex.outputs["Color"], em.inputs["Color"])
        nt.links.new(em.outputs["Emission"], out.inputs["Surface"])
        mesh.materials.append(mat)
        mi = len(mesh.materials) - 1
        verts = [bm.verts.new(Vector((-c[1], c[0], c[2])) * size) for c in corners]
        f = bm.faces.new(verts)
        f.material_index = mi
        # inward-facing: corners are listed as seen from inside, so flip to face the camera
        f.normal_flip()
        uvs = [(0, 1), (1, 1), (1, 0), (0, 0)]
        for loop in f.loops:
            k = verts.index(loop.vert)
            loop[uv_layer].uv = uvs[k]
        found += 1
    bm.to_mesh(mesh); bm.free()
    ob = bpy.data.objects.new("skybox", mesh)
    scene.collection.objects.link(ob)
    # follow the camera so the sky is "infinitely far"; never lights or shadows the scene
    con = ob.constraints.new('COPY_LOCATION'); con.target = cam
    ob.visible_diffuse = False; ob.visible_glossy = False; ob.visible_shadow = False
    ob.visible_transmission = False; ob.visible_volume_scatter = False
    log(f"skybox {skyname}: {found}/6 faces")

def render_cycles_frame():
    scene = bpy.context.scene
    scene.render.engine = 'CYCLES'
    scene.cycles.samples = CYCLES_SAMPLES
    scene.cycles.use_denoising = True
    try:
        scene.cycles.device = 'GPU'
        prefs = bpy.context.preferences.addons['cycles'].preferences
        for ct in ('OPTIX', 'CUDA'):
            try:
                prefs.compute_device_type = ct; prefs.get_devices()
                for d in prefs.devices: d.use = True
                log(f"cycles device: {ct}"); break
            except Exception:
                continue
    except Exception as e:
        log(f"cycles GPU setup failed, using CPU: {e}")
    scene.render.resolution_x, scene.render.resolution_y = 1920, 1080
    scene.render.resolution_percentage = 100
    scene.render.use_multiview = False
    scene.render.image_settings.file_format = 'PNG'
    scene.render.image_settings.color_mode = 'RGB'
    scene.render.image_settings.color_depth = '8'
    scene.render.image_settings.compression = 15
    os.makedirs(PREVIEW, exist_ok=True)
    frame = CYCLES_FRAME if CYCLES_FRAME is not None else scene.frame_end
    scene.frame_set(frame)
    out = os.path.join(PREVIEW, f"cycles_{frame:04d}_{RUN_TAG}.png")
    scene.render.filepath = out
    try:
        bpy.ops.render.render(write_still=True)
    except RuntimeError as e:
        log(f"write_still failed ({e}); saving Render Result directly")
        bpy.data.images['Render Result'].save_render(filepath=out, scene=scene)
    log(f"cycles rendered {frame} -> {out}")
    print(f"@@image {out}", flush=True)

try:
    if not MAP:
        raise SystemExit("usage: see the docstring at the top of agr_scene.py")
    texture_models()
    shrink_viewmodels()
    skyname, sun_info = import_bsp(MAP) or (None, None)
    setup_lighting(sun_info)
    if skyname:
        build_skybox(skyname)
    os.makedirs(os.path.dirname(os.path.abspath(OUT)), exist_ok=True)
    bpy.ops.wm.save_as_mainfile(filepath=OUT)
    log(f"saved {OUT}")
    print(f"@@saved {OUT}", flush=True)
    if ENGINE == "cycles":
        render_cycles_frame()
    else:
        render_previews()
except SystemExit as e:
    log(str(e))
    FAILED = True
except Exception:
    log("ERROR:\n" + traceback.format_exc())
    FAILED = True
finally:
    if LOG:
        with open(LOG, "w", encoding="utf-8") as fh:
            fh.write("\n".join(log_lines))
    # A failure has to reach whoever ran this: Blender itself exits 0.
    if globals().get("FAILED"):
        print("@@failed", flush=True)
        sys.stdout.flush()
        os._exit(1)
