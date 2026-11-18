"""Regenerate the watch app icons from the iOS app icon artwork.

    python3 scripts/generate_watch_icons.py

Writes
  watch/apple/GS26Watch/Assets.xcassets/AppIcon.appiconset/AppIcon.png
      1024x1024, opaque RGB (watchOS rejects alpha and masks the icon to a circle)
  watch/wear/app/src/main/res/mipmap-<density>/ic_launcher_foreground.png
      adaptive-icon foreground, 108dp at each density

Both watches crop the icon to a circle, but the iOS artwork is a wide logo
that runs to the square's left and right edges. The logo is therefore cut out
of its flat background, scaled so every logo pixel lies inside the circle
(Wear: inside the 66dp adaptive-icon safe zone) and drawn on the same
background colour. Needs Pillow."""
from pathlib import Path
import math
import sys

from PIL import Image

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "assets" / "icon_1024x1024.png"
WATCHOS_ICON = ROOT / "watch/apple/GS26Watch/Assets.xcassets/AppIcon.appiconset/AppIcon.png"
WEAR_RES = ROOT / "watch/wear/app/src/main/res"
# Background colour of the iOS icon; also @color/ic_launcher_background.
BACKGROUND = (0x1E, 0x1F, 0x22)
WEAR_DENSITIES = {"mdpi": 1.0, "hdpi": 1.5, "xhdpi": 2.0, "xxhdpi": 3.0, "xxxhdpi": 4.0}
# Logo radius as a fraction of the visible circle's radius.
WATCHOS_FILL = 0.90
# Adaptive icon: 108dp layer, 66dp safe-zone circle.
WEAR_FILL = 0.95 * 33 / 54


def logo(source=SOURCE):
    """The logo cropped to its bounding box and the radius (in source pixels)
    of the smallest circle about the box centre that holds every logo pixel."""
    img = Image.open(source).convert("RGB")
    px = img.load()
    width, height = img.size
    points = [(x, y) for y in range(height) for x in range(width)
              if max(abs(a - b) for a, b in zip(px[x, y], BACKGROUND)) > 12]
    if not points:
        raise SystemExit(f"{source}: no artwork on the background colour {BACKGROUND}")
    left = min(x for x, _ in points); right = max(x for x, _ in points) + 1
    top = min(y for _, y in points); bottom = max(y for _, y in points) + 1
    cx, cy = (left + right) / 2, (top + bottom) / 2
    radius = max(math.hypot(x + 0.5 - cx, y + 0.5 - cy) for x, y in points)
    return img.crop((left, top, right, bottom)), radius


def render(crop, radius, size, fill):
    """`size` square opaque icon with the logo centred inside a circle of
    radius fill * size / 2."""
    scale = fill * (size / 2) / radius
    logo_size = (max(1, round(crop.width * scale)), max(1, round(crop.height * scale)))
    canvas = Image.new("RGB", (size, size), BACKGROUND)
    canvas.paste(crop.resize(logo_size, Image.LANCZOS),
                 ((size - logo_size[0]) // 2, (size - logo_size[1]) // 2))
    return canvas


def main():
    crop, radius = logo()
    WATCHOS_ICON.parent.mkdir(parents=True, exist_ok=True)
    render(crop, radius, 1024, WATCHOS_FILL).save(WATCHOS_ICON, format="PNG", optimize=True)
    print(WATCHOS_ICON.relative_to(ROOT))
    for density, factor in WEAR_DENSITIES.items():
        out = WEAR_RES / f"mipmap-{density}" / "ic_launcher_foreground.png"
        out.parent.mkdir(parents=True, exist_ok=True)
        render(crop, radius, round(108 * factor), WEAR_FILL).save(out, format="PNG", optimize=True)
        print(out.relative_to(ROOT))


if __name__ == "__main__":
    sys.exit(main())
