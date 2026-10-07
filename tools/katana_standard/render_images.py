#!/usr/bin/env python3
"""Draw the catalogue's sheets with Katana itself, headlessly: docs/images/katana-standard-*.png.

    python tools/katana_standard/render_images.py [--bin DIR] [--out DIR] [symbols] [linestyles] [plan]

`DIR` is the folder holding `katana.exe` (default: build/release/bin of this checkout, else KATANA_BIN). Needs
Pillow (to crop, tile and shrink what Katana draws) and a built `katana`; the run is headless
(QT_QPA_PLATFORM=offscreen) and reads no per-user file (KATANA_BUILTIN_CUSTOMISATION=none, so the library is
the only customisation in the session). Without a sheet named, all three are drawn.

What each sheet is, and what is Katana's and what is Pillow's:

* symbols   Katana's own Symbol Library (Format > Symbol Library), one screenshot a group, its icons and
            captions cut out and laid on one sheet by class, each icon tinted with the colour its code takes.
            The icons are the dialog's: a symbol scaled to fill a 64 px tile, which is how a person reads it.
* linestyles  a Katana plot (File > Plot, 300 dpi) of one sample of every linestyle, each drawn by a real
            code of the library in the colour that code gives it, on a sheet of white paper at true size.
* plan      the plan view of the window after `SURVEY IMPORT tests/data/katana_standard/street_corner.fld`:
            the invented street corner, coded and strung by the library, on the plan's own ground.

The palette sheet, katana-standard-palette.png, is not drawn here: a palette is a table of values, so
`palette_preview.py` lays it out from colours.py. The images are not byte-reproducible (fonts differ between
machines), so `make_katana_standard.py --check` leaves them alone. Each is kept under 500 KB.
"""
import json
import os
import subprocess
import sys
import tempfile

sys.dont_write_bytecode = True
HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
JSON = os.path.join(ROOT, "resources", "customisation", "katana-standard.customisation.json")
FLD = os.path.join(ROOT, "tests", "data", "katana_standard", "street_corner.fld")
LIMIT = 500 * 1024
GROUND = (0x1E, 0x23, 0x29)

argv = sys.argv[1:]


def option(name, default):
    return argv[argv.index(name) + 1] if name in argv else default


BIN = option("--bin", os.environ.get("KATANA_BIN", os.path.join(ROOT, "build", "release", "bin")))
OUT = option("--out", os.path.join(ROOT, "docs", "images"))
EXE = os.path.join(BIN, "katana.exe" if os.name == "nt" else "katana")


def katana(args, timeout=300, scale=None):
    """Run katana headless with the library as the only customisation; returns the completed process.
    `scale` is Qt's screen scale factor: 2 grabs a window at twice the pixels, drawn crisply."""
    env = dict(os.environ, QT_QPA_PLATFORM="offscreen", KATANA_BUILTIN_CUSTOMISATION="none")
    if scale:
        env["QT_SCALE_FACTOR"] = str(scale)
    env.pop("KATANA_CUSTOMISATION", None)
    if os.name == "nt":
        env["PATH"] = BIN + os.pathsep + env["PATH"]
    result = subprocess.run([EXE, "--customise", JSON] + args, env=env, capture_output=True, text=True,
                            timeout=timeout)
    if result.returncode != 0:
        sys.exit("katana failed (%d): %s" % (result.returncode, (result.stderr or result.stdout)[-1500:]))
    return result


def modules():
    sys.path.insert(0, HERE)
    import codes, colours, linestyles, symbols
    return codes, colours, linestyles, symbols


def hex_rgb(text):
    return tuple(int(text[i:i + 2], 16) for i in (1, 3, 5))


