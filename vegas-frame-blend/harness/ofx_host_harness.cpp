// A minimal OpenFX host for testing Frame Blend without VEGAS: loads the .ofx,
// describes it in the filter context, creates an instance and renders frames
// from a synthetic clip (a white square moving 2 px per captured frame). Each
// scenario models one thing a host might do with the in-between times the
// plugin asks for, and checks the output against an average the harness works
// out on its own.
//
// Usage: ofx_host_harness.exe <path to FrameBlend.ofx>
// Exit code 0 when every exact check passes.

#include "ofxImageEffect.h"

#include <algorithm>
#include <chrono>
#include <cmath>
#include <cstddef>
#include <cstdarg>
#include <cstdio>
#include <cstring>
#include <functional>
#include <map>
#include <memory>
#include <string>
#include <vector>

#define WIN32_LEAN_AND_MEAN
#include <windows.h>

// ---------------------------------------------------------------- properties

struct Prop {
    char type = 0;  // 'i' 'd' 's' 'p'
    std::vector<int> i;
    std::vector<double> d;
    std::vector<std::string> s;
    std::vector<void *> p;
    int dim() const {
        switch (type) {
        case 'i': return (int)i.size();
        case 'd': return (int)d.size();
        case 's': return (int)s.size();
        case 'p': return (int)p.size();
        }
        return 0;
    }
};

struct PropSet {
    std::map<std::string, Prop> m;
};

static PropSet *PS(OfxPropertySetHandle h) { return reinterpret_cast<PropSet *>(h); }
static OfxPropertySetHandle H(PropSet *p) { return reinterpret_cast<OfxPropertySetHandle>(p); }

template <typename V> static Prop &prop_for_set(OfxPropertySetHandle h, const char *name, char type) {
    Prop &p = PS(h)->m[name];
    p.type = type;
    return p;
}

static OfxStatus setPointer(OfxPropertySetHandle h, const char *n, int idx, void *v) {
    Prop &p = prop_for_set<void *>(h, n, 'p');
    if ((int)p.p.size() <= idx) p.p.resize(idx + 1);
    p.p[idx] = v;
    return kOfxStatOK;
}
static OfxStatus setString(OfxPropertySetHandle h, const char *n, int idx, const char *v) {
    Prop &p = prop_for_set<std::string>(h, n, 's');
    if ((int)p.s.size() <= idx) p.s.resize(idx + 1);
    p.s[idx] = v ? v : "";
    return kOfxStatOK;
}
static OfxStatus setDouble(OfxPropertySetHandle h, const char *n, int idx, double v) {
    Prop &p = prop_for_set<double>(h, n, 'd');
    if ((int)p.d.size() <= idx) p.d.resize(idx + 1);
    p.d[idx] = v;
    return kOfxStatOK;
}
static OfxStatus setInt(OfxPropertySetHandle h, const char *n, int idx, int v) {
    Prop &p = prop_for_set<int>(h, n, 'i');
    if ((int)p.i.size() <= idx) p.i.resize(idx + 1);
    p.i[idx] = v;
    return kOfxStatOK;
}
static OfxStatus setPointerN(OfxPropertySetHandle h, const char *n, int c, void *const *v) {
    for (int k = 0; k < c; ++k) setPointer(h, n, k, v[k]);
    return kOfxStatOK;
}
static OfxStatus setStringN(OfxPropertySetHandle h, const char *n, int c, const char *const *v) {
    for (int k = 0; k < c; ++k) setString(h, n, k, v[k]);
    return kOfxStatOK;
}
static OfxStatus setDoubleN(OfxPropertySetHandle h, const char *n, int c, const double *v) {
    for (int k = 0; k < c; ++k) setDouble(h, n, k, v[k]);
    return kOfxStatOK;
}
static OfxStatus setIntN(OfxPropertySetHandle h, const char *n, int c, const int *v) {
    for (int k = 0; k < c; ++k) setInt(h, n, k, v[k]);
    return kOfxStatOK;
}
static const Prop *find(OfxPropertySetHandle h, const char *n) {
    if (!h) return nullptr;
    auto it = PS(h)->m.find(n);
    return it == PS(h)->m.end() ? nullptr : &it->second;
}
static OfxStatus getPointer(OfxPropertySetHandle h, const char *n, int idx, void **v) {
    const Prop *p = find(h, n);
    if (!p) return kOfxStatErrUnknown;
    if (p->type != 'p') return kOfxStatErrValue;
    if (idx < 0 || idx >= (int)p->p.size()) return kOfxStatErrBadIndex;
    *v = p->p[idx];
    return kOfxStatOK;
}
static OfxStatus getString(OfxPropertySetHandle h, const char *n, int idx, char **v) {
    const Prop *p = find(h, n);
    if (!p) return kOfxStatErrUnknown;
    if (p->type != 's') return kOfxStatErrValue;
    if (idx < 0 || idx >= (int)p->s.size()) return kOfxStatErrBadIndex;
    *v = const_cast<char *>(p->s[idx].c_str());
    return kOfxStatOK;
}
static OfxStatus getDouble(OfxPropertySetHandle h, const char *n, int idx, double *v) {
    const Prop *p = find(h, n);
    if (!p) return kOfxStatErrUnknown;
    if (p->type != 'd') return kOfxStatErrValue;
    if (idx < 0 || idx >= (int)p->d.size()) return kOfxStatErrBadIndex;
    *v = p->d[idx];
    return kOfxStatOK;
}
static OfxStatus getInt(OfxPropertySetHandle h, const char *n, int idx, int *v) {
    const Prop *p = find(h, n);
    if (!p) return kOfxStatErrUnknown;
    if (p->type != 'i') return kOfxStatErrValue;
    if (idx < 0 || idx >= (int)p->i.size()) return kOfxStatErrBadIndex;
    *v = p->i[idx];
    return kOfxStatOK;
}
static OfxStatus getPointerN(OfxPropertySetHandle h, const char *n, int c, void **v) {
    for (int k = 0; k < c; ++k) {
        OfxStatus st = getPointer(h, n, k, v + k);
        if (st != kOfxStatOK) return st;
    }
    return kOfxStatOK;
}
static OfxStatus getStringN(OfxPropertySetHandle h, const char *n, int c, char **v) {
    for (int k = 0; k < c; ++k) {
        OfxStatus st = getString(h, n, k, v + k);
        if (st != kOfxStatOK) return st;
    }
    return kOfxStatOK;
}
static OfxStatus getDoubleN(OfxPropertySetHandle h, const char *n, int c, double *v) {
    for (int k = 0; k < c; ++k) {
        OfxStatus st = getDouble(h, n, k, v + k);
        if (st != kOfxStatOK) return st;
    }
    return kOfxStatOK;
}
static OfxStatus getIntN(OfxPropertySetHandle h, const char *n, int c, int *v) {
    for (int k = 0; k < c; ++k) {
        OfxStatus st = getInt(h, n, k, v + k);
        if (st != kOfxStatOK) return st;
    }
    return kOfxStatOK;
}
static OfxStatus propReset(OfxPropertySetHandle h, const char *n) {
    PS(h)->m.erase(n);
    return kOfxStatOK;
}
static OfxStatus getDimension(OfxPropertySetHandle h, const char *n, int *c) {
    const Prop *p = find(h, n);
    if (!p) return kOfxStatErrUnknown;
    *c = p->dim();
    return kOfxStatOK;
}

