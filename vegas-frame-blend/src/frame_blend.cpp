// Frame Blend: an OpenFX filter that averages several source frames into each
// output frame, so a 300 fps capture on a 60 fps timeline gets real motion
// blur (#1048). Written against the plain OFX C API (no support library).
//
// Diagnostics: while "Write diagnostics log" is on, the plugin appends what
// the host tells it (host properties, clip frame rates and ranges, render
// times, and a hash of every fetched image, so a log shows whether in-between
// times return distinct frames) to %TEMP%\dodstudio_frame_blend.log, or to the
// file named by DODSTUDIO_FRAME_BLEND_LOG.

#include "ofxImageEffect.h"
#include "ofxMessage.h"

#include "blend.h"

#include <atomic>
#include <cstdarg>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <mutex>
#include <string>
#include <thread>
#include <vector>

#define WIN32_LEAN_AND_MEAN
#include <windows.h>

namespace {

const char *const kPluginId = "com.dodstudio.FrameBlend";
const int kVersionMajor = 0;
const int kVersionMinor = 1;

const char *const kParamFrames = "frames";
const char *const kParamCaptureFps = "captureFps";
const char *const kParamWindow = "window";
const char *const kParamStep = "stepMode";
const char *const kParamLog = "writeLog";

OfxHost *g_host = nullptr;
const OfxPropertySuiteV1 *g_prop = nullptr;
const OfxImageEffectSuiteV1 *g_fx = nullptr;
const OfxParameterSuiteV1 *g_param = nullptr;

// ---------------------------------------------------------------- logging

std::mutex g_log_mutex;
std::atomic<bool> g_log_enabled{true};

std::string log_path() {
    char buf[MAX_PATH * 2];
    DWORD n = GetEnvironmentVariableA("DODSTUDIO_FRAME_BLEND_LOG", buf, sizeof buf);
    if (n > 0 && n < sizeof buf) return std::string(buf, n);
    n = GetTempPathA(sizeof buf, buf);
    if (n > 0 && n < sizeof buf) return std::string(buf, n) + "dodstudio_frame_blend.log";
    return "dodstudio_frame_blend.log";
}

void logf(const char *fmt, ...) {
    if (!g_log_enabled.load(std::memory_order_relaxed)) return;
    char line[4096];
    va_list ap;
    va_start(ap, fmt);
    vsnprintf(line, sizeof line, fmt, ap);
    va_end(ap);
    std::lock_guard<std::mutex> lock(g_log_mutex);
    static const std::string path = log_path();
    FILE *f = nullptr;
    if (fopen_s(&f, path.c_str(), "ab") != 0 || !f) return;
    fprintf(f, "%10llu t%-6lu %s\r\n", static_cast<unsigned long long>(GetTickCount64()),
            GetCurrentThreadId(), line);
    fclose(f);
}

// Prints every value of a property whatever its type; OFX has no way to list
// a set's properties, so callers name the ones worth seeing.
std::string describe_prop(OfxPropertySetHandle ps, const char *name) {
    int dim = 0;
    if (!ps || g_prop->propGetDimension(ps, name, &dim) != kOfxStatOK) return "(absent)";
    std::string out;
    for (int i = 0; i < dim; ++i) {
        if (i) out += ", ";
        char *s = nullptr;
        int iv = 0;
        double dv = 0;
        void *pv = nullptr;
        char tmp[64];
        if (g_prop->propGetString(ps, name, i, &s) == kOfxStatOK && s) {
            out += "'";
            out += s;
            out += "'";
        } else if (g_prop->propGetInt(ps, name, i, &iv) == kOfxStatOK) {
            snprintf(tmp, sizeof tmp, "%d", iv);
            out += tmp;
        } else if (g_prop->propGetDouble(ps, name, i, &dv) == kOfxStatOK) {
            snprintf(tmp, sizeof tmp, "%.6f", dv);
            out += tmp;
        } else if (g_prop->propGetPointer(ps, name, i, &pv) == kOfxStatOK) {
            snprintf(tmp, sizeof tmp, "%p", pv);
            out += tmp;
        } else {
            out += "?";
        }
    }
    return dim == 0 ? "(empty)" : out;
}

void log_props(const char *what, OfxPropertySetHandle ps, const char *const *names) {
    for (; *names; ++names) logf("  %s %s = %s", what, *names, describe_prop(ps, *names).c_str());
}

const char *const kHostProps[] = {kOfxPropName, kOfxPropLabel, kOfxPropVersion, kOfxPropVersionLabel,
                                  kOfxPropAPIVersion, kOfxImageEffectHostPropIsBackground,
                                  kOfxImageEffectPropSupportsOverlays,
                                  kOfxImageEffectPropSupportsMultiResolution,
                                  kOfxImageEffectPropSupportsTiles, kOfxImageEffectPropTemporalClipAccess,
                                  kOfxImageEffectPropSupportedComponents,
                                  kOfxImageEffectPropSupportedContexts,
                                  kOfxImageEffectPropSupportedPixelDepths,
                                  kOfxImageEffectPropSupportsMultipleClipDepths,
                                  kOfxImageEffectPropSupportsMultipleClipPARs,
                                  kOfxImageEffectPropSetableFrameRate, kOfxImageEffectPropSetableFielding,
                                  kOfxImageEffectInstancePropSequentialRender,
                                  kOfxParamHostPropSupportsCustomAnimation,
                                  kOfxImageEffectHostPropNativeOrigin, nullptr};

const char *const kInstanceProps[] = {kOfxImageEffectPropContext, kOfxImageEffectPropFrameRate,
                                      kOfxImageEffectPropProjectSize, kOfxImageEffectPropProjectOffset,
                                      kOfxImageEffectPropProjectExtent,
                                      kOfxImageEffectPropProjectPixelAspectRatio,
                                      kOfxImageEffectInstancePropEffectDuration,
                                      kOfxImageEffectInstancePropSequentialRender, kOfxPropIsInteractive,
                                      "OfxImageEffectPropVegasContext", "OfxPropVegasTimeCode", nullptr};

const char *const kClipProps[] = {kOfxPropName, kOfxImageEffectPropFrameRate,
                                  kOfxImageEffectPropUnmappedFrameRate, kOfxImageEffectPropFrameRange,
                                  kOfxImageEffectPropUnmappedFrameRange, kOfxImageClipPropContinuousSamples,
                                  kOfxImageClipPropFieldOrder, kOfxImageEffectPropPixelDepth,
                                  kOfxImageClipPropUnmappedPixelDepth, kOfxImageEffectPropComponents,
                                  kOfxImageEffectPropPreMultiplication, kOfxImagePropPixelAspectRatio,
                                  kOfxImageClipPropConnected, "OfxImageEffectPropPixelOrder", nullptr};

const char *const kImageProps[] = {kOfxImageEffectPropPixelDepth, kOfxImageEffectPropComponents,
                                   kOfxImagePropBounds, kOfxImagePropRowBytes, kOfxImagePropField,
                                   kOfxImagePropUniqueIdentifier, kOfxImageEffectPropRenderScale,
                                   "OfxImageEffectPropPixelOrder", nullptr};

const char *const kRenderArgs[] = {kOfxPropTime, kOfxImageEffectPropFieldToRender,
                                   kOfxImageEffectPropRenderWindow, kOfxImageEffectPropRenderScale,
                                   kOfxImageEffectPropSequentialRenderStatus,
                                   kOfxImageEffectPropInteractiveRenderStatus,
                                   kOfxImageEffectPropRenderQualityDraft, nullptr};

const char *const kSequenceArgs[] = {kOfxImageEffectPropFrameRange, kOfxImageEffectPropFrameStep,
                                     kOfxPropIsInteractive, kOfxImageEffectPropRenderScale,
                                     kOfxImageEffectPropSequentialRenderStatus, nullptr};

// FNV-1a over a 32x32 grid of pixels: enough to tell two frames apart in a log
// without hashing the whole image.
uint64_t image_hash(const blend::View &v, int bytes_per_pixel) {
    uint64_t h = 1469598103934665603ull;
    const int w = v.x2 - v.x1, hgt = v.y2 - v.y1;
    if (w <= 0 || hgt <= 0 || !v.data) return 0;
    for (int gy = 0; gy < 32; ++gy) {
        const int y = gy * hgt / 32;
        const uint8_t *row = v.data + y * v.row_bytes;
        for (int gx = 0; gx < 32; ++gx) {
            const uint8_t *p = row + static_cast<size_t>(gx * w / 32) * bytes_per_pixel;
            for (int b = 0; b < bytes_per_pixel; ++b) h = (h ^ p[b]) * 1099511628211ull;
        }
    }
    return h;
}

// ---------------------------------------------------------------- instance

struct Instance {
    OfxImageClipHandle source = nullptr;
    OfxImageClipHandle output = nullptr;
    OfxParamHandle frames = nullptr, capture_fps = nullptr, window = nullptr, step = nullptr,
                   log = nullptr;
    std::atomic<int> renders{0};
    std::atomic<bool> warned{false};
    int id = 0;
};

std::atomic<int> g_next_instance{1};

Instance *get_instance(OfxImageEffectHandle effect) {
    OfxPropertySetHandle ps = nullptr;
    g_fx->getPropertySet(effect, &ps);
    void *p = nullptr;
    g_prop->propGetPointer(ps, kOfxPropInstanceData, 0, &p);
    return static_cast<Instance *>(p);
}

struct Settings {
    int frames = 5;
    double capture_fps = 300.0;
    blend::Window window = blend::Window::Forward;
    blend::StepMode step = blend::StepMode::OneCapturedFrame;
    bool log = true;
};

Settings read_settings(Instance *inst, OfxTime time) {
    Settings s;
    int iv = 0, logv = 1;
    double dv = 0;
    if (inst->frames && g_param->paramGetValueAtTime(inst->frames, time, &iv) == kOfxStatOK)
        s.frames = std::min(16, std::max(1, iv));
    if (inst->capture_fps && g_param->paramGetValueAtTime(inst->capture_fps, time, &dv) == kOfxStatOK)
        s.capture_fps = dv;
    if (inst->window && g_param->paramGetValueAtTime(inst->window, time, &iv) == kOfxStatOK)
        s.window = iv == 1 ? blend::Window::Centred : blend::Window::Forward;
    if (inst->step && g_param->paramGetValueAtTime(inst->step, time, &iv) == kOfxStatOK)
        s.step = iv == 1 ? blend::StepMode::OneHostFrame : blend::StepMode::OneCapturedFrame;
    if (inst->log && g_param->paramGetValueAtTime(inst->log, time, &logv) == kOfxStatOK) s.log = logv != 0;
    return s;
}

double effect_fps(OfxImageEffectHandle effect) {
    OfxPropertySetHandle ps = nullptr;
    double fps = 0;
    g_fx->getPropertySet(effect, &ps);
    if (g_prop->propGetDouble(ps, kOfxImageEffectPropFrameRate, 0, &fps) == kOfxStatOK && fps > 0) return fps;
    return 0;
}

double clip_fps(OfxImageClipHandle clip) {
    OfxPropertySetHandle ps = nullptr;
    double fps = 0;
    if (g_fx->clipGetPropertySet(clip, &ps) != kOfxStatOK) return 0;
    if (g_prop->propGetDouble(ps, kOfxImageEffectPropFrameRate, 0, &fps) != kOfxStatOK) return 0;
    return fps;
}

// Whether the source can deliver the captured frames between two output
// frames. A clip whose reported frame rate is below the capture rate cannot:
// VEGAS gives an Event FX the event already conformed to the render rate
// (60 fps) and snaps every in-between time to a whole frame, so a "blend"
// there would be a 4:1 double image of two output frames, not motion blur.
// The effect then passes the source through. As a Media FX the clip runs at
// the capture rate (300 fps) and the blend works.
bool can_blend(Instance *inst, const Settings &s) {
    if (s.step == blend::StepMode::OneHostFrame) return true;
    const double fps = clip_fps(inst->source);
    return !(fps > 0) || fps >= s.capture_fps * 0.99;
}

bool clip_range(OfxImageClipHandle clip, double &lo, double &hi) {
    OfxPropertySetHandle ps = nullptr;
    if (g_fx->clipGetPropertySet(clip, &ps) != kOfxStatOK) return false;
    double r[2] = {0, 0};
    if (g_prop->propGetDoubleN(ps, kOfxImageEffectPropFrameRange, 2, r) != kOfxStatOK) return false;
    lo = r[0];
    hi = r[1];
    return true;
}

// ---------------------------------------------------------------- actions

OfxStatus on_load() {
    g_prop = static_cast<const OfxPropertySuiteV1 *>(g_host->fetchSuite(g_host->host, kOfxPropertySuite, 1));
    g_fx = static_cast<const OfxImageEffectSuiteV1 *>(g_host->fetchSuite(g_host->host, kOfxImageEffectSuite, 1));
    g_param = static_cast<const OfxParameterSuiteV1 *>(g_host->fetchSuite(g_host->host, kOfxParameterSuite, 1));
    if (!g_prop || !g_fx || !g_param) return kOfxStatErrMissingHostFeature;
    logf("==== Frame Blend %d.%d loaded", kVersionMajor, kVersionMinor);
    log_props("host", g_host->host, kHostProps);
    return kOfxStatOK;
}

OfxStatus on_describe(OfxImageEffectHandle effect) {
    OfxPropertySetHandle ps = nullptr;
    g_fx->getPropertySet(effect, &ps);
    g_prop->propSetString(ps, kOfxPropLabel, 0, "Frame Blend");
    g_prop->propSetString(ps, kOfxPropShortLabel, 0, "Frame Blend");
    g_prop->propSetString(ps, kOfxPropLongLabel, 0, "Frame Blend (DoD Studio)");
    g_prop->propSetString(ps, kOfxPropPluginDescription, 0,
                          "Averages several source frames into each output frame: motion blur from a "
                          "high frame rate capture. In VEGAS, apply it as a Media FX: an Event FX only "
                          "sees the event at the project frame rate and passes it through.");
    g_prop->propSetString(ps, kOfxImageEffectPluginPropGrouping, 0, "DoD Studio");
    g_prop->propSetString(ps, kOfxImageEffectPropSupportedContexts, 0, kOfxImageEffectContextFilter);
    g_prop->propSetString(ps, kOfxImageEffectPropSupportedPixelDepths, 0, kOfxBitDepthByte);
    g_prop->propSetString(ps, kOfxImageEffectPropSupportedPixelDepths, 1, kOfxBitDepthShort);
    g_prop->propSetString(ps, kOfxImageEffectPropSupportedPixelDepths, 2, kOfxBitDepthFloat);
    g_prop->propSetInt(ps, kOfxImageEffectPropTemporalClipAccess, 0, 1);
    g_prop->propSetInt(ps, kOfxImageEffectPluginPropSingleInstance, 0, 0);
    g_prop->propSetInt(ps, kOfxImageEffectPluginPropHostFrameThreading, 0, 0);
    g_prop->propSetInt(ps, kOfxImageEffectPropSupportsMultiResolution, 0, 1);
    g_prop->propSetInt(ps, kOfxImageEffectPropSupportsTiles, 0, 0);
    g_prop->propSetInt(ps, kOfxImageEffectPropSupportsMultipleClipDepths, 0, 0);
    g_prop->propSetInt(ps, kOfxImageEffectPluginPropFieldRenderTwiceAlways, 0, 0);
    g_prop->propSetString(ps, kOfxImageEffectPluginRenderThreadSafety, 0, kOfxImageEffectRenderFullySafe);
    logf("describe");
    return kOfxStatOK;
}

void define_clip(OfxImageEffectHandle effect, const char *name, bool temporal) {
    OfxPropertySetHandle ps = nullptr;
    g_fx->clipDefine(effect, name, &ps);
    g_prop->propSetString(ps, kOfxImageEffectPropSupportedComponents, 0, kOfxImageComponentRGBA);
    g_prop->propSetInt(ps, kOfxImageEffectPropSupportsTiles, 0, 0);
    if (temporal) g_prop->propSetInt(ps, kOfxImageEffectPropTemporalClipAccess, 0, 1);
}

OfxPropertySetHandle define_param(OfxParamSetHandle params, const char *type, const char *name,
                                  const char *label, const char *hint) {
    OfxPropertySetHandle ps = nullptr;
    g_param->paramDefine(params, type, name, &ps);
    g_prop->propSetString(ps, kOfxPropLabel, 0, label);
    g_prop->propSetString(ps, kOfxParamPropHint, 0, hint);
    return ps;
}

OfxStatus on_describe_in_context(OfxImageEffectHandle effect, OfxPropertySetHandle in_args) {
    char *context = nullptr;
    g_prop->propGetString(in_args, kOfxImageEffectPropContext, 0, &context);
    logf("describe in context %s", context ? context : "?");
    define_clip(effect, kOfxImageEffectSimpleSourceClipName, true);
    define_clip(effect, kOfxImageEffectOutputClipName, false);

    OfxParamSetHandle params = nullptr;
    g_fx->getParamSet(effect, &params);

    OfxPropertySetHandle p = define_param(
        params, kOfxParamTypeInteger, kParamFrames, "Frames to blend",
        "How many captured frames make each output frame. 5 for a 300 fps capture at normal speed on a "
        "60 fps project, 2 for 2.5x slow motion, 1 turns the blend off (5x slow).");
    g_prop->propSetInt(p, kOfxParamPropDefault, 0, 5);
    g_prop->propSetInt(p, kOfxParamPropMin, 0, 1);
    g_prop->propSetInt(p, kOfxParamPropMax, 0, 16);
    g_prop->propSetInt(p, kOfxParamPropDisplayMin, 0, 1);
    g_prop->propSetInt(p, kOfxParamPropDisplayMax, 0, 16);
    g_prop->propSetInt(p, kOfxParamPropAnimates, 0, 1);

    p = define_param(params, kOfxParamTypeDouble, kParamCaptureFps, "Capture frame rate",
                     "The frame rate the clip was captured at. Sets the time between blended frames: "
                     "one captured frame.");
    g_prop->propSetString(p, kOfxParamPropDoubleType, 0, kOfxParamDoubleTypePlain);
    g_prop->propSetDouble(p, kOfxParamPropDefault, 0, 300.0);
    g_prop->propSetDouble(p, kOfxParamPropMin, 0, 1.0);
    g_prop->propSetDouble(p, kOfxParamPropMax, 0, 2000.0);
    g_prop->propSetDouble(p, kOfxParamPropDisplayMin, 0, 24.0);
    g_prop->propSetDouble(p, kOfxParamPropDisplayMax, 0, 600.0);
    g_prop->propSetInt(p, kOfxParamPropAnimates, 0, 0);

    p = define_param(params, kOfxParamTypeChoice, kParamWindow, "Frames used",
                     "Which captured frames each output frame blends: the frame and the ones after it "
                     "(as in Studio's #1039 sample), or the ones around it.");
    g_prop->propSetString(p, kOfxParamPropChoiceOption, 0, "From this frame on");
    g_prop->propSetString(p, kOfxParamPropChoiceOption, 1, "Centred on this frame");
    g_prop->propSetInt(p, kOfxParamPropDefault, 0, 0);
    g_prop->propSetInt(p, kOfxParamPropAnimates, 0, 0);

    p = define_param(params, kOfxParamTypeChoice, kParamStep, "Step between frames",
                     "One captured frame (from the capture frame rate) is right when the host counts time "
                     "in project frames. One host frame is for a host that counts time in captured frames.");
    g_prop->propSetString(p, kOfxParamPropChoiceOption, 0, "One captured frame");
    g_prop->propSetString(p, kOfxParamPropChoiceOption, 1, "One host frame");
    g_prop->propSetInt(p, kOfxParamPropDefault, 0, 0);
    g_prop->propSetInt(p, kOfxParamPropAnimates, 0, 0);

    p = define_param(params, kOfxParamTypeBoolean, kParamLog, "Write diagnostics log",
                     "Appends what the host passes to %TEMP%\\dodstudio_frame_blend.log.");
    g_prop->propSetInt(p, kOfxParamPropDefault, 0, 1);
    g_prop->propSetInt(p, kOfxParamPropAnimates, 0, 0);
    return kOfxStatOK;
}

void log_clip(const char *what, OfxImageClipHandle clip) {
    OfxPropertySetHandle ps = nullptr;
    if (g_fx->clipGetPropertySet(clip, &ps) == kOfxStatOK) log_props(what, ps, kClipProps);
}

OfxStatus on_create_instance(OfxImageEffectHandle effect) {
    Instance *inst = new Instance();
    inst->id = g_next_instance++;
    g_fx->clipGetHandle(effect, kOfxImageEffectSimpleSourceClipName, &inst->source, nullptr);
    g_fx->clipGetHandle(effect, kOfxImageEffectOutputClipName, &inst->output, nullptr);
    OfxParamSetHandle params = nullptr;
    g_fx->getParamSet(effect, &params);
    g_param->paramGetHandle(params, kParamFrames, &inst->frames, nullptr);
    g_param->paramGetHandle(params, kParamCaptureFps, &inst->capture_fps, nullptr);
    g_param->paramGetHandle(params, kParamWindow, &inst->window, nullptr);
    g_param->paramGetHandle(params, kParamStep, &inst->step, nullptr);
    g_param->paramGetHandle(params, kParamLog, &inst->log, nullptr);
    OfxPropertySetHandle ps = nullptr;
    g_fx->getPropertySet(effect, &ps);
    g_prop->propSetPointer(ps, kOfxPropInstanceData, 0, inst);
    logf("create instance #%d", inst->id);
    log_props("instance", ps, kInstanceProps);
    if (inst->source) log_clip("source", inst->source);
    if (inst->output) log_clip("output", inst->output);
    return kOfxStatOK;
}

OfxStatus on_destroy_instance(OfxImageEffectHandle effect) {
    Instance *inst = get_instance(effect);
    if (inst) logf("destroy instance #%d after %d renders", inst->id, inst->renders.load());
    delete inst;
    return kOfxStatOK;
}

// Filled in for hosts that ask; the samples span [time + first, time + last].
OfxStatus on_get_frames_needed(OfxImageEffectHandle effect, OfxPropertySetHandle in_args,
                               OfxPropertySetHandle out_args) {
    Instance *inst = get_instance(effect);
    if (!inst) return kOfxStatReplyDefault;
    double time = 0;
    g_prop->propGetDouble(in_args, kOfxPropTime, 0, &time);
    const Settings s = read_settings(inst, time);
    const double step = blend::step_between_samples(s.step, effect_fps(effect), s.capture_fps);
    const int n = can_blend(inst, s) ? s.frames : 1;
    const std::vector<double> ts = blend::sample_times(time, n, step, s.window, false, 0, 0);
    double range[2] = {ts.front(), ts.back()};
    g_prop->propSetDoubleN(out_args, "OfxImageClipPropFrameRange_Source", 2, range);
    if (s.log) logf("frames needed #%d t=%.6f -> [%.6f, %.6f]", inst->id, time, range[0], range[1]);
    return kOfxStatOK;
}

OfxStatus on_is_identity(OfxImageEffectHandle effect, OfxPropertySetHandle in_args,
                         OfxPropertySetHandle out_args) {
    Instance *inst = get_instance(effect);
    if (!inst) return kOfxStatReplyDefault;
    double time = 0;
    g_prop->propGetDouble(in_args, kOfxPropTime, 0, &time);
    const Settings s = read_settings(inst, time);
    const bool blend_ok = can_blend(inst, s);
    if (s.frames != 1 && blend_ok) return kOfxStatReplyDefault;
    if (!blend_ok && s.log && !inst->warned.exchange(true))
        logf("instance #%d: source clip runs at %.3f fps, below the %.3f fps capture: the in-between frames "
             "are not reachable here (VEGAS: use Frame Blend as a Media FX), passing through",
             inst->id, clip_fps(inst->source), s.capture_fps);
    g_prop->propSetString(out_args, kOfxPropName, 0, kOfxImageEffectSimpleSourceClipName);
    g_prop->propSetDouble(out_args, kOfxPropTime, 0, time);
    return kOfxStatOK;
}

struct Image {
    OfxPropertySetHandle handle = nullptr;
    blend::View view;
    std::string depth, components;
};

bool read_image(OfxPropertySetHandle h, Image &img) {
    img.handle = h;
    void *data = nullptr;
    int b[4] = {0, 0, 0, 0}, row_bytes = 0;
    char *depth = nullptr, *comps = nullptr;
    if (g_prop->propGetPointer(h, kOfxImagePropData, 0, &data) != kOfxStatOK || !data) return false;
    if (g_prop->propGetIntN(h, kOfxImagePropBounds, 4, b) != kOfxStatOK) return false;
    if (g_prop->propGetInt(h, kOfxImagePropRowBytes, 0, &row_bytes) != kOfxStatOK) return false;
    g_prop->propGetString(h, kOfxImageEffectPropPixelDepth, 0, &depth);
    g_prop->propGetString(h, kOfxImageEffectPropComponents, 0, &comps);
    img.view.data = static_cast<const uint8_t *>(data);
    img.view.x1 = b[0];
    img.view.y1 = b[1];
    img.view.x2 = b[2];
    img.view.y2 = b[3];
    img.view.row_bytes = row_bytes;
    img.depth = depth ? depth : "";
    img.components = comps ? comps : "";
    return true;
}

int bytes_per_channel(const std::string &depth) {
    if (depth == kOfxBitDepthByte) return 1;
    if (depth == kOfxBitDepthShort) return 2;
    if (depth == kOfxBitDepthFloat) return 4;
    return 0;
}

template <typename T>
void average_threaded(const std::vector<blend::View> &srcs, const blend::View &dst, const int win[4]) {
    const int rows = win[3] - win[1];
    unsigned n = std::thread::hardware_concurrency();
    if (n == 0) n = 4;
    n = std::min<unsigned>(n, static_cast<unsigned>(std::max(1, rows / 32)));
    if (n <= 1) {
        blend::average_rows<T>(srcs, dst, win[0], win[2], win[1], win[3]);
        return;
    }
    std::vector<std::thread> threads;
    for (unsigned i = 0; i < n; ++i) {
        const int lo = win[1] + static_cast<int>(rows * static_cast<long long>(i) / n);
        const int hi = win[1] + static_cast<int>(rows * static_cast<long long>(i + 1) / n);
        threads.emplace_back([&srcs, &dst, win, lo, hi] { blend::average_rows<T>(srcs, dst, win[0], win[2], lo, hi); });
    }
    for (auto &t : threads) t.join();
}

OfxStatus on_render(OfxImageEffectHandle effect, OfxPropertySetHandle in_args) {
    Instance *inst = get_instance(effect);
    if (!inst || !inst->source || !inst->output) return kOfxStatErrBadHandle;
    double time = 0;
    int win[4] = {0, 0, 0, 0};
    g_prop->propGetDouble(in_args, kOfxPropTime, 0, &time);
    g_prop->propGetIntN(in_args, kOfxImageEffectPropRenderWindow, 4, win);

    const Settings s = read_settings(inst, time);
    g_log_enabled = s.log;
    const int render_no = inst->renders++;
    // Every render for the first 300, then one in 60: enough to see a whole
    // short event without growing the log by megabytes per minute.
    const bool log_this = s.log && (render_no < 300 || render_no % 60 == 0);

    const double fps = effect_fps(effect);
    const double step = blend::step_between_samples(s.step, fps, s.capture_fps);
    double lo = 0, hi = 0;
    const bool have_range = clip_range(inst->source, lo, hi);
    const bool blend_ok = can_blend(inst, s);
    const std::vector<double> times =
        blend::sample_times(time, blend_ok ? s.frames : 1, step, s.window, have_range, lo, hi);

    if (log_this) {
        if (!blend_ok)
            logf("render #%d.%d: source clip runs at %.3f fps, below the %.3f fps capture: the in-between "
                 "frames are not reachable here (VEGAS: use Frame Blend as a Media FX), passing through",
                 inst->id, render_no, clip_fps(inst->source), s.capture_fps);
        logf("render #%d.%d t=%.6f frames=%d step=%.6f (effect fps %.4f, capture fps %.3f, %s, %s) range %s",
             inst->id, render_no, time, s.frames, step, fps, s.capture_fps,
             s.window == blend::Window::Forward ? "forward" : "centred",
             s.step == blend::StepMode::OneCapturedFrame ? "captured-frame step" : "host-frame step",
             have_range ? (std::to_string(lo) + ".." + std::to_string(hi)).c_str() : "(none)");
        if (render_no == 0) {
            log_props("render args", in_args, kRenderArgs);
            log_clip("source@render", inst->source);
            OfxPropertySetHandle ps = nullptr;
            g_fx->getPropertySet(effect, &ps);
            log_props("instance@render", ps, kInstanceProps);
        }
    }

    OfxPropertySetHandle out_h = nullptr;
    if (g_fx->clipGetImage(inst->output, time, nullptr, &out_h) != kOfxStatOK) {
        if (log_this) logf("  output image fetch failed");
        return kOfxStatFailed;
    }
    Image out;
    if (!read_image(out_h, out)) {
        g_fx->clipReleaseImage(out_h);
        return kOfxStatFailed;
    }
    const int bpc = bytes_per_channel(out.depth);
    if (bpc == 0 || out.components != kOfxImageComponentRGBA) {
        if (log_this) logf("  unsupported output %s %s", out.depth.c_str(), out.components.c_str());
        g_fx->clipReleaseImage(out_h);
        return kOfxStatErrUnsupported;
    }

    std::vector<Image> srcs;
    std::string sample_log;
    std::vector<uint64_t> hashes;
    for (double t : times) {
        OfxPropertySetHandle h = nullptr;
        const OfxStatus st = g_fx->clipGetImage(inst->source, t, nullptr, &h);
        char tmp[160];
        if (st != kOfxStatOK || !h) {
            snprintf(tmp, sizeof tmp, " %.4f:fail(%d)", t, st);
            sample_log += tmp;
            continue;
        }
        Image img;
        if (!read_image(h, img) || img.depth != out.depth || img.components != out.components) {
            snprintf(tmp, sizeof tmp, " %.4f:skip(%s %s)", t, img.depth.c_str(), img.components.c_str());
            sample_log += tmp;
            g_fx->clipReleaseImage(h);
            continue;
        }
        if (log_this) {
            const uint64_t hash = image_hash(img.view, bpc * 4);
            hashes.push_back(hash);
            snprintf(tmp, sizeof tmp, " %.4f:%04llx", t, static_cast<unsigned long long>(hash & 0xffff));
            sample_log += tmp;
            if (render_no == 0 && srcs.empty()) log_props("source image", h, kImageProps);
        }
        srcs.push_back(img);
    }
    if (log_this) {
        std::vector<uint64_t> uniq = hashes;
        std::sort(uniq.begin(), uniq.end());
        uniq.erase(std::unique(uniq.begin(), uniq.end()), uniq.end());
        logf("  samples%s -> %zu fetched, %zu distinct", sample_log.c_str(), srcs.size(), uniq.size());
        if (render_no == 0) log_props("output image", out_h, kImageProps);
    }

    std::vector<blend::View> views;
    for (const Image &i : srcs) views.push_back(i.view);
    if (bpc == 1) average_threaded<uint8_t>(views, out.view, win);
    else if (bpc == 2) average_threaded<uint16_t>(views, out.view, win);
    else average_threaded<float>(views, out.view, win);

    for (const Image &i : srcs) g_fx->clipReleaseImage(i.handle);
    g_fx->clipReleaseImage(out_h);
    return kOfxStatOK;
}

OfxStatus main_entry(const char *action, const void *handle, OfxPropertySetHandle in_args,
                     OfxPropertySetHandle out_args) {
    OfxImageEffectHandle effect = (OfxImageEffectHandle)handle;
    if (!strcmp(action, kOfxActionLoad)) return on_load();
    if (!g_prop) return kOfxStatErrFatal;
    if (!strcmp(action, kOfxActionUnload)) return kOfxStatOK;
    if (!strcmp(action, kOfxActionDescribe)) return on_describe(effect);
    if (!strcmp(action, kOfxImageEffectActionDescribeInContext)) return on_describe_in_context(effect, in_args);
    if (!strcmp(action, kOfxActionCreateInstance)) return on_create_instance(effect);
    if (!strcmp(action, kOfxActionDestroyInstance)) return on_destroy_instance(effect);
    if (!strcmp(action, kOfxImageEffectActionRender)) return on_render(effect, in_args);
    if (!strcmp(action, kOfxImageEffectActionIsIdentity)) return on_is_identity(effect, in_args, out_args);
    if (!strcmp(action, kOfxImageEffectActionGetFramesNeeded))
        return on_get_frames_needed(effect, in_args, out_args);
    if (!strcmp(action, kOfxImageEffectActionBeginSequenceRender)) {
        logf("begin sequence render");
        log_props("sequence args", in_args, kSequenceArgs);
        return kOfxStatOK;
    }
    if (!strcmp(action, kOfxImageEffectActionEndSequenceRender)) {
        logf("end sequence render");
        return kOfxStatOK;
    }
    if (!strcmp(action, kOfxImageEffectActionGetClipPreferences) ||
        !strcmp(action, kOfxImageEffectActionGetTimeDomain) ||
        !strcmp(action, kOfxActionInstanceChanged)) {
        logf("action %s", action);
        if (!strcmp(action, kOfxActionInstanceChanged)) {
            char *name = nullptr, *reason = nullptr;
            g_prop->propGetString(in_args, kOfxPropName, 0, &name);
            g_prop->propGetString(in_args, kOfxPropChangeReason, 0, &reason);
            logf("  changed %s (%s)", name ? name : "?", reason ? reason : "?");
        }
        return kOfxStatReplyDefault;
    }
    // Everything else gets the host's default; the first few are logged so a
    // log shows which other actions the host sends.
    static std::atomic<int> unhandled{0};
    if (unhandled++ < 200) logf("action %s (host default)", action);
    return kOfxStatReplyDefault;
}

void set_host(OfxHost *host) { g_host = host; }

OfxPlugin g_plugin = {kOfxImageEffectPluginApi, 1, kPluginId, kVersionMajor, kVersionMinor, set_host, main_entry};

}  // namespace

OfxExport int OfxGetNumberOfPlugins(void) { return 1; }
OfxExport OfxPlugin *OfxGetPlugin(int nth) { return nth == 0 ? &g_plugin : nullptr; }