def save_small(image, name):
    """Write a PNG under 500 KB: palette first, then fewer colours, then a smaller image."""
    from PIL import Image
    path = os.path.join(OUT, name)
    os.makedirs(OUT, exist_ok=True)
    image = image.convert("RGB")
    # MAXCOVERAGE keeps a thin line's own colour: the median cut splits the palette by how many pixels there are, and
    # on a sheet that is mostly white and grey it merges a blue line with a green one at 128 colours.
    for colours in (256, 192, 128, 96, 64, 48, 32):
        image.quantize(colors=colours, method=Image.Quantize.MAXCOVERAGE, dither=Image.Dither.NONE).save(
            path, optimize=True)
        if os.path.getsize(path) <= LIMIT:
            break
    else:
        sys.exit("%s stays over %d bytes at 32 colours" % (name, LIMIT))
    print("wrote %s %dx%d %d bytes" % (path, image.width, image.height, os.path.getsize(path)))


# ---- symbols: the Symbol Library, one screenshot a group ----------------------------------------------------

GRID_X0, GRID_Y0 = 227, 81           # the first icon tile of the dialog's grid, in the screenshot
PITCH_X, PITCH_Y = 108, 104          # a tile and its caption
TILE = 64                            # an icon is 64 px square, on the plan's own ground
COLUMNS_IN_DIALOG = 5
ROWS_IN_DIALOG = 4                   # the fifth row is cut by the window


def font(size, bold=False):
    from PIL import ImageFont
    for name in (("segoeuib.ttf", "arialbd.ttf", "DejaVuSans-Bold.ttf") if bold
                 else ("segoeui.ttf", "arial.ttf", "DejaVuSans.ttf")):
        try:
            return ImageFont.truetype(name, size)
        except OSError:
            pass
    return ImageFont.load_default()


def wrapped(draw, text, face, width):
    """`text` broken on blanks into lines no wider than `width` pixels."""
    lines, line = [], ""
    for word in text.split():
        trial = (line + " " + word).strip()
        if draw.textlength(trial, font=face) <= width or not line:
            line = trial
        else:
            lines.append(line)
            line = word
    return lines + [line]


