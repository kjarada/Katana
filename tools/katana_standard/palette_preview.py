"""Draw the Katana Standard palette to a PNG: every colour on the plan view's ground and on white paper,
with its role and both contrast ratios, grouped by family.

    python tools/katana_standard/palette_preview.py [--out docs/images/katana-standard-palette.png] [--scale 2]

The picture is the proof of the palette's two promises (colours.py): each colour holds at least 3:1 against
the dark ground and against white, and no two colours are close enough to be taken for each other. Each
row shows the colour as a chip, as a line and as a ring, the three forms in which a plan uses it.
Needs Pillow; nothing else.
"""
import os
import sys

from PIL import Image, ImageDraw, ImageFont

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import colours  # noqa: E402

PANELS = (
    # (ground, heading ink, body ink, quiet ink, rule, which contrast to print, its label)
    {"ground": colours.GROUND, "head": "#E8ECF2", "body": "#C5CCD6", "quiet": "#7C8696", "rule": "#2E353E",
     "other": colours.PAPER, "title": "On the plan view", "note": "ground #1E2329"},
    {"ground": colours.PAPER, "head": "#1D232B", "body": "#3B4450", "quiet": "#7B8491", "rule": "#E3E6EA",
     "other": colours.GROUND, "title": "On white paper", "note": "paper #FFFFFF"},
)


def _font(size, bold=False):
    names = (("arialbd.ttf", "DejaVuSans-Bold.ttf", "LiberationSans-Bold.ttf") if bold
             else ("arial.ttf", "DejaVuSans.ttf", "LiberationSans-Regular.ttf"))
    for name in names:
        try:
            return ImageFont.truetype(name, size)
        except OSError:
            continue
    return ImageFont.load_default()


def palette_sheet(path, scale=2):
    """Write the sheet to `path`. `scale` multiplies every size (2 is crisp on a screen)."""
    s = scale
    panel_w, row_h, head_h, family_h, margin = 640 * s, 30 * s, 92 * s, 34 * s, 28 * s
    rows = sum(len(names) for _, names in colours.FAMILIES)
    body_h = rows * row_h + len(colours.FAMILIES) * family_h
    foot_h = 84 * s
    width = 2 * panel_w
    height = head_h + body_h + foot_h
    image = Image.new("RGB", (width, height), colours.GROUND)
    draw = ImageDraw.Draw(image)

    f_title, f_head, f_name = _font(22 * s, True), _font(12 * s, True), _font(13 * s, True)
    f_role, f_num, f_note = _font(11 * s), _font(12 * s), _font(11 * s)

    for n, panel in enumerate(PANELS):
        x0 = n * panel_w
        draw.rectangle([x0, 0, x0 + panel_w, height], fill=panel["ground"])
        draw.text((x0 + margin, 22 * s), panel["title"], font=f_title, fill=panel["head"])
        draw.text((x0 + margin, 54 * s),
                  f"{len(colours.COLOURS)} colours against {panel['note']}: each holds at least 3 : 1",
                  font=f_role, fill=panel["quiet"])
        y = head_h
        for heading, names in colours.FAMILIES:
            draw.text((x0 + margin, y + 12 * s), heading.upper(), font=f_head, fill=panel["quiet"])
            draw.line([x0 + margin, y + family_h - 3 * s, x0 + panel_w - margin, y + family_h - 3 * s],
                      fill=panel["rule"], width=max(s, 1))
            y += family_h
            for name in names:
                value = colours.COLOURS[name]
                mid = y + row_h // 2
                chip = [x0 + margin, mid - 9 * s, x0 + margin + 44 * s, mid + 9 * s]
                draw.rounded_rectangle(chip, radius=4 * s, fill=value)
                draw.line([x0 + margin + 56 * s, mid, x0 + margin + 100 * s, mid], fill=value, width=3 * s)
                r = 7 * s
                cx = x0 + margin + 120 * s
                draw.ellipse([cx - r, mid - r, cx + r, mid + r], outline=value, width=max(2 * s, 2))
                draw.text((x0 + margin + 142 * s, mid), name, font=f_name, fill=panel["head"], anchor="lm")
                draw.text((x0 + margin + 276 * s, mid), colours.ROLES[name], font=f_role, fill=panel["body"], anchor="lm")
                ratio = colours.contrast(value, panel["ground"])
                draw.text((x0 + panel_w - margin, mid), f"{ratio:.2f} : 1", font=f_num, fill=panel["body"], anchor="rm")
                y += row_h

    # The footer sits across both panels, on the plan ground.
    foot_y = head_h + body_h
    draw.rectangle([0, foot_y, width, height], fill=colours.GROUND)
    closest = colours.closest_pairs(3)
    pairs = "; ".join(f"{a[7:]} and {b[7:]} {d:.3f}" for d, a, b in closest)
    ink, quiet = "#C5CCD6", "#7C8696"
    draw.text((margin, foot_y + 16 * s),
              "Closest pairs in OKLab (the floor is 0.05): " + pairs + ".", font=f_note, fill=ink)
    draw.text((margin, foot_y + 38 * s),
              "The buried-service colours follow the widely published marking convention: water blue, sewer green, gas yellow, "
              "electricity red, communications orange, recycled water purple.", font=f_note, fill=quiet)
    draw.text((margin, foot_y + 58 * s),
              "Katana's UTILITY DRAW has colours of its own (AS 5488 sets none), so a drawing that mixes the two needs a legend.",
              font=f_note, fill=quiet)
    image.save(path)
    return image.size


if __name__ == "__main__":
    argv = sys.argv[1:]
    out = argv[argv.index("--out") + 1] if "--out" in argv else os.path.join(HERE, "..", "..", "docs", "images",
                                                                            "katana-standard-palette.png")
    scale = int(argv[argv.index("--scale") + 1]) if "--scale" in argv else 2
    os.makedirs(os.path.dirname(os.path.abspath(out)), exist_ok=True)
    print(os.path.normpath(out), palette_sheet(out, scale))
