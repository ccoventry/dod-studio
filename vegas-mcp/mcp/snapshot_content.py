"""The snapshot tool's frames as MCP image content (#1050), so Claude sees each
frame in the tool result itself instead of only a path.

MCP tool results carry images as {"type": "image", "data": <base64>,
"mimeType": ...} content blocks (in every protocol version since 2024-11-05).
The bridge sends each frame's inline copy in its reply (the PNG when it is
small, else a JPEG of the same pixels: extension/FrameImage.cs); this moves
those copies out of the reply's text into image blocks, under a cap for the
whole call, since every image in a conversation is sent again with each later
request. A frame left out is named in the text, and its PNG is still on disk.
"""

# Raw image bytes in one call's result; base64 adds a third. A 1280 px game
# frame is about 0.3 MB as the bridge's JPEG. The bridge stops at the same
# total (FrameImage.InlineCallMaxBytes); this cap still guards the result.
MAX_TOTAL_BYTES = 4_000_000


def image_blocks(snapshots, max_total_bytes=MAX_TOTAL_BYTES):
    """The image blocks for a snapshot reply's `snapshots` list, in its order.
    Takes each shot's `inline` copy out of it (so the text stays small) and
    leaves `inline_as` (the image's type) or `not_inline` (why not) instead."""
    blocks = []
    total = 0
    for shot in snapshots:
        if not isinstance(shot, dict):
            continue
        copy = shot.pop("inline", None)
        why = left_out_because(copy, total, max_total_bytes)
        if why:
            shot["not_inline"] = why
            continue
        total += raw_size(copy["data"])
        shot["inline_as"] = copy["mime_type"]
        if copy.get("width") and shot.get("width") and copy["width"] != shot["width"]:
            # A frame wider than the bridge's inline cap goes inline scaled down;
            # its PNG on disk keeps the asked-for width.
            shot["inline_width"] = copy["width"]
        blocks.append({"type": "image", "data": copy["data"], "mimeType": copy["mime_type"]})
    return blocks


def left_out_because(copy, total, max_total_bytes):
    """Why an inline copy cannot go in the result, or None when it can."""
    if not isinstance(copy, dict):
        return "the bridge sent no inline copy"
    if copy.get("left_out"):
        return copy["left_out"]
    if not isinstance(copy.get("data"), str) or copy.get("mime_type") not in ("image/png", "image/jpeg"):
        return "the bridge's inline copy is not a PNG or JPEG"
    if total + raw_size(copy["data"]) > max_total_bytes:
        return "the call's images would pass %d bytes; ask for fewer times or a smaller width" % max_total_bytes
    return None


def raw_size(base64_text):
    """The decoded size of base64 text, without decoding it."""
    return len(base64_text) * 3 // 4