static OfxPropertySuiteV1 g_prop_suite = {setPointer, setString,  setDouble,  setInt,     setPointerN,
                                          setStringN, setDoubleN, setIntN,    getPointer, getString,
                                          getDouble,  getInt,     getPointerN, getStringN, getDoubleN,
                                          getIntN,    propReset,  getDimension};

// ---------------------------------------------------------------- params

struct Param {
    std::string type, name;
    PropSet props;
    int ival = 0;
    double dval = 0;
};

struct ParamSet {
    PropSet props;
    std::vector<std::unique_ptr<Param>> list;
    Param *get(const std::string &n) {
        for (auto &p : list)
            if (p->name == n) return p.get();
        return nullptr;
    }
};

static OfxStatus paramDefine(OfxParamSetHandle ps, const char *type, const char *name, OfxPropertySetHandle *props) {
    ParamSet *set = reinterpret_cast<ParamSet *>(ps);
    auto p = std::make_unique<Param>();
    p->type = type;
    p->name = name;
    if (props) *props = H(&p->props);
    set->list.push_back(std::move(p));
    return kOfxStatOK;
}
static OfxStatus paramGetHandle(OfxParamSetHandle ps, const char *name, OfxParamHandle *param, OfxPropertySetHandle *props) {
    Param *p = reinterpret_cast<ParamSet *>(ps)->get(name);
    if (!p) return kOfxStatErrUnknown;
    *param = reinterpret_cast<OfxParamHandle>(p);
    if (props) *props = H(&p->props);
    return kOfxStatOK;
}
static OfxStatus paramSetGetPropertySet(OfxParamSetHandle ps, OfxPropertySetHandle *props) {
    *props = H(&reinterpret_cast<ParamSet *>(ps)->props);
    return kOfxStatOK;
}
static OfxStatus paramGetPropertySet(OfxParamHandle p, OfxPropertySetHandle *props) {
    *props = H(&reinterpret_cast<Param *>(p)->props);
    return kOfxStatOK;
}
static OfxStatus get_value(Param *p, va_list ap) {
    if (p->type == kOfxParamTypeDouble) {
        *va_arg(ap, double *) = p->dval;
    } else if (p->type == kOfxParamTypeInteger || p->type == kOfxParamTypeChoice ||
               p->type == kOfxParamTypeBoolean) {
        *va_arg(ap, int *) = p->ival;
    } else {
        return kOfxStatErrUnsupported;
    }
    return kOfxStatOK;
}
static OfxStatus paramGetValue(OfxParamHandle h, ...) {
    va_list ap;
    va_start(ap, h);
    OfxStatus st = get_value(reinterpret_cast<Param *>(h), ap);
    va_end(ap);
    return st;
}
// Values do not animate in this host: every time reads the one value.
static OfxStatus paramGetValueAtTime(OfxParamHandle h, OfxTime t, ...) {
    va_list ap;
    va_start(ap, t);
    OfxStatus st = get_value(reinterpret_cast<Param *>(h), ap);
    va_end(ap);
    return st;
}
static OfxStatus paramUnsupported() { return kOfxStatErrUnsupported; }

