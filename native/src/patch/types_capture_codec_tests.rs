use super::CaptureCodec;

const ALL: [CaptureCodec; 4] = [
    CaptureCodec::UtVideo,
    CaptureCodec::Ffv1,
    CaptureCodec::X264Lossless,
    CaptureCodec::RawVideo,
];

#[test]
fn test_str_id_round_trips_through_every_codec() {
    // Settings persist the string id, so these must stay inverses or a
    // saved choice silently reverts to the default on next launch.
    for codec in ALL {
        assert_eq!(CaptureCodec::from_str_id(codec.to_str_id()), codec);
    }
}

#[test]
fn test_unknown_id_falls_back_to_default() {
    // A settings file naming a codec this build does not have must not
    // stop a capture.
    assert_eq!(CaptureCodec::from_str_id("h265"), CaptureCodec::default());
    assert_eq!(CaptureCodec::from_str_id(""), CaptureCodec::default());
}

#[test]
fn test_every_codec_pins_an_explicit_pixel_format() {
    // hudAlpha carries the HUD matte in its red channel and the renderer
    // reads it with extractplanes=r. Leaving pix_fmt to FFmpeg's
    // negotiation is what would silently destroy that.
    for codec in ALL {
        assert!(
            codec.args().contains("-pix_fmt"),
            "{:?} must pin a pixel format",
            codec
        );
    }
}

#[test]
fn test_no_codec_uses_chroma_subsampling() {
    // Any yuv420p/yuv422p entry would wreck the HUD alpha with no error
    // anywhere — the failure would only show up in a finished render.
    for codec in ALL {
        let args = codec.args();
        assert!(
            !args.contains("yuv420") && !args.contains("yuv422"),
            "{:?} must stay RGB/4:4:4, got {}",
            codec,
            args
        );
    }
}

#[test]
fn test_no_codec_is_lossy() {
    // The render pass always re-encodes, so a lossy capture codec costs
    // quality for no benefit. x264 is admitted only at -qp 0.
    for codec in ALL {
        let args = codec.args();
        if args.contains("libx264") {
            assert!(args.contains("-qp 0"), "x264 must be lossless: {}", args);
        }
    }
}

#[test]
fn test_default_is_the_one_proven_in_a_real_capture() {
    assert_eq!(CaptureCodec::default(), CaptureCodec::UtVideo);
}
