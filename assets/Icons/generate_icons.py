#!/usr/bin/env python3
"""Generate BaseOS installer icons from the BaseOS boot-splash design language.

Palette sampled from pvaibhav/BaseOS assets/bootlogo.bmp:
  background  #0b0e11 -> #090b0d  (cool near-black, subtle vertical falloff)
  wordmark    #646c70               (muted cool grey, very low contrast)

The splash can afford near-invisible contrast; an app icon cannot, so the
mark is lifted to #b4bcc2 while keeping the same cool-grey cast.
"""
import os

from PIL import Image, ImageDraw, ImageFont

M = 2048  # master render size (supersampled, downsampled with LANCZOS)

BG_TOP = (14, 18, 22)
BG_BOT = (7, 9, 11)
BORDER = (32, 38, 44)
MARK = (180, 188, 194)
RULE = (106, 114, 120)

# A light geometric/humanist sans matching the boot splash. Noto Sans ships on
# most Linux distros; substitute any comparable family if these are absent.
FONT_CANDIDATES = {
    "light": ["NotoSans-Light.ttf", "DejaVuSans-ExtraLight.ttf", "Inter-Light.ttf"],
    "regular": ["NotoSans-Regular.ttf", "DejaVuSans.ttf", "Inter-Regular.ttf"],
}
SEARCH_DIRS = [
    "/usr/share/fonts/noto", "/usr/share/fonts/truetype/noto",
    "/usr/share/fonts/TTF", "/usr/share/fonts/truetype/dejavu",
    "/Library/Fonts", "C:/Windows/Fonts",
]


def find_font(kind):
    for name in FONT_CANDIDATES[kind]:
        for d in SEARCH_DIRS:
            path = os.path.join(d, name)
            if os.path.exists(path):
                return path
    raise SystemExit(f"No {kind} font found; edit FONT_CANDIDATES/SEARCH_DIRS.")


def vertical_gradient(size, top, bot):
    strip = Image.new("RGB", (1, size))
    px = strip.load()
    for y in range(size):
        t = y / max(1, size - 1)
        px[0, y] = tuple(round(a + (b - a) * t) for a, b in zip(top, bot))
    return strip.resize((size, size), Image.BILINEAR)


def draw_mark(img, *, font_path, cap_frac, with_rule, mark_color, rule_color):
    """Draw the 'B' monogram, optionally over a 'base' rule, centred as a group."""
    d = ImageDraw.Draw(img)

    # Size the glyph so its cap height is cap_frac of the canvas
    probe = ImageFont.truetype(font_path, 1000)
    pb = probe.getbbox("B")
    cap_h = pb[3] - pb[1]
    fs = round(1000 * (M * cap_frac) / cap_h)

    font = ImageFont.truetype(font_path, fs)
    bb = font.getbbox("B")
    gw, gh = bb[2] - bb[0], bb[3] - bb[1]

    if with_rule:
        # The rule must sit clearly wider than the glyph and far enough below it,
        # or it reads as an underline instead of a base the letter rests on.
        gap = round(M * 0.080)
        rule_h = round(M * 0.011)
        rule_w = round(M * 0.50)
        total_h = gh + gap + rule_h
    else:
        gap = rule_h = rule_w = 0
        total_h = gh

    top = (M - total_h) / 2
    # getbbox offsets are relative to the text anchor, so subtract them
    d.text((M / 2 - gw / 2 - bb[0], top - bb[1]), "B", font=font, fill=mark_color)

    if with_rule:
        ry = top + gh + gap
        d.rounded_rectangle(
            [M / 2 - rule_w / 2, ry, M / 2 + rule_w / 2, ry + rule_h],
            radius=rule_h / 2,
            fill=rule_color,
        )


def tile(detailed=True):
    """Self-contained app icon: rounded near-black tile with the mark."""
    radius = round(M * 0.219)

    mask = Image.new("L", (M, M), 0)
    ImageDraw.Draw(mask).rounded_rectangle([0, 0, M - 1, M - 1], radius=radius, fill=255)

    img = vertical_gradient(M, BG_TOP, BG_BOT).convert("RGBA")
    img.putalpha(mask)

    if detailed:
        # Hairline edge so the tile reads against pure-black backgrounds
        ImageDraw.Draw(img).rounded_rectangle(
            [2, 2, M - 3, M - 3], radius=radius - 2, outline=BORDER + (255,), width=5
        )
        draw_mark(img, font_path=find_font("light"), cap_frac=0.36, with_rule=True,
                  mark_color=MARK + (255,), rule_color=RULE + (255,))
    else:
        # Small sizes: heavier weight, no rule — thin strokes vanish under 64px
        draw_mark(img, font_path=find_font("regular"), cap_frac=0.44, with_rule=False,
                  mark_color=MARK + (255,), rule_color=RULE + (255,))
    return img


def glyph_only():
    """Transparent variant for dark UI panels — mark with no tile."""
    img = Image.new("RGBA", (M, M), (0, 0, 0, 0))
    draw_mark(img, font_path=find_font("light"), cap_frac=0.36, with_rule=True,
              mark_color=MARK + (255,), rule_color=RULE + (255,))
    return img


def down(img, size):
    return img.resize((size, size), Image.LANCZOS)


if __name__ == "__main__":
    detailed, simple, glyph = tile(True), tile(False), glyph_only()

    down(detailed, 1024).save("icon.png")
    down(glyph, 256).save("icon_dark.png")

    # Multi-resolution ICO. Below 48px the light weight and the rule disappear,
    # so those frames come from the simplified master.
    frames = [down(simple if s < 64 else detailed, s) for s in (16, 32, 48, 64, 128, 256)]
    frames[-1].save("icon.ico", format="ICO",
                    sizes=[(f.width, f.height) for f in frames],
                    append_images=frames[:-1])

    print("wrote icon.png icon_dark.png icon.ico")