// Fills the suite in the header's order; the members this host does not
// support point at a stub that says so.
static OfxParameterSuiteV1 make_param_suite() {
    OfxParameterSuiteV1 s;
    memset(&s, 0, sizeof s);
    s.paramDefine = paramDefine;
    s.paramGetHandle = paramGetHandle;
    s.paramSetGetPropertySet = paramSetGetPropertySet;
    s.paramGetPropertySet = paramGetPropertySet;
    s.paramGetValue = paramGetValue;
    s.paramGetValueAtTime = paramGetValueAtTime;
    s.paramGetDerivative = reinterpret_cast<decltype(s.paramGetDerivative)>(paramUnsupported);
    s.paramGetIntegral = reinterpret_cast<decltype(s.paramGetIntegral)>(paramUnsupported);
    s.paramSetValue = reinterpret_cast<decltype(s.paramSetValue)>(paramUnsupported);
    s.paramSetValueAtTime = reinterpret_cast<decltype(s.paramSetValueAtTime)>(paramUnsupported);
    s.paramGetNumKeys = reinterpret_cast<decltype(s.paramGetNumKeys)>(paramUnsupported);
    s.paramGetKeyTime = reinterpret_cast<decltype(s.paramGetKeyTime)>(paramUnsupported);
    s.paramGetKeyIndex = reinterpret_cast<decltype(s.paramGetKeyIndex)>(paramUnsupported);
    s.paramDeleteKey = reinterpret_cast<decltype(s.paramDeleteKey)>(paramUnsupported);
    s.paramDeleteAllKeys = reinterpret_cast<decltype(s.paramDeleteAllKeys)>(paramUnsupported);
    s.paramCopy = reinterpret_cast<decltype(s.paramCopy)>(paramUnsupported);
    s.paramEditBegin = reinterpret_cast<decltype(s.paramEditBegin)>(paramUnsupported);
    s.paramEditEnd = reinterpret_cast<decltype(s.paramEditEnd)>(paramUnsupported);
    return s;
}
static OfxParameterSuiteV1 g_param_suite = make_param_suite();

// ---------------------------------------------------------------- scenario

// What the synthetic host does with a requested time.
struct Scenario {
    const char *name;
    const char *depth;  // kOfxBitDepth*
    int width = 64, height = 8;
    double effect_fps = 60;  // what the host reports as the effect's frame rate
    double clip_fps = 300;   // what it reports as the Source clip's frame rate
    // Captured frame shown at host time t, or -1 when the host has no image there.
    std::function<int(double)> frame_at;
    bool report_range = true;
    double range_lo = 0, range_hi = 10000;
    int frames = 5, window = 0, step_mode = 0;
    double capture_fps = 300;
    double render_time = 10;
    // What the harness expects the plugin to ask for, worked out here, not by the plugin's code.
    std::vector<double> expected_times;
    bool cache_sources = false;
};

static const Scenario *g_sc = nullptr;
static std::vector<double> g_fetched;  // source times the plugin asked for

struct Image {
    PropSet props;
    std::vector<uint8_t> pixels;
    bool owned_by_host_cache = false;
};

static int bytes_per_channel(const char *depth) {
    if (!strcmp(depth, kOfxBitDepthByte)) return 1;
    if (!strcmp(depth, kOfxBitDepthShort)) return 2;
    return 4;
}

static const int kSquare = 8, kTop = 2, kBottom = 6;
static int square_x(int frame) { return 2 * frame; }

static void fill_frame(Image &img, int frame, int w, int h, const char *depth) {
    const int bpc = bytes_per_channel(depth);
    img.pixels.assign((size_t)w * h * 4 * bpc, 0);
    for (int y = 0; y < h; ++y)
        for (int x = 0; x < w; ++x) {
            const bool in = frame >= 0 && y >= kTop && y < kBottom && x >= square_x(frame) % w &&
                            x < square_x(frame) % w + kSquare;
            uint8_t *p = img.pixels.data() + ((size_t)y * w + x) * 4 * bpc;
            for (int c = 0; c < 4; ++c) {
                const bool on = c == 3 || in;
                if (bpc == 1) p[c] = on ? 255 : 0;
                else if (bpc == 2) ((uint16_t *)p)[c] = on ? 65535 : 0;
                else ((float *)p)[c] = on ? 1.0f : 0.0f;
            }
        }
}

static void set_image_props(Image &img, int w, int h, const char *depth, const std::string &uid) {
    OfxPropertySetHandle ps = H(&img.props);
    setString(ps, kOfxPropType, 0, kOfxTypeImage);
    setPointer(ps, kOfxImagePropData, 0, img.pixels.data());
    const int b[4] = {0, 0, w, h};
    setIntN(ps, kOfxImagePropBounds, 4, b);
    setIntN(ps, kOfxImagePropRegionOfDefinition, 4, b);
    setInt(ps, kOfxImagePropRowBytes, 0, w * 4 * bytes_per_channel(depth));
    setString(ps, kOfxImageEffectPropPixelDepth, 0, depth);
    setString(ps, kOfxImageEffectPropComponents, 0, kOfxImageComponentRGBA);
    setString(ps, kOfxImagePropField, 0, kOfxImageFieldNone);
    setString(ps, kOfxImagePropUniqueIdentifier, 0, uid.c_str());
    setDouble(ps, kOfxImagePropPixelAspectRatio, 0, 1.0);
}

