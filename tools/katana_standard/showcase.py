#!/usr/bin/env python3
"""An invented street corner that touches every class of Katana Standard, as a script Katana runs.

    python tools/katana_standard/showcase.py > showcase.kcs        the script, one command a line
    python tools/katana_standard/showcase.py --keys                the keys it uses, one a line

The scene is a hundred and fifty metres by a hundred or so of a made-up suburb: a main road with a side street
joining it on the north side, kerbs with rounded returns, footpaths, painted lines, a crossing, houses and a
shed in three lots, fences and walls, trees and shrubs, the buried and overhead services under and above the
road, drainage, a short stretch of rail on a bridge, contours and a creek with its banks, control marks, a few
investigations and hazards, and the labels and the limit of the survey. It is drawn with plain `POINT`, `PLINE`
and `TEXT` commands; each object is given its field code by `PROP SET code`, and the one `CODE` at the end lets
the library style them all, so what the picture shows is what the library says and nothing else. No object of it
comes from a survey.

The scene is the data of `render_images.py showcase` (docs/images/katana-standard-showcase.png). Every code it
uses is checked against the keys of `codes.py`, so a renamed key fails here and not in the picture.
"""
import math
import os
import sys

sys.dont_write_bytecode = True
HERE = os.path.dirname(os.path.abspath(__file__))

# Metres on the ground for each unit of the design below. A mark is the same size on paper at any scale, so a
# symbol that is 4.4 mm across is about 3 m across in this picture; the factor leaves each service room to read.
K = 1.5

# The picture is taken at the plot scale 1:500: 2 mm of paper to the metre, so a design unit of K metres is 3 mm and a
# 4.4 mm symbol is 2.2 m across. Katana's plan view draws a millimetre of paper as 96 / 25.4 logical pixels, and
# `ZOOM CENTRE x,y SCALE s` takes pixels to the metre.
PLOT_SCALE = 500
MM = PLOT_SCALE / 1000.0                       # metres of ground for one millimetre of paper
PLOT_SCALE_PIXELS = 96 / 25.4 / MM             # logical pixels to the metre
# What the picture shows, in metres: x0, y0, x1, y1 (the survey limit's rectangle, the north arrow and the title).
FRAME = (-6.0, -8.0, 162.0, 123.0)


def known_keys():
    sys.path.insert(0, HERE)
    import codes
    return {c["key"] for c in codes.CODES}


def matched_key(code, keys):
    """The most specific key that answers a field code: the exact key, else the longest prefix key (`WM*` answers
    `WM1`), else None. The bare `*` answers every code and so says nothing about one."""
    if code in keys:
        return code
    for n in range(len(code), 0, -1):
        if code[:n] + "*" in keys:
            return code[:n] + "*"
    return None


class Scene:
    def __init__(self):
        self.commands = []
        self.next_id = 1
        self.uses = []

    def _made(self, command, code):
        self.commands.append(command)
        entity = self.next_id
        self.next_id += 1
        self.uses.append(code)
        self.commands += ["SELECT %d" % entity, "PROP SET code %s text" % code, "SELECT NONE"]

    def point(self, code, x, y):
        self._made("POINT %.3f,%.3f" % (x * K, y * K), code)

    def line(self, code, points, close=False):
        self._made("PLINE " + " ".join("%.3f,%.3f" % (x * K, y * K) for x, y in points) +
                   (" CLOSE" if close else ""), code)

    def text(self, code, x, y, mm, words, rotation=0):
        self._made('TEXT %.3f,%.3f "%s" paper=%.2f rotation=%.1f' % (x * K, y * K, words, mm, rotation), code)


def dash_centre(t, period, dash):
    """`t` (design units along a lettered pipe, from its start) moved to the middle of the nearest dash.

    A service's pipe is broken by its letter every `period` mm of paper, the dash being `dash` mm of that, and a
    symbol that lands on the letter prints a second one over it ("WW"). A symbol is put in the middle of a dash
    instead, as a surveyor would put a valve where the pipe can be seen."""
    millimetres = t * K / MM
    k = max(0, round((millimetres - dash / 2.0) / period))
    return (k * period + dash / 2.0) * MM / K


def densify(points, step):
    """The polyline through `points` with a vertex at least every `step` units (a fence has a post at each)."""
    out = [points[0]]
    for (x0, y0), (x1, y1) in zip(points, points[1:]):
        parts = max(1, math.ceil(math.hypot(x1 - x0, y1 - y0) / step))
        out += [(x0 + (x1 - x0) * i / parts, y0 + (y1 - y0) * i / parts) for i in range(1, parts + 1)]
    return out


