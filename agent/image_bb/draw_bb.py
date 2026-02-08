#!/usr/bin/env python3
"""
Show bounding boxes from a JSON file on top of an image (no output file written).

Supports either:
- {"boxes":[{"label":"car","bbox_2d":[x1,y1,x2,y2]}, ...]}
- {"blocks":[{"text":"...", "bbox_2d":[x1,y1,x2,y2]}, ...]}

Assumes bbox_2d is normalized to a 1000x1000 coordinate system (0..1000).
"""

import argparse
import json
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont


def load_json(path: Path) -> dict:
    with path.open("r", encoding="utf-8") as f:
        return json.load(f)


def get_items(data: dict):
    if isinstance(data, dict):
        if "boxes" in data and isinstance(data["boxes"], list):
            return data["boxes"], "boxes"
        if "blocks" in data and isinstance(data["blocks"], list):
            return data["blocks"], "blocks"
    raise ValueError("JSON must contain a top-level 'boxes' or 'blocks' array.")


def clamp(v: float, lo: float, hi: float) -> float:
    return max(lo, min(hi, v))


def to_px_bbox(bbox_2d, W: int, H: int):
    if not (isinstance(bbox_2d, (list, tuple)) and len(bbox_2d) == 4):
        raise ValueError(f"bbox_2d must be [x1,y1,x2,y2], got: {bbox_2d}")

    x1, y1, x2, y2 = bbox_2d
    # Handle stringified numbers just in case
    x1, y1, x2, y2 = float(x1), float(y1), float(x2), float(y2)

    # Normalize range 0..1000 -> pixels
    x1 = clamp(x1, 0, 1000) / 1000.0 * W
    x2 = clamp(x2, 0, 1000) / 1000.0 * W
    y1 = clamp(y1, 0, 1000) / 1000.0 * H
    y2 = clamp(y2, 0, 1000) / 1000.0 * H

    # Ensure proper ordering
    left, right = sorted([x1, x2])
    top, bottom = sorted([y1, y2])

    return int(round(left)), int(round(top)), int(round(right)), int(round(bottom))


def draw_label(draw: ImageDraw.ImageDraw, x: int, y: int, text: str, font, bg=(255, 0, 0)):
    # Background rectangle for text
    tb = draw.textbbox((0, 0), text, font=font)
    tw, th = tb[2] - tb[0], tb[3] - tb[1]
    pad = 4
    y0 = max(0, y - th - 2 * pad)
    draw.rectangle([x, y0, x + tw + 2 * pad, y], fill=bg)
    draw.text((x + pad, y0 + pad), text, fill=(0, 0, 0), font=font)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--image", required=True, help="Path to image (jpg/png/...)")
    ap.add_argument("--json", required=True, help="Path to JSON output (boxes/blocks)")
    ap.add_argument("--line-width", type=int, default=5)
    args = ap.parse_args()

    img = Image.open(args.image).convert("RGB")
    W, H = img.size
    data = load_json(Path(args.json))
    items, kind = get_items(data)

    draw = ImageDraw.Draw(img)

    # Font (optional)
    try:
        font = ImageFont.truetype("DejaVuSans.ttf", 22)
    except Exception:
        font = ImageFont.load_default()

    # Simple color map
    color_map = {
        "person": (255, 0, 0),
        "car": (0, 120, 255),
        "text": (255, 0, 0),
    }

    for it in items:
        bbox = it.get("bbox_2d")
        if bbox is None:
            continue

        x1, y1, x2, y2 = to_px_bbox(bbox, W, H)

        label = ""
        if kind == "boxes":
            label = str(it.get("label", ""))
        elif kind == "blocks":
            label = str(it.get("text", ""))

        # Pick color
        key = it.get("label", "text") if kind == "boxes" else "text"
        color = color_map.get(str(key), (255, 255, 0))

        draw.rectangle([x1, y1, x2, y2], outline=color, width=args.line_width)
        if label:
            draw_label(draw, x1, y1, label, font, bg=color)

    # Show without saving
    img.show()


if __name__ == "__main__":
    main()