// ---------------------------------------------------------------- effects & clips

struct Effect;
struct Clip {
    std::string name;
    PropSet props;
    Effect *owner = nullptr;
};
struct Effect {
    PropSet props;
    ParamSet params;
    std::vector<std::unique_ptr<Clip>> clips;
    Clip *clip(const std::string &n) {
        for (auto &c : clips)
            if (c->name == n) return c.get();
        return nullptr;
    }
};

static Image *g_output = nullptr;
static std::map<int, std::unique_ptr<Image>> g_cache;
static int g_live_images = 0;

static OfxStatus getPropertySet(OfxImageEffectHandle e, OfxPropertySetHandle *ps) {
    *ps = H(&reinterpret_cast<Effect *>(e)->props);
    return kOfxStatOK;
}
static OfxStatus getParamSet(OfxImageEffectHandle e, OfxParamSetHandle *ps) {
    *ps = reinterpret_cast<OfxParamSetHandle>(&reinterpret_cast<Effect *>(e)->params);
    return kOfxStatOK;
}
static OfxStatus clipDefine(OfxImageEffectHandle e, const char *name, OfxPropertySetHandle *ps) {
    Effect *fx = reinterpret_cast<Effect *>(e);
    auto c = std::make_unique<Clip>();
    c->name = name;
    c->owner = fx;
    setString(H(&c->props), kOfxPropName, 0, name);
    if (ps) *ps = H(&c->props);
    fx->clips.push_back(std::move(c));
    return kOfxStatOK;
}
static OfxStatus clipGetHandle(OfxImageEffectHandle e, const char *name, OfxImageClipHandle *clip, OfxPropertySetHandle *ps) {
    Clip *c = reinterpret_cast<Effect *>(e)->clip(name);
    if (!c) return kOfxStatErrBadHandle;
    *clip = reinterpret_cast<OfxImageClipHandle>(c);
    if (ps) *ps = H(&c->props);
    return kOfxStatOK;
}
static OfxStatus clipGetPropertySet(OfxImageClipHandle clip, OfxPropertySetHandle *ps) {
    *ps = H(&reinterpret_cast<Clip *>(clip)->props);
    return kOfxStatOK;
}
static OfxStatus clipGetImage(OfxImageClipHandle clip, OfxTime t, const OfxRectD *, OfxPropertySetHandle *out) {
    Clip *c = reinterpret_cast<Clip *>(clip);
    if (c->name == kOfxImageEffectOutputClipName) {
        *out = H(&g_output->props);
        ++g_live_images;
        return kOfxStatOK;
    }
    g_fetched.push_back(t);
    const int frame = g_sc->frame_at(t);
    if (frame < 0) return kOfxStatFailed;
    if (g_sc->cache_sources) {
        auto &slot = g_cache[frame];
        if (!slot) {
            slot = std::make_unique<Image>();
            fill_frame(*slot, frame, g_sc->width, g_sc->height, g_sc->depth);
            set_image_props(*slot, g_sc->width, g_sc->height, g_sc->depth, "frame" + std::to_string(frame));
            slot->owned_by_host_cache = true;
        }
        *out = H(&slot->props);
        ++g_live_images;
        return kOfxStatOK;
    }
    Image *img = new Image();
    fill_frame(*img, frame, g_sc->width, g_sc->height, g_sc->depth);
    set_image_props(*img, g_sc->width, g_sc->height, g_sc->depth, "frame" + std::to_string(frame));
    *out = H(&img->props);
    ++g_live_images;
    return kOfxStatOK;
}
static OfxStatus clipReleaseImage(OfxPropertySetHandle h) {
    --g_live_images;
    Image *img = reinterpret_cast<Image *>(reinterpret_cast<char *>(PS(h)) - offsetof(Image, props));
    if (img != g_output && !img->owned_by_host_cache) delete img;
    return kOfxStatOK;
}
static OfxStatus clipGetRegionOfDefinition(OfxImageClipHandle, OfxTime, OfxRectD *rod) {
    rod->x1 = rod->y1 = 0;
    rod->x2 = g_sc ? g_sc->width : 0;
    rod->y2 = g_sc ? g_sc->height : 0;
    return kOfxStatOK;
}
static int fxAbort(OfxImageEffectHandle) { return 0; }
static OfxStatus memUnsupported() { return kOfxStatErrUnsupported; }