def arc(cx, cy, radius, a0, a1, parts=12):
    return [(cx + radius * math.cos(math.radians(a0 + (a1 - a0) * i / parts)),
             cy + radius * math.sin(math.radians(a0 + (a1 - a0) * i / parts))) for i in range(parts + 1)]


def ring(cx, cy, radius, parts=24):
    return arc(cx, cy, radius, 0, 360, parts)[:-1]


def box(x0, y0, x1, y1):
    return [(x0, y0), (x1, y0), (x1, y1), (x0, y1)]


def wave(x0, x1, base, amplitude, length, phase, step=3.0):
    count = max(2, int((x1 - x0) / step))
    return [(x0 + (x1 - x0) * i / count,
             base + amplitude * math.sin(2 * math.pi * (x0 + (x1 - x0) * i / count) / length + phase))
            for i in range(count + 1)]


def build():
    s = Scene()
    # ---- the road: kerbs, returns, paint ----------------------------------------------------------------------
    s.line("KB", [(0, 28), (100, 28)])
    s.line("KB", [(0, 44), (56, 44)])
    s.line("KB", arc(56, 50, 6, -90, 0))
    s.line("KB", [(62, 50), (62, 72)])
    s.line("KB", [(76, 72), (76, 50)])
    s.line("KB", arc(82, 50, 6, 180, 270))
    s.line("KB", [(82, 44), (100, 44)])
    s.line("LL", [(0, 36), (56, 36)])
    s.line("LL", [(82, 36), (100, 36)])
    s.line("DB", [(69, 72), (69, 52)])
    s.line("CG", [(63, 45.5), (69, 45.5)])
    s.line("CZ", [(30, 29), (30, 43)])
    s.line("AS", [(4, 31.5), (14, 31.5)])
    s.line("AL", [(86, 40.5), (96, 40.5)])
    s.line("KR", box(27, 24.8, 33, 27.2), close=True)
    # ---- footpaths and the verge ---------------------------------------------------------------------------------
    s.line("FP", [(0, 23), (100, 23)])
    s.line("FP", [(0, 47.5), (55, 47.5)])
    s.line("FP", [(83, 47.5), (100, 47.5)])
    s.line("DW", [(14, 47.5), (14, 52)])
    s.line("BK", [(84, 46.8), (99, 46.8)])
    s.line("DD", [(2, 42.8), (18, 42.8)])
    s.line("EM", [(86, 45.2), (99, 45.2)])
    # ---- the lots, their marks, the buildings -----------------------------------------------------------------------
    for lot in (box(4, 52, 30, 69), box(30, 52, 54, 69), box(84, 52, 98, 69)):
        s.line("BT", lot, close=True)
    for x, y in ((4, 52), (30, 52), (54, 52), (4, 69), (30, 69), (54, 69)):
        s.point("PBPG", x, y)
    s.point("PBDY", 84, 52)
    s.point("PGPI", 98, 52)
    s.point("PSKI", 84, 69)
    s.point("PDHL", 98, 69)
    s.line("BE", [(34, 55), (50, 55)])
    s.line("BE", [(88, 66), (98, 66)])
    s.line("BC", [(0, 49.5), (55, 49.5)])
    s.line("BU", box(8, 57, 24, 66), close=True)
    s.line("BV", box(7, 56, 25, 67), close=True)
    s.line("AW", box(8, 54.2, 24, 56.8), close=True)
    s.line("DO", [(14.5, 57), (17.5, 57)])
    s.point("PCBU", 8, 66)
    s.point("PCBU", 24, 66)
    s.line("BU", box(36, 59, 45, 65), close=True)
    s.line("OR", box(46, 57, 52, 63), close=True)
    s.line("SO", [(14, 52.5), (18, 52.5), (18, 54), (14, 54)])
    s.line("VE", box(88, 56, 95, 62), close=True)
    s.point("PFLG", 51, 66)
    s.point("PHIS", 33, 50.5)
    # ---- fences, walls and gates ----------------------------------------------------------------------------------------
    s.line("FE", densify([(4, 52), (4, 69)], 3.4))
    s.line("FM", densify([(54, 52), (54, 69)], 5.7))
    s.line("FC", densify([(5, 69), (53, 69)], 6))
    s.line("FE", densify([(98, 52), (98, 69)], 4.3))
    s.line("SF", densify([(85, 69), (97, 69)], 4))
    s.line("FL", densify([(85, 52.6), (97, 52.6)], 4))
    s.line("FM", densify([(30.5, 52.4), (30.5, 68.6)], 3.4))
    s.line("TW", [(4, 51.5), (12, 51.5)])
    s.line("BW", [(20, 51.5), (30, 51.5)])
    s.line("RW", [(38, 51.5), (52, 51.5)])
    s.line("RW", [(84, 51), (96, 51)])
    s.line("TW", [(36, 70.5), (46, 70.5)])
    s.line("BW", [(4, 71), (14, 71)])
    s.line("AG", [(14.5, 51.5), (17.5, 51.5)])
    s.line("AG", [(23, 51.5), (27, 51.5)])
    s.point("POST", 4, 60)
    s.point("POST", 54, 60)
    # ---- street furniture -----------------------------------------------------------------------------------------------
    for x in (14, 40, 66, 92):
        s.point("PLPL", x, 25.2)
    s.point("PLGN", 58, 25.2)
    s.point("PSIN", 52, 25.2)
    s.point("PSDS", 64.5, 51.5)
    s.point("PSIN", 8, 25.2)
    s.point("PSIN", 90, 45)
    s.point("AC", 45, 24.8)
    s.point("AC", 47.5, 24.8)
    s.point("AC", 50, 24.8)
    s.point("POST", 74, 70)
    s.point("POST", 64, 70)
    s.line("FG", densify([(78, 72), (78, 56)], 2.5))
    s.line("SE", box(89.2, 48.4, 90.8, 49.6), close=True)
    s.point("PBIN", 86, 49)
    s.point("PKME", 94, 49)
    s.point("PMBX", 98, 49)
    s.point("PSGL", 57.2, 45.2)
    s.point("PSGL", 80.8, 45.2)
    s.point("PSCL", 58.6, 46.6)
    s.point("PCAM", 61, 46.2)
    # ---- drainage -----------------------------------------------------------------------------------------------------------
    s.line("U3", [(20, 30.4), (60, 30.4)])
    for x in (20, 40):
        s.point("PGUL", x, 28.7)
    s.point("PILT", 60, 28.7)
    s.point("PJBX", 36, 30.4)
    s.point("PHWB", 50, 30.4)
    s.point("PDJM", 8, 30.4)
    s.line("D1", [(66, 27), (90, 27)])
    s.point("PEWW", 66, 27)
    s.point("PHWT", 90, 27)
    s.line("U1", [(58, 52), (58, 60)])
    s.point("PSFP", 58, 60)
    s.line("DT", [(0, 19), (36, 19)])
    s.line("DF", [(40, 18.5), (58, 17)])
    s.line("DT", [(60, 16.5), (74, 14)])
    # ---- services: one run of each, spaced so each reads ---------------------------------------------------------------------
    s.line("WM", [(0, 33), (100, 33)])
    s.line("WY", [(34, 33), (34, 46)])
    s.line("WI", [(5, 34.6), (22, 34.6)])
    for x in (20, 60):
        s.point("PWSV", dash_centre(x, 16, 11), 33)
    s.point("PWMR", 34, 47)
    s.point("PWSR", dash_centre(80, 16, 11), 33)
    s.point("PWAV", dash_centre(98, 16, 11), 33)
    s.point("PWEP", 2, 71)
    s.line("SM", [(0, 39), (100, 39)])
    s.line("SY", [(dash_centre(15, 20, 12), 39), (dash_centre(15, 20, 12), 52)])
    s.line("SR", [(78, 40), (78, 70)])
    s.line("SZ", [(86, 41), (100, 41)])
    for x in (15, 45, 70):
        s.point("PSMH", dash_centre(x, 20, 12), 39)
    s.point("PSVP", dash_centre(15, 20, 12), 51)
    s.point("PSLH", 91, 65)
    s.point("PSPS", 78, 71)
    s.line("YG", [(0, 26.8), (100, 26.8)])
    s.line("DG", [(34, 26.8), (34, 24.2)])
    s.line("ZG", [(2, 25.4), (14, 25.4)])
    s.point("PGAS", dash_centre(56, 16, 11), 26.8)
    s.point("PGRB", dash_centre(22, 16, 11), 26.8)
    s.point("PGMR", 34, 24)
    s.point("PGHL", dash_centre(82, 16, 11), 26.8)
    s.point("PGPM", dash_centre(96, 16, 11), 26.8)
    s.line("EU", [(0, 46.2), (55, 46.2)])
    s.line("UL", [(6, 48.5), (30, 48.5), (54, 48.5)])
    s.line("EZ", [(84, 42.6), (100, 42.6)])
    s.line("ED", [(0, 45), (20, 45)])
    for x in (6, 30, 54):
        s.point("PPPL", x, 48.5)
    s.point("PSAP", 6, 50)
    s.point("PETC", 84, 49.6)
    s.point("PEUP", dash_centre(46, 12, 7), 46.2)
    s.point("PEJB", dash_centre(22, 12, 7), 46.2)
    s.line("TN", [(0, 21), (100, 21)])
    s.line("TD", [(36, 21), (36, 22.6)])
    s.line("TZ", [(70, 21.8), (86, 21.8)])
    s.point("PTSP", dash_centre(18, 12, 7), 21)
    s.point("PTTP", dash_centre(62, 12, 7), 21)
    s.point("PTDP", dash_centre(90, 12, 7), 21)
    s.line("RM", [(66, 50), (66, 72)])
    s.line("RM", [(66, 60), (60, 60)])
    s.line("RM", [(70, 31.2), (96, 31.2)])
    s.point("PRSV", 66, 50 + dash_centre(56 - 50, 16, 11))
    s.point("PRMR", 66, 50 + dash_centre(70 - 50, 16, 11))
    s.line("WM", [(48, 33), (48, 27.4)])
    s.point("PWFB", 48, 27)
    s.point("PWHY", 68, 46.6)
    s.point("PWSV", 48, 33 - dash_centre(2, 16, 11))
    s.line("FU", [(73, 50), (73, 72)])
    s.point("POHM", 73, 50 + dash_centre(5, 16, 11))
    s.point("PFPM", 73, 50 + dash_centre(16, 16, 11))
    s.point("PPET", 96, 62)
    s.line("IT", [(8, 41.6), (56, 41.6)])
    s.point("PSJX", 56, 41.6)
    s.point("PSDR", 24, 41)
    s.line("UP", [(56.5, 52), (56.5, 69)])
    s.point("PGAT", 56.5, 52 + dash_centre(4, 12, 7))
    s.point("PUSR", 56.5, 52 + dash_centre(11, 12, 7))
    s.point("PPOL", 50.5, 34.6)
    # ---- vegetation ---------------------------------------------------------------------------------------------------------------
    for x, code in ((6, "PT04"), (20, "PT07"), (34, "PT04"), (50, "PT10"), (64, "PT04"), (80, "PT02"), (94, "PT04")):
        s.point(code, x, 18)
    s.point("PTRE", 42, 17.6)
    s.point("PT15", 72, 17.8)
    s.point("PT05", 41, 67.4)
    s.point("PT06", 27.4, 64)
    s.line("TR", densify([(88, 71), (97, 71)], 3.2))
    s.line("TR", ring(34, 18, 3.4), close=True)
    s.line("JD", [(31, 52.9), (53, 52.9)])
    s.line("JG", [(34, 56), (50, 56), (50, 54), (34, 54)], close=True)
    s.line("JC", [(6, 67.6), (28, 67.6)])
    s.line("JM", wave(0, 60, 13.2, 0.6, 18, 0.4, step=2.5))
    s.line("JO", box(2, 4, 28, 10), close=True)
    s.line("JS", [(60, 66), (75, 66)])
    for x in (6, 9, 11.5):
        s.point("PT01", x, 54.4)
    s.point("PF03", 91, 54)
    s.point("PT02", 59.5, 64)
    s.point("PT03", 60.2, 62.4)
    # ---- rail and a bridge ---------------------------------------------------------------------------------------------------------
    s.line("RA", [(0, 5.0), (100, 5.0)])
    s.line("RA", [(0, 6.435), (100, 6.435)])
    s.line("RT", [(0, 5.7175), (100, 5.7175)])
    s.line("RF", [(0, 3.4), (100, 3.4)])
    s.line("RF", [(0, 8.1), (100, 8.1)])
    s.line("BR", [(42, 2.6), (60, 2.6)])
    s.line("BR", [(42, 8.9), (60, 8.9)])
    for x in (46, 52, 57):
        s.point("PBPI", x, 5.7)
    s.line("RP", [(64, 9.6), (92, 9.6)])
    for x in (10, 30, 74):
        s.point("PSTR", x, 10.4)
    s.point("PRSG", 96, 10.4)
    s.point("PRBX", 20, 5.7)
    s.point("PRCC", 100, 5.7)
    # ---- the ground: contours, creek, banks, shots -----------------------------------------------------------------------------------
    for k, base in enumerate((11.4, 14.2, 16.2)):
        s.line("Z0" if k == 2 else "X0", wave(0, 100, base, 0.7, 38, k * 0.9))
    s.line("XQ", ring(14, 13, 1.6, 18), close=True)
    s.line("WE", [(44, -3), (46, 0), (50, 3), (58, 5)])
    s.line("WR", [(47, -3), (49, -0.5), (53, 2.2), (61, 4)])
    s.line("TC", [(34, -3), (35.5, 0), (40, 2.2), (48, 4)])
    s.line("BB", [(38, -3), (39.5, 0), (43, 1.4), (50, 3.2)])
    s.line("WC", [(45, -3), (47, 0), (51, 2.6), (59, 4.4)])
    s.line("NB", [(0, 15.4), (22, 15.4)])
    s.line("NS", [(26, 15.6), (38, 16)])
    s.line("CU", [(62, 13), (80, 15), (98, 14)])
    s.line("VL", [(64, 11.4), (82, 12.4), (98, 11.6)])
    s.line("TC", [(2, 12.2), (20, 12)])
    s.line("BB", [(2, 9.6), (20, 9.4)])
    s.line("HP", box(66, 7.8, 80, 10.4), close=True)
    s.line("JR", [(84, 12.2), (88, 13.6), (92, 12.4), (96, 13.6)])
    s.line("VC", [(74, 19.6), (78, 20.4), (82, 19.6), (86, 20.4)])
    s.point("PBRK", 70, 17)
    for x, y, code in ((18, 15.2, "PNSS"), (30, 12.6, "PSHT"), (8, 16.2, "PBRK"), (54, 15, "PNSS"), (66, 15.6, "PSHT"),
                       (12, 32, "PSHT"), (14, 45.6, "PNSS")):
        s.point(code, x, y)
    # ---- control, investigation, plan marks -----------------------------------------------------------------------------------------
    s.point("PSSF", 6, 17.6)
    s.point("PSSG", 25, 21.8)
    s.point("PBMK", 52, 20.4)
    s.point("PPMK", 76.5, 49.2)
    s.point("PCMK", 96, 15)
    s.point("PSSC", 4, 45.8)
    s.point("PDPY", 96.5, 43.4)
    s.point("PBHX", 32, 12)
    s.point("PGEO", 22, 10.6)
    s.point("PHRL", 68, 41.5)
    s.line("FS", [(-3, -4), (103, -4), (103, 74), (-3, 74)], close=True)
    s.line("DE", [(64.2, 43.5), (71.8, 43.5), (71.8, 41.5), (64.2, 41.5)], close=True)
    # ---- hazards, heritage, general -------------------------------------------------------------------------------------------------
    s.point("PHAZ", 8, 41.4)
    s.line("OH", box(88, 11, 96, 16), close=True)
    s.point("PHST", 33, 49)
    s.point("PHEC", 40, 23.2)
    s.point("PVID", 20, 38)
    s.point("PUSR", 2, 62)
    s.line("QQ", [(14, 70.6), (26, 70.6)])
    s.line("SF", [(94, 52.4), (94, 58)])
    s.line("OP", box(88, 28.8, 99, 32.2), close=True)
    # ---- words ------------------------------------------------------------------------------------------------------------------------------------------
    s.text("4", 0, 79, 3.6, "KATANA STANDARD - A STREET CORNER")
    s.text("2", 0, 76, 2.0, "Every mark on this plan is drawn by the library from its survey code")
    s.text("3", 42, 37.0, 2.2, "MAIN STREET")
    s.text("3", 71.0, 60, 2.2, "RIVER ROAD", 90)
    s.text("1", 3, 0.9, 1.8, "Rail corridor")
    s.text("1", 18.6, 14.2, 1.5, "RL 24.3")
    s.text("1", 60, 17.0, 1.5, "25")
    s.text("8", 25.6, 58.0, 1.8, "Lot 7")
    s.text("8", 31.6, 66.4, 1.8, "Lot 8")
    s.text("8", 86, 63.2, 1.8, "Lot 9")
    return s


def commands():
    scene = build()
    keys = known_keys()
    unknown = sorted({u for u in scene.uses if matched_key(u, keys) is None})
    if unknown:
        raise SystemExit("showcase.py uses codes that no key of codes.py answers: " + ", ".join(unknown))
    return scene.commands + ["CODE"], sorted({matched_key(u, keys) for u in scene.uses})


def main(argv):
    lines, used = commands()
    if "--keys" in argv:
        print("\n".join(used))
    else:
        sys.stdout.write("\n".join(lines) + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
