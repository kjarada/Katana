#!/usr/bin/env python3
"""An invented street corner that touches every class of Katana Standard, as a script Katana runs.

    python tools/katana_standard/showcase.py > showcase.kcs        the script, one command a line
    python tools/katana_standard/showcase.py --keys                the codes it uses, one a line

The scene is a hundred and fifty metres by a hundred or so of a made-up suburb: a main road with a side street
joining it on the north side, kerbs with rounded returns, footpaths, painted lines, a crossing, houses and a
shed in three lots, fences and walls, trees and shrubs, the buried and overhead services under and above the
road, drainage, a short stretch of rail on a bridge, contours and a creek with its banks, control marks, a few
investigations and hazards, and the labels and the limit of the survey. It is drawn with plain `POINT`, `PLINE`
and `TEXT` commands; each object is given its survey code by `PROP SET code`, and the one `CODE` at the end lets
the library style them all, so what the picture shows is what the library says and nothing else. No object of it
comes from a survey.

The scene is the data of `render_images.py showcase` (docs/images/katana-standard-showcase.png). Every key it
uses is checked against `codes.py`, so a renamed code fails here and not in the picture.
"""
import math
import os
import sys

sys.dont_write_bytecode = True
HERE = os.path.dirname(os.path.abspath(__file__))

# Metres on the ground for each unit of the design below. A mark is the same size on paper at any scale, so a
# symbol that is 4.4 mm across is about 3 m across in this picture; the factor leaves each service room to read.
K = 1.5