static OfxImageEffectSuiteV1 make_fx_suite() {
    OfxImageEffectSuiteV1 s;
    memset(&s, 0, sizeof s);
    s.getPropertySet = getPropertySet;
    s.getParamSet = getParamSet;
    s.clipDefine = clipDefine;
    s.clipGetHandle = clipGetHandle;
    s.clipGetPropertySet = clipGetPropertySet;
    s.clipGetImage = clipGetImage;
    s.clipReleaseImage = clipReleaseImage;
    s.clipGetRegionOfDefinition = clipGetRegionOfDefinition;
    s.abort = fxAbort;
    s.imageMemoryAlloc = reinterpret_cast<decltype(s.imageMemoryAlloc)>(memUnsupported);
    s.imageMemoryFree = reinterpret_cast<decltype(s.imageMemoryFree)>(memUnsupported);
    s.imageMemoryLock = reinterpret_cast<decltype(s.imageMemoryLock)>(memUnsupported);
    s.imageMemoryUnlock = reinterpret_cast<decltype(s.imageMemoryUnlock)>(memUnsupported);
    return s;
}
static OfxImageEffectSuiteV1 g_fx_suite = make_fx_suite();

static PropSet g_host_props;
static const void *fetchSuite(OfxPropertySetHandle, const char *name, int version) {
    if (!strcmp(name, kOfxPropertySuite) && version == 1) return &g_prop_suite;
    if (!strcmp(name, kOfxImageEffectSuite) && version == 1) return &g_fx_suite;
    if (!strcmp(name, kOfxParameterSuite) && version == 1) return &g_param_suite;
    return nullptr;
}
static OfxHost g_host = {nullptr, fetchSuite};

// ---------------------------------------------------------------- driver

static OfxPlugin *g_plugin = nullptr;

static OfxStatus call(const char *action, void *handle, PropSet *in, PropSet *out) {
    return g_plugin->mainEntry(action, handle, in ? H(in) : nullptr, out ? H(out) : nullptr);
}

static int g_failures = 0;

// The expected value of one output channel at x on a square row: the share
// of sampled captured frames whose square covers x.
static double expected_coverage(int x, const std::vector<int> &frames) {
    if (frames.empty()) return 0;
    int hits = 0;
    for (int f : frames) hits += (x >= square_x(f) % g_sc->width && x < square_x(f) % g_sc->width + kSquare);
    return (double)hits / frames.size();
}

static std::string row_string(const Image &img, int y, int w, const char *depth) {
    std::string s;
    const int bpc = bytes_per_channel(depth);
    for (int x = 0; x < w; ++x) {
        const uint8_t *p = img.pixels.data() + ((size_t)y * w + x) * 4 * bpc;
        double v = bpc == 1 ? p[0] / 255.0 : bpc == 2 ? ((const uint16_t *)p)[0] / 65535.0 : ((const float *)p)[0];
        const int d = (int)std::lround(v * 9);
        s += d == 0 ? '.' : (char)('0' + d);
    }
    return s;
}

static Effect *make_instance(const Effect &ctx_desc, const Scenario &sc) {
    Effect *fx = new Effect();
    setString(H(&fx->props), kOfxPropType, 0, kOfxTypeImageEffectInstance);
    setString(H(&fx->props), kOfxImageEffectPropContext, 0, kOfxImageEffectContextFilter);
    setDouble(H(&fx->props), kOfxImageEffectPropFrameRate, 0, sc.effect_fps);
    const double size[2] = {(double)sc.width, (double)sc.height};
    setDoubleN(H(&fx->props), kOfxImageEffectPropProjectSize, 2, size);
    setDoubleN(H(&fx->props), kOfxImageEffectPropProjectExtent, 2, size);
    setDouble(H(&fx->props), kOfxImageEffectPropProjectPixelAspectRatio, 0, 1.0);
    setInt(H(&fx->props), kOfxPropIsInteractive, 0, 0);
    for (auto &c : ctx_desc.clips) {
        auto ci = std::make_unique<Clip>();
        ci->name = c->name;
        ci->owner = fx;
        ci->props = c->props;
        OfxPropertySetHandle ps = H(&ci->props);
        setDouble(ps, kOfxImageEffectPropFrameRate, 0,
                  ci->name == kOfxImageEffectSimpleSourceClipName ? sc.clip_fps : sc.effect_fps);
        setString(ps, kOfxImageEffectPropPixelDepth, 0, sc.depth);
        setString(ps, kOfxImageEffectPropComponents, 0, kOfxImageComponentRGBA);
        setInt(ps, kOfxImageClipPropConnected, 0, 1);
        setInt(ps, kOfxImageClipPropContinuousSamples, 0, 0);
        if (sc.report_range) {
            const double r[2] = {sc.range_lo, sc.range_hi};
            setDoubleN(ps, kOfxImageEffectPropFrameRange, 2, r);
        }
        fx->clips.push_back(std::move(ci));
    }
    for (auto &p : ctx_desc.params.list) {
        auto pi = std::make_unique<Param>(*p);
        int iv = 0;
        double dv = 0;
        if (getInt(H(&pi->props), kOfxParamPropDefault, 0, &iv) == kOfxStatOK) pi->ival = iv;
        if (getDouble(H(&pi->props), kOfxParamPropDefault, 0, &dv) == kOfxStatOK) pi->dval = dv;
        fx->params.list.push_back(std::move(pi));
    }
    fx->params.get("frames")->ival = sc.frames;
    fx->params.get("captureFps")->dval = sc.capture_fps;
    fx->params.get("window")->ival = sc.window;
    fx->params.get("stepMode")->ival = sc.step_mode;
    return fx;
}

