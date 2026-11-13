// Host-independent parts of the frame blend: which times to sample and how to
// average the fetched images. Kept apart from the OFX glue so the maths reads
// on its own.
#pragma once

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <vector>

namespace blend {

enum class Window { Forward = 0, Centred = 1 };
enum class StepMode { OneCapturedFrame = 0, OneHostFrame = 1 };

// Distance between two blended samples, in host time (OFX time is measured in
// frames of the effect's own rate). A 300 fps capture on a 60 fps project is
// 0.2 host frames per captured frame.
inline double step_between_samples(StepMode mode, double effect_fps, double capture_fps) {
    if (mode == StepMode::OneHostFrame) return 1.0;
    if (!(effect_fps > 0.0) || !(capture_fps > 0.0)) return 1.0;
    return std::min(1.0, effect_fps / capture_fps);
}

// The n sample times for an output frame at `time`.
// Forward: time, time+step, ... (#1039's FFmpeg sample: output k = mean of
// captured frames 5k..5k+4). Centred: as many before as after, one extra
// before when n is even, so every offset is a whole number of captured frames.
// When the render time is inside the clip's frame range, samples past either
// end repeat the end frame instead of fetching nothing.
inline std::vector<double> sample_times(double time, int n, double step, Window window,
                                        bool have_range, double range_lo, double range_hi) {
    std::vector<double> out;
    if (n < 1) n = 1;
    const int first = window == Window::Forward ? 0 : -(n / 2);
    const bool clamp = have_range && range_hi >= range_lo && time >= range_lo && time <= range_hi;
    for (int k = 0; k < n; ++k) {
        double t = time + (first + k) * step;
        if (clamp) t = std::min(range_hi, std::max(range_lo, t));
        out.push_back(t);
    }
    return out;
}

// One source image as the averaging loop sees it.
struct View {
    const uint8_t *data = nullptr;  // pixel (bounds.x1, bounds.y1)
    int x1 = 0, y1 = 0, x2 = 0, y2 = 0;  // bounds, x2/y2 exclusive
    long long row_bytes = 0;  // may be negative (bottom-up images)
};

template <typename T> inline float to_float(T v) { return static_cast<float>(v); }

template <typename T> inline T from_mean(float mean);
template <> inline uint8_t from_mean<uint8_t>(float m) {
    return static_cast<uint8_t>(std::min(255.0f, std::max(0.0f, m + 0.5f)));
}
template <> inline uint16_t from_mean<uint16_t>(float m) {
    return static_cast<uint16_t>(std::min(65535.0f, std::max(0.0f, m + 0.5f)));
}
template <> inline float from_mean<float>(float m) { return m; }

// Averages rows [y_lo, y_hi) of the render window [x_lo, x_hi) from `srcs`
// into `dst`, 4 channels per pixel. A pixel takes the mean of the sources
// whose bounds hold it; with none, it is written transparent black.
template <typename T>
void average_rows(const std::vector<View> &srcs, const View &dst, int x_lo, int x_hi, int y_lo,
                  int y_hi) {
    const int w = x_hi - x_lo;
    if (w <= 0) return;
    std::vector<float> acc(static_cast<size_t>(w) * 4);
    std::vector<uint8_t> cnt(static_cast<size_t>(w));
    for (int y = y_lo; y < y_hi; ++y) {
        if (y < dst.y1 || y >= dst.y2) continue;
        std::fill(acc.begin(), acc.end(), 0.0f);
        std::fill(cnt.begin(), cnt.end(), static_cast<uint8_t>(0));
        for (const View &s : srcs) {
            if (y < s.y1 || y >= s.y2) continue;
            const int a = std::max(x_lo, s.x1), b = std::min(x_hi, s.x2);
            if (a >= b) continue;
            const T *row = reinterpret_cast<const T *>(s.data + (y - s.y1) * s.row_bytes) +
                           static_cast<size_t>(a - s.x1) * 4;
            float *ap = acc.data() + static_cast<size_t>(a - x_lo) * 4;
            for (int x = a; x < b; ++x, row += 4, ap += 4) {
                ap[0] += to_float(row[0]);
                ap[1] += to_float(row[1]);
                ap[2] += to_float(row[2]);
                ap[3] += to_float(row[3]);
                ++cnt[x - x_lo];
            }
        }
        const int a = std::max(x_lo, dst.x1), b = std::min(x_hi, dst.x2);
        T *out = reinterpret_cast<T *>(const_cast<uint8_t *>(dst.data) + (y - dst.y1) * dst.row_bytes) +
                 static_cast<size_t>(a - dst.x1) * 4;
        for (int x = a; x < b; ++x, out += 4) {
            const int c = cnt[x - x_lo];
            const float *ap = acc.data() + static_cast<size_t>(x - x_lo) * 4;
            if (c == 0) {
                out[0] = out[1] = out[2] = out[3] = T(0);
                continue;
            }
            const float inv = 1.0f / static_cast<float>(c);
            out[0] = from_mean<T>(ap[0] * inv);
            out[1] = from_mean<T>(ap[1] * inv);
            out[2] = from_mean<T>(ap[2] * inv);
            out[3] = from_mean<T>(ap[3] * inv);
        }
    }
}

}  // namespace blend