def known_keys():
    sys.path.insert(0, HERE)
    import codes
    return {c["key"] for c in codes.CODES}


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
    s.line("KKT", [(0, 28), (100, 28)])
    s.line("KKT", [(0, 44), (56, 44)])
    s.line("KKR", arc(56, 50, 6, -90, 0))
    s.line("KKT", [(62, 50), (62, 72)])
    s.line("KKT", [(76, 72), (76, 50)])
    s.line("KKR", arc(82, 50, 6, 180, 270))
    s.line("KKT", [(82, 44), (100, 44)])
    s.line("RMD", [(0, 36), (56, 36)])
    s.line("RMD", [(82, 36), (100, 36)])
    s.line("RMB", [(69, 72), (69, 52)])
    s.line("RMS", [(63, 45.5), (69, 45.5)])
    s.line("RMZ", [(30, 29), (30, 43)])
    s.point("RMA", 10, 31.5)
    s.point("RMA", 92, 40.5)
    s.point("RPR", 28, 27)
    s.point("RPR", 32, 27)
    s.point("RPT", 30, 25.5)
    # ---- footpaths and the verge ---------------------------------------------------------------------------------
    s.line("RPF", [(0, 23), (100, 23)])
    s.line("RPF", [(0, 47.5), (55, 47.5)])
    s.line("RPF", [(83, 47.5), (100, 47.5)])
    s.line("RPD", [(14, 47.5), (14, 52)])
    s.line("KKB", [(84, 46.8), (99, 46.8)])
    s.line("KKD", [(2, 42.8), (18, 42.8)])
    s.line("KKM", [(86, 45.2), (99, 45.2)])
    # ---- the lots, their marks, the buildings -----------------------------------------------------------------------
    for lot in (box(4, 52, 30, 69), box(30, 52, 54, 69), box(84, 52, 98, 69)):
        s.line("CBT", lot, close=True)
    for x, y in ((4, 52), (30, 52), (54, 52), (4, 69), (30, 69), (54, 69)):
        s.point("CMP", x, y)
    s.point("CMI", 84, 52)
    s.point("CMT", 98, 52)
    s.point("CMS", 84, 69)
    s.point("CMN", 98, 69)
    s.line("CEG", [(34, 55), (50, 55)])
    s.line("CED", [(88, 66), (98, 66)])
    s.line("CBR", [(0, 49.5), (55, 49.5)])
    s.line("BBW", box(8, 57, 24, 66), close=True)
    s.line("BBE", box(7, 56, 25, 67), close=True)
    s.line("BBV", box(8, 54.2, 24, 56.8), close=True)
    s.point("BBD", 16, 57)
    s.point("BBC", 8, 66)
    s.point("BBC", 24, 66)
    s.line("BBW", box(36, 59, 45, 65), close=True)
    s.line("BBH", box(46, 57, 52, 63), close=True)
    s.line("BAS", [(14, 52.5), (18, 52.5), (18, 54), (14, 54)])
    s.line("BAH", [(13.5, 52.2), (13.5, 54.4)])
    s.line("BBU", box(88, 56, 95, 62), close=True)
    s.point("BSF", 51, 66)
    s.point("BSM", 33, 50.5)
    # ---- fences, walls and gates ----------------------------------------------------------------------------------------
    s.line("FFP", densify([(4, 52), (4, 69)], 3.4))
    s.line("FFC", densify([(54, 52), (54, 69)], 5.7))
    s.line("FFR", densify([(5, 69), (53, 69)], 6))
    s.line("FFW", densify([(98, 52), (98, 69)], 4.3))
    s.line("FFB", densify([(85, 69), (97, 69)], 4))
    s.line("FFE", densify([(85, 52.6), (97, 52.6)], 4))
    s.line("FFM", densify([(30.5, 52.4), (30.5, 68.6)], 3.4))
    s.line("FWB", [(4, 51.5), (12, 51.5)])
    s.line("FWK", [(20, 51.5), (30, 51.5)])
    s.line("FWR", [(38, 51.5), (52, 51.5)])
    s.line("FWT", [(84, 51), (96, 51)])
    s.line("FWS", [(36, 70.5), (46, 70.5)])
    s.line("FWC", [(4, 71), (14, 71)])
    s.point("FGP", 16, 51.5)
    s.point("FGV", 25, 51.5)
    s.point("FGS", 4, 60)
    s.point("FGF", 54, 60)
    # ---- street furniture -----------------------------------------------------------------------------------------------
    for x in (14, 40, 66, 92):
        s.point("SLS", x, 25.2)
    s.point("SLB", 58, 25.2)
    s.point("SSR", 52, 25.2)
    s.point("SSW", 64.5, 51.5)
    s.point("SSG", 8, 25.2)
    s.point("SSI", 90, 45)
    s.point("SBB", 45, 24.8)
    s.point("SBB", 47.5, 24.8)
    s.point("SBR", 50, 24.8)
    s.point("SBG", 74, 70)
    s.point("SBG", 64, 70)
    s.line("SBL", densify([(78, 72), (78, 56)], 2.5))
    s.point("SAS", 90, 49)
    s.point("SAB", 86, 49)
    s.point("SAR", 94, 49)
    s.point("SAF", 98, 49)
    s.point("STS", 57.2, 45.2)
    s.point("STS", 80.8, 45.2)
    s.point("STC", 58.6, 46.6)
    s.point("STK", 61, 46.2)
    # ---- drainage -----------------------------------------------------------------------------------------------------------
    s.line("KCP", [(20, 30.4), (60, 30.4)])
    for x in (20, 40):
        s.point("KPG", x, 28.7)
    s.point("KPK", 60, 28.7)
    s.point("KPJ", 36, 30.4)
    s.point("KPH", 50, 30.4)
    s.point("KPT", 8, 30.4)
    s.line("KCB", [(66, 27), (90, 27)])
    s.point("KPW", 66, 27)
    s.point("KPC", 90, 27)
    s.line("KCS", [(58, 52), (58, 60)])
    s.point("KPS", 58, 60)
    s.line("KWD", [(0, 19), (36, 19)])
    s.line("KWS", [(40, 18.5), (58, 17)])
    s.line("KWF", [(60, 16.5), (74, 14)])
    # ---- services: one run of each, spaced so each reads ---------------------------------------------------------------------
    s.line("UWM", [(0, 33), (100, 33)])
    s.line("UWB", [(34, 33), (34, 46)])
    s.line("UWD", [(5, 34.6), (22, 34.6)])
    for x, y in ((20, 33), (60, 33)):
        s.point("UWV", x, y)
    s.point("UWX", 34, 47)
    s.point("UWP", 80, 33)
    s.point("UWT", 98, 33)
    s.point("UWU", 2, 71)
    s.line("USM", [(0, 39), (100, 39)])
    s.line("USB", [(15, 39), (15, 52)])
    s.line("USR", [(78, 40), (78, 70)])
    s.line("USD", [(86, 41), (100, 41)])
    for x in (15, 45, 70):
        s.point("USH", x, 39)
    s.point("USE", 15, 51)
    s.point("UST", 91, 65)
    s.point("USP", 78, 71)
    s.line("UGM", [(0, 26.8), (100, 26.8)])
    s.line("UGB", [(34, 26.8), (34, 24.2)])
    s.line("UGD", [(2, 25.4), (14, 25.4)])
    s.point("UGV", 56, 26.8)
    s.point("UGR", 22, 26.8)
    s.point("UGX", 34, 24)
    s.point("UGP", 82, 26.8)
    s.point("UGK", 96, 26.8)
    s.line("UEC", [(0, 46.2), (55, 46.2)])
    s.line("UEO", [(6, 48.5), (30, 48.5), (54, 48.5)])
    s.line("UED", [(84, 42.6), (100, 42.6)])
    s.line("UEN", [(0, 45), (20, 45)])
    for x in (6, 30, 54):
        s.point("UEP", x, 48.5)
    s.point("UEY", 6, 50)
    s.point("UET", 84, 49.6)
    s.point("UEK", 46, 46.2)
    s.point("UEJ", 22, 46.2)
    s.line("UCC", [(0, 21), (100, 21)])
    s.line("UCN", [(36, 21), (36, 22.6)])
    s.line("UCD", [(70, 21.8), (86, 21.8)])
    s.point("UCP", 18, 21)
    s.point("UCH", 62, 21)
    s.point("UCK", 90, 21)
    s.line("URM", [(66, 50), (66, 72)])
    s.line("URB", [(66, 60), (60, 60)])
    s.line("URD", [(70, 31.2), (96, 31.2)])
    s.point("URV", 66, 56)
    s.point("URP", 66, 70)
    s.line("UFM", [(48, 33), (48, 27.4)])
    s.point("UFH", 48, 27)
    s.point("UFB", 68, 46.6)
    s.point("UFV", 48, 31)
    s.line("UPM", [(73, 50), (73, 72)])
    s.point("UPV", 73, 55)
    s.point("UPK", 73, 66)
    s.point("UPT", 96, 62)
    s.line("UIC", [(8, 41.6), (56, 41.6)])
    s.point("UIP", 56, 41.6)
    s.point("UIL", 24, 41)
    s.line("UXL", [(56.5, 52), (56.5, 69)])
    s.point("UXP", 56.5, 56)
    s.point("UXH", 56.5, 63)
    s.point("UXM", 46, 36)
    # ---- vegetation ---------------------------------------------------------------------------------------------------------------
    for x, code in ((6, "VTB"), (20, "VTG"), (34, "VTB"), (50, "VTC"), (64, "VTB"), (80, "VTP"), (94, "VTB")):
        s.point(code, x, 18)
    s.point("VTD", 42, 17.6)
    s.point("VTS", 72, 17.8)
    s.point("VTB", 40, 63)
    s.point("VTB", 12, 61)
    s.line("VTR", densify([(88, 71), (97, 71)], 3.2))
    s.line("VTK", ring(34, 18, 3.4), close=True)
    s.line("VSH", [(31, 52.9), (53, 52.9)])
    s.line("VGM", [(34, 56), (50, 56), (50, 54), (34, 54)], close=True)
    s.line("VGL", [(6, 67.6), (28, 67.6)])
    s.line("VGB", wave(0, 60, 13.2, 0.6, 18, 0.4, step=2.5))
    s.line("VGC", box(2, 4, 28, 10), close=True)
    for x in (6, 9, 11.5):
        s.point("VSS", x, 54.4)
    s.point("VSC", 91, 54)
    s.point("VST", 59.5, 64)
    s.point("VST", 60.2, 62.4)
    # ---- rail and a bridge ---------------------------------------------------------------------------------------------------------
    s.line("TTR", [(0, 5.0), (100, 5.0)])
    s.line("TTR", [(0, 6.435), (100, 6.435)])
    s.line("TTC", [(0, 5.7175), (100, 5.7175)])
    s.line("TTB", [(0, 3.4), (100, 3.4)])
    s.line("TTB", [(0, 8.1), (100, 8.1)])
    s.line("BSB", [(42, 2.6), (60, 2.6)])
    s.line("BSB", [(42, 8.9), (60, 8.9)])
    for x in (46, 52, 57):
        s.point("BSP", x, 5.7)
    s.line("TPE", [(64, 9.6), (92, 9.6)])
    s.line("TOW", [(0, 5.7), (100, 5.7)])
    for x in (10, 30, 74):
        s.point("TOM", x, 10.4)
    s.point("TOS", 96, 10.4)
    s.point("TTX", 20, 5.7)
    s.point("TTE", 100, 5.7)
    # ---- the ground: contours, creek, banks, shots -----------------------------------------------------------------------------------
    for k, base in enumerate((11.4, 14.2, 16.2)):
        s.line("GCI" if k == 2 else "GCM", wave(0, 100, base, 0.7, 38, k * 0.9))
    s.line("GCD", ring(14, 13, 1.6, 18), close=True)
    s.line("GWE", [(44, -3), (46, 0), (50, 3), (58, 5)])
    s.line("GWH", [(47, -3), (49, -0.5), (53, 2.2), (61, 4)])
    s.line("GBT", [(34, -3), (35.5, 0), (40, 2.2), (48, 4)])
    s.line("GBB", [(38, -3), (39.5, 0), (43, 1.4), (50, 3.2)])
    s.line("KWC", [(45, -3), (47, 0), (51, 2.6), (59, 4.4)])
    s.line("GBH", [(0, 15.4), (22, 15.4)])
    s.line("GBS", [(26, 15.6), (38, 16)])
    s.line("GBR", [(62, 13), (80, 15), (98, 14)])
    s.line("GBG", [(64, 11.4), (82, 12.4), (98, 11.6)])
    s.line("GBK", [(2, 12.2), (20, 12)])
    s.line("GBF", [(2, 9.6), (20, 9.4)])
    s.line("GBX", box(66, 7.8, 80, 10.4), close=True)
    s.line("GRO", [(84, 12.2), (88, 13.6), (92, 12.4), (96, 13.6)])
    s.line("GRC", [(74, 19.6), (78, 20.4), (82, 19.6), (86, 20.4)])
    s.point("GRB", 70, 17)
    for x, y, code in ((18, 15.2, "GPS"), (30, 12.6, "GPH"), (8, 16.2, "GPL"), (54, 15, "GPS"), (66, 15.6, "GPG"),
                       (12, 32, "GPR"), (14, 45.6, "GPI")):
        s.point(code, x, y)
    # ---- control, investigation, plan marks -----------------------------------------------------------------------------------------------
    s.point("MCT", 6, 17.6)
    s.point("MCC", 25, 21.8)
    s.point("MCB", 52, 20.4)
    s.point("MCP", 76.5, 49.2)
    s.point("MCG", 96, 15)
    s.point("MCW", 4, 45.8)
    s.point("MCI", 96.5, 43.4)
    s.point("MIB", 32, 12)
    s.point("MIT", 22, 10.6)
    s.point("MIP", 68, 41.5)
    s.point("MMN", 106, 62)
    s.line("MMS", [(-3, -4), (103, -4), (103, 74), (-3, 74)], close=True)
    s.line("MMV", ring(68, 41.5, 3.4), close=True)
    s.line("MML", [(41, 41), (45, 36.8)])
    # ---- hazards, heritage, temporary works -----------------------------------------------------------------------------------------------------
    s.point("XHZ", 8, 41.4)
    s.line("XHB", box(88, 11, 96, 16), close=True)
    s.point("XHT", 33, 50)
    s.point("XHH", 40, 23.2)
    s.point("XGP", 20, 38)
    s.point("XGQ", 2, 62)
    s.line("XGL", [(14, 70.6), (26, 70.6)])
    s.line("XTF", [(94, 52.4), (94, 58)])
    s.line("XTW", box(88, 28.8, 99, 32.2), close=True)
    # ---- words ------------------------------------------------------------------------------------------------------------------------------------------
    s.text("MTH", 0, 79, 3.6, "KATANA STANDARD - A STREET CORNER")
    s.text("MTN", 0, 76, 2.0, "Every mark on this plan is drawn by the library from its survey code")
    s.text("MTR", 33, 40.4, 2.2, "MAIN STREET")
    s.text("MTR", 72.6, 55, 2.2, "RIVER ROAD", 90)
    s.text("MTL", 4, 6.6, 1.8, "Rail corridor")
    s.text("MTS", 18.6, 14.2, 1.5, "RL 24.3")
    s.text("MTC", 60, 17.0, 1.5, "25")
    s.text("MTL", 10.5, 60.2, 1.8, "Lot 7")
    s.text("MTL", 36.5, 61.2, 1.8, "Lot 8")
    s.text("MTL", 86, 63.2, 1.8, "Lot 9")
    return s


def commands():
    scene = build()
    keys = known_keys()
    unknown = sorted({u for u in scene.uses if u not in keys})
    if unknown:
        raise SystemExit("showcase.py uses codes that codes.py does not have: " + ", ".join(unknown))
    return scene.commands + ["CODE"], sorted(set(scene.uses))


def main(argv):
    lines, used = commands()
    if "--keys" in argv:
        print("\n".join(used))
    else:
        sys.stdout.write("\n".join(lines) + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