// Runs one scenario: identity check, frames needed, render, exact compare.
static void run(const Effect &ctx_desc, const Scenario &sc, bool show_row = true) {
    g_sc = &sc;
    g_fetched.clear();
    g_cache.clear();
    Effect *fx = make_instance(ctx_desc, sc);
    call(kOfxActionCreateInstance, fx, nullptr, nullptr);

    PropSet in, out;
    setDouble(H(&in), kOfxPropTime, 0, sc.render_time);
    const double scale[2] = {1, 1};
    setDoubleN(H(&in), kOfxImageEffectPropRenderScale, 2, scale);
    setString(H(&in), kOfxImageEffectPropFieldToRender, 0, kOfxImageFieldNone);
    const int win[4] = {0, 0, sc.width, sc.height};
    setIntN(H(&in), kOfxImageEffectPropRenderWindow, 4, win);

    printf("\n== %s\n", sc.name);
    printf("   depth %s, effect fps %.0f, clip fps %.0f, frames %d, capture fps %.0f, %s, %s, render t=%.2f\n",
           sc.depth, sc.effect_fps, sc.clip_fps, sc.frames, sc.capture_fps, sc.window ? "centred" : "forward",
           sc.step_mode ? "host-frame step" : "captured-frame step", sc.render_time);

    PropSet fn_out;
    if (call(kOfxImageEffectActionGetFramesNeeded, fx, &in, &fn_out) == kOfxStatOK) {
        double r[2] = {0, 0};
        getDoubleN(H(&fn_out), "OfxImageClipPropFrameRange_Source", 2, r);
        printf("   frames needed: [%.4f, %.4f]\n", r[0], r[1]);
    }

    Image output;
    output.pixels.assign((size_t)sc.width * sc.height * 4 * bytes_per_channel(sc.depth), 0xcd);
    set_image_props(output, sc.width, sc.height, sc.depth, "output");
    g_output = &output;

    std::vector<int> frames;
    const OfxStatus ident = call(kOfxImageEffectActionIsIdentity, fx, &in, &out);
    bool rendered = false;
    if (ident == kOfxStatOK) {
        char *clip = nullptr;
        double t = 0;
        getString(H(&out), kOfxPropName, 0, &clip);
        getDouble(H(&out), kOfxPropTime, 0, &t);
        printf("   identity: pass through '%s' at t=%.4f\n", clip ? clip : "?", t);
        const int f = sc.frame_at(t);
        fill_frame(output, f, sc.width, sc.height, sc.depth);
        frames.push_back(f);
    } else {
        const auto t0 = std::chrono::high_resolution_clock::now();
        const OfxStatus st = call(kOfxImageEffectActionRender, fx, &in, nullptr);
        const auto t1 = std::chrono::high_resolution_clock::now();
        rendered = true;
        printf("   render status %d (%.2f ms)\n", st, std::chrono::duration<double, std::milli>(t1 - t0).count());
        if (st != kOfxStatOK) ++g_failures;
        printf("   fetched times:");
        for (double t : g_fetched) {
            const int f = sc.frame_at(t);
            printf(" %.4f->%d", t, f);
            if (f >= 0) frames.push_back(f);
        }
        printf("\n");
    }

    // The plugin must ask for exactly the times the harness worked out.
    if (rendered && !sc.expected_times.empty()) {
        bool same = sc.expected_times.size() == g_fetched.size();
        for (size_t i = 0; same && i < g_fetched.size(); ++i)
            same = std::fabs(g_fetched[i] - sc.expected_times[i]) < 1e-9;
        printf("   requested times %s\n", same ? "match the expected ones" : "DIFFER from the expected ones");
        if (!same) ++g_failures;
    }
    if (g_live_images != 0) {
        printf("   LEAK: %d images not released\n", g_live_images);
        ++g_failures;
        g_live_images = 0;
    }

    // Exact compare against the harness's own average.
    const int bpc = bytes_per_channel(sc.depth);
    int bad = 0;
    double worst = 0;
    for (int y = 0; y < sc.height; ++y)
        for (int x = 0; x < sc.width; ++x) {
            const double cov = (y >= kTop && y < kBottom) ? expected_coverage(x, frames) : 0.0;
            const uint8_t *p = output.pixels.data() + ((size_t)y * sc.width + x) * 4 * bpc;
            for (int c = 0; c < 4; ++c) {
                const double want = c == 3 ? (frames.empty() ? 0.0 : 1.0) : cov;
                double got, tol;
                if (bpc == 1) { got = p[c] / 255.0; tol = 0.51 / 255.0; }
                else if (bpc == 2) { got = ((const uint16_t *)p)[c] / 65535.0; tol = 0.51 / 65535.0; }
                else { got = ((const float *)p)[c]; tol = 1e-6; }
                const double err = std::fabs(got - want);
                worst = std::max(worst, err);
                if (err > tol) ++bad;
            }
        }
    std::vector<int> distinct = frames;
    std::sort(distinct.begin(), distinct.end());
    distinct.erase(std::unique(distinct.begin(), distinct.end()), distinct.end());
    printf("   captured frames blended: %zu samples, %zu distinct (", frames.size(), distinct.size());
    for (size_t i = 0; i < distinct.size(); ++i) printf(i ? " %d" : "%d", distinct[i]);
    printf(")\n");
    printf("   exact check: %s (%d channel values off, worst error %.3g)\n", bad ? "FAIL" : "pass", bad, worst);
    if (bad) ++g_failures;
    if (show_row) printf("   row 3, red, 0-9: %s\n", row_string(output, 3, sc.width, sc.depth).c_str());

    call(kOfxActionDestroyInstance, fx, nullptr, nullptr);
    delete fx;
    g_output = nullptr;
}