def draw_symbols():
    from PIL import Image, ImageDraw
    codes_module, colours_module, _, symbols_module = modules()
    colour_of = {}
    for c in codes_module.CODES:
        if c.get("symbol") and c["symbol"] not in colour_of:
            colour_of[c["symbol"]] = hex_rgb(colours_module.COLOURS[c["colour"]])
    by_group = {}
    for d in symbols_module.SYMBOLS:
        by_group.setdefault(d["group"], []).append(d["name"])

    # The dialog's tree rows are "<name> (<count>)". A top group of more than twenty symbols is shot a
    # subgroup at a time, since the grid shows four rows of five.
    rows = []
    for top in codes_module.TAXONOMY:
        subs = {sub[1]: by_group.get(top[1] + "/" + sub[1], []) for sub in top[4]}
        if top[0] == "U":
            subs["All Services"] = by_group.get("Utilities/All Services", [])
        total = sum(len(v) for v in subs.values())
        if total == 0:
            continue
        if total <= COLUMNS_IN_DIALOG * ROWS_IN_DIALOG:
            rows.append("%s (%d)" % (top[1], total))
        else:
            rows += ["%s (%d)" % (sub, len(names)) for sub, names in subs.items() if names]
    work = tempfile.mkdtemp(prefix="ksimg_")
    tiles = {}
    for index, row in enumerate(rows):
        png = os.path.join(work, "g%02d.png" % index)
        result = katana(["--dialog", "formatSymbols", "--fill", "symbolGroups=" + row, "--report", "symbolGrid",
                         "--screenshot", png])
        listed = [line for line in result.stderr.splitlines() if line.startswith("symbolGrid:")]
        order = [n.strip() for n in listed[0].split(":", 1)[1].split(";")]
        shot = Image.open(png).convert("RGB")
        for n, name in enumerate(order):   # the dialog's own order, as it reports it
            x = GRID_X0 + (n % COLUMNS_IN_DIALOG) * PITCH_X
            y = GRID_Y0 + (n // COLUMNS_IN_DIALOG) * PITCH_Y
            tiles[name] = shot.crop((x, y, x + TILE, y + TILE))
    missing = [d["name"] for d in symbols_module.SYMBOLS if d["name"] not in tiles]
    if missing:
        sys.exit("no tile for " + ", ".join(missing))

    # The dialog draws a white line on the ground; take the brightness as the strength of the symbol's own
    # colour. The ground is the plan view's, so the icon sits on the sheet as it sits on the plan.
    def tinted(tile, colour):
        out = tile.copy()
        px, src = out.load(), tile.load()
        for yy in range(TILE):
            for xx in range(TILE):
                r, g, b = src[xx, yy]
                a = min(max((min(r, g, b) - 30) / 180.0, 0.0), 1.0)   # a little stronger than the dialog's own
                px[xx, yy] = tuple(round(GROUND[i] * (1 - a) + colour[i] * a) for i in range(3))
        return out

    per_row, cell_w, cell_h, margin = 8, 118, 112, 36
    classes = []
    for top in codes_module.TAXONOMY:
        names = [d["name"] for d in symbols_module.SYMBOLS if d["group"].split("/")[0] == top[1]]
        if names:
            classes.append((top, names))
    heading = 58
    height = margin * 2 + 92 + sum(((len(n) + per_row - 1) // per_row) * cell_h + heading for _, n in classes)
    width = margin * 2 + per_row * cell_w
    sheet = Image.new("RGB", (width, height), GROUND)
    draw = ImageDraw.Draw(sheet)
    draw.text((margin, margin - 10), "Katana Standard: %d symbols" % len(symbols_module.SYMBOLS),
              fill=(236, 240, 244), font=font(30, True))
    draw.text((margin, margin + 30), "As the Symbol Library draws them, in the colour of the code that uses each.",
              fill=(154, 164, 176), font=font(14))
    y = margin + 76
    small = font(12)
    for top, names in classes:
        label = top[1].upper()
        draw.text((margin, y), label, fill=(196, 206, 218), font=font(14, True))
        draw.text((margin + 12 + draw.textlength(label, font=font(14, True)), y + 1), str(len(names)),
                  fill=(112, 122, 134), font=font(14))
        draw.line((margin, y + 26, width - margin, y + 26), fill=(58, 66, 76), width=1)
        y += heading - 18
        for n, name in enumerate(names):
            x = margin + (n % per_row) * cell_w + (cell_w - TILE) // 2
            top_y = y + (n // per_row) * cell_h
            sheet.paste(tinted(tiles[name], colour_of.get(name, hex_rgb(colours_module.COLOURS["katana note"]))),
                        (x, top_y))
            draw.rectangle((x - 1, top_y - 1, x + TILE, top_y + TILE), outline=(46, 53, 62))
            for line_no, line in enumerate(wrapped(draw, name, small, cell_w - 12)):
                draw.text((x + TILE / 2 - draw.textlength(line, font=small) / 2, top_y + TILE + 5 + line_no * 14),
                          line, fill=(166, 176, 188), font=small)
        y += ((len(names) + per_row - 1) // per_row) * cell_h + 18
    save_small(sheet.crop((0, 0, width, y + margin - 18)), "katana-standard-symbols.png")


# ---- linestyles: a Katana plot of a sample of each ----------------------------------------------------------

SCALE = 100          # the plot scale of the sheet: 1 model unit is 1000 / SCALE millimetres... see mm()


def mm(paper):
    """Model units for `paper` millimetres at the plot scale 1:SCALE (the model is in metres)."""
    return paper * SCALE / 1000.0


def draw_linestyles():
    from PIL import Image
    codes_module, colours_module, linestyles_module, _ = modules()
    user = {}
    for c in codes_module.CODES:
        if c.get("linestyle") and c["linestyle"] != "continuous" and c["linestyle"] not in user:
            user[c["linestyle"]] = c["key"]
    groups = []
    for top in codes_module.TAXONOMY:
        for sub in top[4]:
            g = top[1] + "/" + sub[1]
            names = [d["name"] for d in linestyles_module.LINESTYLES if d["group"] == g]
            if top[0] == "U" and sub[0] == "X":
                names += [d["name"] for d in linestyles_module.LINESTYLES if d["group"] == "Utilities/All Services"]
            if names:
                groups.append((g, names))
    # Three columns of whole groups, in order, cut where the longest column is shortest; a heading for every
    # group, so it is never left at the foot of a column away from its lines.
    sizes = [1 + len(names) for _, names in groups]
    best = None
    for first in range(1, len(groups) - 1):
        for second in range(first + 1, len(groups)):
            longest = max(sum(sizes[:first]), sum(sizes[first:second]), sum(sizes[second:]))
            if best is None or longest < best[0]:
                best = (longest, first, second)
    _, first, second = best
    columns = []
    for block in (groups[:first], groups[first:second], groups[second:]):
        column = []
        for g, names in block:
            column.append(("heading", g))
            column += [("style", n) for n in sorted(names)]
        columns.append(column)
    # A3 holds the longest column with room to spare: the stamps Katana puts in the sheet's corners (the plan title
    # bottom left, the scale bar bottom right) sit below the last row, so none of it is cut or hidden.
    col_w, row_h, name_w, line_w = 122.0, 5.7, 46.0, 66.0
    lines = []
    ident = 0

    def made(command):
        nonlocal ident
        lines.append(command)
        ident += 1
        return ident
    made('TEXT %.4f,%.4f %.4f "Katana Standard: %d linestyles"' % (0, mm(9), mm(4.6), len(linestyles_module.LINESTYLES)))
    made('TEXT %.4f,%.4f %.4f "One sample of each, drawn by a code that uses it, in the colour that code gives it. '
         'Plotted at 1:%d: every mark is the size it is on paper."' % (0, mm(2.5), mm(2.1), SCALE))
    for c, column in enumerate(columns):
        x0 = c * col_w
        for r, (kind, text) in enumerate(column):
            y = -(r + 1) * row_h
            if kind == "heading":
                made('TEXT %.4f,%.4f %.4f "%s"' % (mm(x0), mm(y - 1.0), mm(2.6), text.upper().replace("/", "  /  ")))
                continue
            made('TEXT %.4f,%.4f %.4f "%s"' % (mm(x0), mm(y - 0.7), mm(2.1), text))
            xs = [x0 + name_w, x0 + name_w + 36, x0 + name_w + 44, x0 + name_w + line_w]
            ys = [y, y, y + 1.2, y + 1.2]
            pid = made("PLINE " + " ".join("%.4f,%.4f" % (mm(a), mm(b)) for a, b in zip(xs, ys)))
            lines += ["SELECT %d" % pid, "PROP SET code %s text" % user[text], "SELECT NONE"]
    lines.append("CODE")
    work = tempfile.mkdtemp(prefix="ksimg_")
    script = os.path.join(work, "linestyles.kcs")
    with open(script, "w", encoding="utf-8", newline="\n") as handle:
        handle.write("\n".join(lines) + "\n")
    folder = os.path.join(work, "plot")
    os.makedirs(folder)
    katana(["--script", script, "--plot-sheets", folder, "--format", "png", "--dpi", "300", "--plot-style", "colour"],
           timeout=900)
    sheet = next(f for f in os.listdir(folder) if f.endswith(".png"))
    image = Image.open(os.path.join(folder, sheet)).convert("RGB")
    save_small(crop_plot(image), "katana-standard-linestyles.png")


def crop_plot(image, shrink=0.5):
    """The drawing inside the plot frame: cut the frame, the title block and the three stamps Katana puts in the
    corners of a sheet (north arrow, scale label, scale bar), then trim to the ink and shrink."""
    from PIL import Image
    w, h = image.size
    inner = image.crop((int(0.057 * w), int(0.037 * h), int(0.975 * w), int(0.88 * h)))
    iw, ih = inner.size
    edge = 40   # the frame's own line runs just inside the crop: whiten its margin
    for box in ((0, 0, iw, edge), (0, ih - edge, iw, ih), (0, 0, edge, ih), (iw - edge, 0, iw, ih)):
        inner.paste((255, 255, 255), box)
    white = Image.new("RGB", inner.size, (255, 255, 255))
    for box in ((int(0.955 * iw), 0, iw, int(0.09 * ih)), (0, int(0.95 * ih), int(0.07 * iw), ih),
                (int(0.85 * iw), int(0.955 * ih), iw, ih)):
        inner.paste(white.crop((0, 0, box[2] - box[0], box[3] - box[1])), (box[0], box[1]))
    ink = inner.convert("L").point(lambda v: 255 if v < 245 else 0).getbbox()
    pad = 60
    inner = inner.crop((max(ink[0] - pad, 0), max(ink[1] - pad, 0), min(ink[2] + pad, iw), min(ink[3] + pad, ih)))
    return inner.resize((round(inner.width * shrink), round(inner.height * shrink)), Image.Resampling.LANCZOS)


# ---- plan: the window after the import -----------------------------------------------------------------------

# What the plan sheet labels: the code, the colour's role, and where its words go, as pixels from the anchor (a point of
# the field file, in metres): "right" and "start" put the words' first letter at the anchor plus the offset, "above"
# and "below" centre them on it.
PLAN_LABELS = (
    ("CBT", (330000.0, 6250030.0), "start", (-4, -38)),
    ("MCC", (330020.0, 6250030.0), "right", (30, 0)),
    ("UWQ", (330005.0, 6250020.0), "right", (24, 0)),
    ("VTB", (330008.0, 6250012.0), "below", (0, 42)),
    ("FFP", (330030.0, 6250010.0), "right", (24, 0)),
    ("KPG", (330006.0, 6249991.0), "above", (0, -34)),
    ("KKT", (330024.0, 6249990.0), "right", (40, 0)),
    ("UWV", (330012.0, 6249980.0), "below", (0, 54)),
    ("UWM", (330024.0, 6249980.0), "right", (40, 0)),
    ("UED", (330020.0, 6249970.0), "right", (40, 0)),
)


def draw_plan():
    """The plan view alone (`--panel View1`), at twice the pixels, grid off, the street corner filling it, and each
    feature named beside it: the code in its own colour and the code's legend label. Katana draws the plan; the
    words are laid on afterwards from the library's own data, at the places the field file's points project to."""
    import re
    from PIL import Image, ImageDraw
    codes_module, colours_module, _, _ = modules()
    by_key = {c["key"]: c for c in codes_module.CODES}
    work = tempfile.mkdtemp(prefix="ksimg_")
    png = os.path.join(work, "plan.png")
    result = katana(["--command", 'SURVEY IMPORT "%s"' % FLD.replace("\\", "/"), "--trigger", "viewGrid",
                     "--command", "ZOOM AREA 329990,6249962,330040,6250038", "--command", "VIEWS LIST",
                     "--panel", "View1", "--screenshot", png], scale=2)
    area = re.findall(r"area=([-0-9.e]+),([-0-9.e]+),([-0-9.e]+),([-0-9.e]+)", result.stdout + result.stderr)
    if not area:
        sys.exit("the view did not say what area it shows")
    x0, _, x1, ytop = [float(v) for v in area[-1]]
    shot = Image.open(png).convert("RGB")
    shot = shot.crop((0, TITLE_BAR, shot.width, shot.height))     # the view's own title bar
    per_metre = shot.width / (x1 - x0)
    ink = shot.convert("L").point(lambda v: 255 if abs(v - 36) > 14 else 0).getbbox()
    draw = ImageDraw.Draw(shot)
    key_face, word_face = font(24, bold=True), font(24)
    near, far = shot.width, 0
    for key, (x, y), where, (dx, dy) in PLAN_LABELS:
        px, py = (x - x0) * per_metre + dx, (ytop - y) * per_metre + dy
        hexcolour = hex_rgb(colours_module.COLOURS[by_key[key]["colour"]]) if key != "UWQ" else (154, 163, 174)
        words = by_key[key]["comment"] if key != "UWQ" else "no rule"
        key_w = draw.textlength(key + "  ", font=key_face)
        total = key_w + draw.textlength(words, font=word_face)
        left = px if where in ("right", "start") else px - total / 2
        draw.text((left, py), key, font=key_face, fill=hexcolour, anchor="lm")
        draw.text((left + key_w, py), words, font=word_face, fill=(197, 204, 214), anchor="lm")
        near, far = min(near, left), max(far, left + total)
    pad = 70
    left, right = max(int(min(ink[0], near)) - pad, 0), min(max(ink[2], int(far)) + pad, shot.width)
    top, bottom = max(ink[1] - pad, 0), min(ink[3] + pad, shot.height)
    save_small(shot.crop((left, top, right, bottom)), "katana-standard-plan.png")


TITLE_BAR = 52       # the pixels of a panel grab above the plan view itself, at twice the pixels


def plan_tiles(script, frame, scale):
    """The plan view of `script` over `frame` (x0, y0, x1, y1 in metres) as one picture, at `scale` logical
    pixels a metre and twice the pixels. A paper-sized mark has a fixed size in pixels whatever the zoom, so
    the way to give a crowded scene room is a bigger picture, not a different drawing: the window is one size,
    so the picture is taken a window at a time, each placed by `ZOOM CENTRE x,y SCALE s` so that the tiles
    abut to the pixel, and laid side by side. Returns (image, pixels per metre)."""
    import re
    from PIL import Image
    work = tempfile.mkdtemp(prefix="ksimg_")

    def grab(cx, cy, name):
        png = os.path.join(work, name)
        result = katana(["--script", script, "--trigger", "viewGrid", "--command",
                         "ZOOM CENTRE %.6f,%.6f SCALE %s" % (cx, cy, scale), "--command", "VIEWS LIST",
                         "--panel", "View1", "--screenshot", png], timeout=900, scale=2)
        area = re.findall(r"area=([-0-9.e]+),([-0-9.e]+),([-0-9.e]+),([-0-9.e]+)", result.stdout + result.stderr)
        if not area:
            sys.exit("the view did not say what area it shows")
        shot = Image.open(png).convert("RGB")
        return shot.crop((0, TITLE_BAR, shot.width, shot.height)), [float(v) for v in area[-1]]

    first, area = grab((frame[0] + frame[2]) / 2, (frame[1] + frame[3]) / 2, "probe.png")
    width, height = area[2] - area[0], area[3] - area[1]
    cols = max(1, -(-int(round((frame[2] - frame[0]) * 1000)) // int(round(width * 1000))))
    rows = max(1, -(-int(round((frame[3] - frame[1]) * 1000)) // int(round(height * 1000))))
    # The grid is centred on the frame; row 0 is the top (the largest y).
    left = (frame[0] + frame[2]) / 2 - cols * width / 2
    top = (frame[1] + frame[3]) / 2 + rows * height / 2
    sheet = Image.new("RGB", (cols * first.width, rows * first.height), GROUND)
    for row in range(rows):
        for col in range(cols):
            tile, _ = grab(left + (col + 0.5) * width, top - (row + 0.5) * height, "t%d%d.png" % (row, col))
            sheet.paste(tile, (col * first.width, row * first.height))
    return sheet, first.width / width


def draw_showcase():
    """The invented street corner of showcase.py, drawn by the library from its codes, in the plan view at the
    plot scale 1:500, laid out as four screenshots side by side."""
    sys.path.insert(0, HERE)
    import showcase
    lines, _ = showcase.commands()
    work = tempfile.mkdtemp(prefix="ksimg_")
    script = os.path.join(work, "showcase.kcs")
    with open(script, "w", encoding="utf-8", newline="\n") as handle:
        handle.write("\n".join(lines) + "\n")
    # 1:500 is 2 mm of paper to the metre, and a logical pixel is 1/96 inch.
    sheet, _ = plan_tiles(script, showcase.FRAME, showcase.PLOT_SCALE_PIXELS)
    ink = sheet.convert("L").point(lambda v: 255 if abs(v - 36) > 14 else 0).getbbox()
    pad = 80
    sheet = sheet.crop((max(ink[0] - pad, 0), max(ink[1] - pad, 0), min(ink[2] + pad, sheet.width),
                        min(ink[3] + pad, sheet.height)))
    save_small(sheet, "katana-standard-showcase.png")


if __name__ == "__main__":
    sheets = ("symbols", "linestyles", "plan", "showcase")
    wanted = [a for a in argv if a in sheets] or list(sheets)
    if "symbols" in wanted:
        draw_symbols()
    if "linestyles" in wanted:
        draw_linestyles()
    if "plan" in wanted:
        draw_plan()
    if "showcase" in wanted:
        draw_showcase()
