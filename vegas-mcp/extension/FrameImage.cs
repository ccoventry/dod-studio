// A snapshot's pixels: VEGAS's RGBA PNG made opaque and scaled for the file
// on disk, and the copy sent inline in the tool result (#1050).
//
// The timeline outside any event comes out transparent; a viewer can show
// that as white or a checkerboard, so the frame is composited over black, as
// a render shows it. The inline copy is the PNG itself when it is small (a
// scaled-down frame, a black gap), else a JPEG of the same pixels: a 1280 px
// game frame is about 2.5 MB as PNG and 0.3 MB as JPEG, and every image
// in a conversation is sent again with each later request.
using System;
using System.Collections.Generic;
using System.IO;
using System.Runtime.InteropServices;
using Gdi = System.Drawing;
using GdiImaging = System.Drawing.Imaging;

namespace DodStudio.VegasBridge {

public static class FrameImage {
    // Raw bytes of one inline image; base64 adds a third.
    public const int InlineMaxBytes = 1000000;
    static readonly long[] JpegQualities = { 90, 75 };

    // VEGAS's PNG at `rawPath`, composited over black, at `width` (aspect
    // kept; 0, or a width at or above the frame's, keeps its size).
    public static Gdi.Bitmap Opaque(string rawPath, int width) {
        // Read through a copy in memory: a Bitmap opened on the file keeps it
        // locked, and the raw file is deleted next.
        using (MemoryStream bytes = new MemoryStream(File.ReadAllBytes(rawPath)))
        using (Gdi.Bitmap src = new Gdi.Bitmap(bytes)) {
            Gdi.Bitmap flat = OnBlack(src);
            if (width <= 0 || width >= flat.Width) return flat;
            using (flat) {
                int h = Math.Max(1, (int)Math.Round(flat.Height * (double)width / flat.Width));
                return Scaled(flat, width, h);
            }
        }
    }

    public static byte[] Png(Gdi.Bitmap frame) {
        using (MemoryStream m = new MemoryStream()) {
            frame.Save(m, GdiImaging.ImageFormat.Png);
            return m.ToArray();
        }
    }

    // {"mime_type", "data" (base64)} for the tool result, or {"left_out": why}.
    public static Dictionary<string, object> InlineCopy(Gdi.Bitmap frame, byte[] png) {
        Dictionary<string, object> r = new Dictionary<string, object>();
        if (png.Length <= InlineMaxBytes) {
            r["mime_type"] = "image/png";
            r["data"] = Convert.ToBase64String(png);
            return r;
        }
        foreach (long quality in JpegQualities) {
            byte[] jpeg = Jpeg(frame, quality);
            if (jpeg.Length > InlineMaxBytes) continue;
            r["mime_type"] = "image/jpeg";
            r["jpeg_quality"] = quality;
            r["data"] = Convert.ToBase64String(jpeg);
            return r;
        }
        r["left_out"] = "over " + InlineMaxBytes + " bytes even as a JPEG; ask for a smaller width";
        return r;
    }

    static byte[] Jpeg(Gdi.Bitmap frame, long quality) {
        GdiImaging.ImageCodecInfo codec = null;
        foreach (GdiImaging.ImageCodecInfo c in GdiImaging.ImageCodecInfo.GetImageEncoders())
            if (c.FormatID == GdiImaging.ImageFormat.Jpeg.Guid) codec = c;
        using (GdiImaging.EncoderParameters ps = new GdiImaging.EncoderParameters(1))
        using (MemoryStream m = new MemoryStream()) {
            ps.Param[0] = new GdiImaging.EncoderParameter(GdiImaging.Encoder.Quality, quality);
            frame.Save(m, codec, ps);
            return m.ToArray();
        }
    }

    // The image over black, as 24-bit RGB: each channel times its alpha (PNG
    // alpha is straight), so an opaque pixel is copied exactly.
    static Gdi.Bitmap OnBlack(Gdi.Bitmap src) {
        Gdi.Rectangle all = new Gdi.Rectangle(0, 0, src.Width, src.Height);
        Gdi.Bitmap dst = new Gdi.Bitmap(src.Width, src.Height, GdiImaging.PixelFormat.Format24bppRgb);
        GdiImaging.BitmapData from = src.LockBits(all, GdiImaging.ImageLockMode.ReadOnly, GdiImaging.PixelFormat.Format32bppArgb);
        GdiImaging.BitmapData to = dst.LockBits(all, GdiImaging.ImageLockMode.WriteOnly, GdiImaging.PixelFormat.Format24bppRgb);
        try {
            byte[] row = new byte[src.Width * 4];
            byte[] outRow = new byte[to.Stride];
            for (int y = 0; y < src.Height; y++) {
                Marshal.Copy(new IntPtr(from.Scan0.ToInt64() + (long)y * from.Stride), row, 0, row.Length);
                for (int x = 0; x < src.Width; x++) {
                    int alpha = row[x * 4 + 3];
                    for (int c = 0; c < 3; c++) outRow[x * 3 + c] = (byte)((row[x * 4 + c] * alpha + 127) / 255);
                }
                Marshal.Copy(outRow, 0, new IntPtr(to.Scan0.ToInt64() + (long)y * to.Stride), outRow.Length);
            }
        } finally {
            src.UnlockBits(from);
            dst.UnlockBits(to);
        }
        return dst;
    }

    // Bicubic with mirrored edges, so the border does not darken from
    // sampling outside the frame.
    static Gdi.Bitmap Scaled(Gdi.Bitmap src, int w, int h) {
        Gdi.Bitmap dst = new Gdi.Bitmap(w, h, GdiImaging.PixelFormat.Format24bppRgb);
        using (Gdi.Graphics g = Gdi.Graphics.FromImage(dst))
        using (GdiImaging.ImageAttributes edges = new GdiImaging.ImageAttributes()) {
            g.InterpolationMode = Gdi.Drawing2D.InterpolationMode.HighQualityBicubic;
            g.PixelOffsetMode = Gdi.Drawing2D.PixelOffsetMode.HighQuality;
            edges.SetWrapMode(Gdi.Drawing2D.WrapMode.TileFlipXY);
            g.DrawImage(src, new Gdi.Rectangle(0, 0, w, h), 0, 0, src.Width, src.Height, Gdi.GraphicsUnit.Pixel, edges);
        }
        return dst;
    }
}

}