static std::vector<double> times_from(double t, int n, double step, int first) {
    std::vector<double> v;
    for (int k = 0; k < n; ++k) v.push_back(t + (first + k) * step);
    return v;
}

int main(int argc, char **argv) {
    if (argc < 2) {
        fprintf(stderr, "usage: %s <FrameBlend.ofx>\n", argv[0]);
        return 2;
    }
    HMODULE lib = LoadLibraryA(argv[1]);
    if (!lib) {
        fprintf(stderr, "LoadLibrary failed for %s: error %lu\n", argv[1], GetLastError());
        return 2;
    }
    auto get_n = (int (*)())GetProcAddress(lib, "OfxGetNumberOfPlugins");
    auto get_p = (OfxPlugin * (*)(int)) GetProcAddress(lib, "OfxGetPlugin");
    if (!get_n || !get_p) {
        fprintf(stderr, "%s does not export OfxGetNumberOfPlugins/OfxGetPlugin\n", argv[1]);
        return 2;
    }
    printf("plugins in bundle: %d\n", get_n());
    g_plugin = get_p(0);
    printf("plugin: %s v%u.%u, api %s v%d\n", g_plugin->pluginIdentifier, g_plugin->pluginVersionMajor,
           g_plugin->pluginVersionMinor, g_plugin->pluginApi, g_plugin->apiVersion);

    OfxPropertySetHandle hp = H(&g_host_props);
    setString(hp, kOfxPropName, 0, "com.dodstudio.ofxharness");
    setString(hp, kOfxPropLabel, 0, "Frame Blend test harness");
    setInt(hp, kOfxImageEffectPropTemporalClipAccess, 0, 1);
    setInt(hp, kOfxImageEffectPropSupportsMultipleClipDepths, 0, 0);
    setInt(hp, kOfxImageEffectPropSupportsTiles, 0, 0);
    const char *depths[3] = {kOfxBitDepthByte, kOfxBitDepthShort, kOfxBitDepthFloat};
    setStringN(hp, kOfxImageEffectPropSupportedPixelDepths, 3, depths);
    setString(hp, kOfxImageEffectPropSupportedContexts, 0, kOfxImageEffectContextFilter);
    g_host.host = hp;
    g_plugin->setHost(&g_host);

    if (call(kOfxActionLoad, nullptr, nullptr, nullptr) != kOfxStatOK) {
        fprintf(stderr, "load failed\n");
        return 2;
    }
    Effect desc;
    call(kOfxActionDescribe, &desc, nullptr, nullptr);
    char *label = nullptr;
    int temporal = 0;
    getString(H(&desc.props), kOfxPropLabel, 0, &label);
    getInt(H(&desc.props), kOfxImageEffectPropTemporalClipAccess, 0, &temporal);
    printf("describe: label '%s', temporal clip access %d\n", label ? label : "?", temporal);

    Effect ctx;
    PropSet ctx_in;
    setString(H(&ctx_in), kOfxImageEffectPropContext, 0, kOfxImageEffectContextFilter);
    call(kOfxImageEffectActionDescribeInContext, &ctx, &ctx_in, nullptr);
    for (auto &c : ctx.clips) {
        int t = 0;
        getInt(H(&c->props), kOfxImageEffectPropTemporalClipAccess, 0, &t);
        printf("clip '%s' temporal access %d\n", c->name.c_str(), t);
    }
    for (auto &p : ctx.params.list) {
        char *l = nullptr;
        getString(H(&p->props), kOfxPropLabel, 0, &l);
        printf("param '%s' (%s) '%s'\n", p->name.c_str(), p->type.c_str(), l ? l : "?");
    }

    const auto r = [](double v) { return (int)std::floor(v + 0.5); };
    std::vector<Scenario> list;
    {   // VEGAS Pro 21 Media FX, as measured (#1048): time counts media frames,
        // the effect and the Source clip both run at the capture's 300 fps.
        Scenario s;
        s.name = "VEGAS Media FX model, 8-bit: 5 captured frames per output frame";
        s.depth = kOfxBitDepthByte;
        s.effect_fps = 300;
        s.clip_fps = 300;
        s.render_time = 675;
        s.range_lo = 0;
        s.range_hi = 2997;
        s.frame_at = [r](double t) { return r(t); };
        s.expected_times = times_from(675, 5, 1.0, 0);
        list.push_back(s);
        s.name = "VEGAS Media FX model, 32-bit float";
        s.depth = kOfxBitDepthFloat;
        list.push_back(s);
        s.name = "VEGAS Media FX model, 2 frames (2.5x slow motion)";
        s.depth = kOfxBitDepthByte;
        s.frames = 2;
        s.expected_times = times_from(675, 2, 1.0, 0);
        list.push_back(s);
        s.name = "frames to blend = 1: identity, the host passes the source through";
        s.frames = 1;
        s.expected_times.clear();
        list.push_back(s);
        s.name = "centred window, 5 frames";
        s.frames = 5;
        s.window = 1;
        s.expected_times = times_from(675, 5, 1.0, -2);
        list.push_back(s);
        s.name = "centred window, 4 frames (one extra before)";
        s.frames = 4;
        s.expected_times = times_from(675, 4, 1.0, -2);
        list.push_back(s);
        s.name = "last frame of the clip, range reported: repeats the last frame";
        s.frames = 5;
        s.window = 0;
        s.render_time = 2997;
        s.frame_at = [r](double t) { return t > 2997 + 1e-9 ? -1 : r(t); };
        s.expected_times = {2997, 2997, 2997, 2997, 2997};
        list.push_back(s);
        s.name = "last frame of the clip, no range reported: missing frames left out";
        s.report_range = false;
        s.expected_times = times_from(2997, 5, 1.0, 0);
        list.push_back(s);
    }
    {   // VEGAS Pro 21 Event FX, as measured (#1048): time counts 60 fps output
        // frames, the Source clip is the event conformed to 60 fps, and every
        // in-between time snaps to the nearest whole frame. Blending there
        // would give a 4:1 double image, so the plugin passes the source through.
        Scenario s;
        s.name = "VEGAS Event FX model: no in-between frames, passes through";
        s.depth = kOfxBitDepthByte;
        s.effect_fps = 60;
        s.clip_fps = 60;
        s.render_time = 15;
        s.frame_at = [r](double t) { return 5 * r(t); };
        list.push_back(s);
    }
    {   // A host with a 60 fps time base that does deliver the in-between
        // captured frames (the OFX spec's reading of a 300 fps clip).
        Scenario s;
        s.name = "60 fps time base, 300 fps clip, 16-bit, 3 frames";
        s.depth = kOfxBitDepthShort;
        s.frames = 3;
        s.frame_at = [r](double t) { return r(t * 5); };
        s.expected_times = times_from(10, 3, 0.2, 0);
        list.push_back(s);
        // The fallback choice: a host counting captured frames but reporting 60 fps.
        s.name = "time in captured frames, rates reported as 60: step = one host frame";
        s.frames = 5;
        s.clip_fps = 60;
        s.step_mode = 1;
        s.render_time = 50;
        s.frame_at = [r](double t) { return r(t); };
        s.expected_times = times_from(50, 5, 1.0, 0);
        list.push_back(s);
    }
    for (const Scenario &s : list) run(ctx, s);

    // Speed at 2560x1440 (a cached source, as a host's frame cache would be).
    for (const char *depth : {kOfxBitDepthByte, kOfxBitDepthFloat}) {
        Scenario s;
        s.name = !strcmp(depth, kOfxBitDepthByte) ? "speed: 2560x1440 8-bit, 5 frames" : "speed: 2560x1440 float, 5 frames";
        s.depth = depth;
        s.width = 2560;
        s.height = 1440;
        s.cache_sources = true;
        s.frame_at = [r](double t) { return r(t * 5); };
        s.expected_times = times_from(10, 5, 0.2, 0);
        run(ctx, s, false);
        // Repeat with the frames cached, as in a real render.
        g_sc = &s;
        Effect *fx = make_instance(ctx, s);
        call(kOfxActionCreateInstance, fx, nullptr, nullptr);
        Image output;
        output.pixels.assign((size_t)s.width * s.height * 4 * bytes_per_channel(depth), 0);
        set_image_props(output, s.width, s.height, depth, "output");
        g_output = &output;
        PropSet in;
        const int win[4] = {0, 0, s.width, s.height};
        setIntN(H(&in), kOfxImageEffectPropRenderWindow, 4, win);
        double best = 1e9, total = 0;
        const int reps = 20;
        for (int i = 0; i < reps; ++i) {
            setDouble(H(&in), kOfxPropTime, 0, 10.0 + 0.0);
            const auto t0 = std::chrono::high_resolution_clock::now();
            call(kOfxImageEffectActionRender, fx, &in, nullptr);
            const double ms = std::chrono::duration<double, std::milli>(std::chrono::high_resolution_clock::now() - t0).count();
            best = std::min(best, ms);
            total += ms;
        }
        printf("   %d renders with cached sources: best %.1f ms, mean %.1f ms per frame\n", reps, best, total / reps);
        call(kOfxActionDestroyInstance, fx, nullptr, nullptr);
        delete fx;
        g_output = nullptr;
        g_cache.clear();
    }

    call(kOfxActionUnload, nullptr, nullptr, nullptr);
    printf("\n%s: %d failure(s)\n", g_failures ? "FAILED" : "ALL PASSED", g_failures);
    return g_failures ? 1 : 0;
}
