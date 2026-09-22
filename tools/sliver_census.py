# tools/sliver_census.py <file.12da> [...]
#
# Counts the FLAT triangles in a 12da's tins, without using any of Katana's
# code. It exists because "a surface will not read back" was blamed on a
# duplicated triangle, and the real cause - a triangle with no plan area,
# whose listed direction is therefore noise - needed evidence from outside
# the program that was getting it wrong. See docs/interop.md and PLAN.MD 20.2.
#
# It reads the archive's own hexadecimal floats, which are exact doubles,
# applies the same filter the importer applies (nulled triangles, the four
# construction points of a full_tin, null heights, repeated vertices), and
# reports for each tin how many surviving triangles change the SIGN of their
# plan area when every coordinate is rounded to eight decimal places - the
# precision a 12da carries when it is not written in hex.
#
# Measured on plot_PW_example_data.12da (2026-09-22): LOTS 2, ROADS 4,
# ROADS DETAIL 1, and the super tin that ranks them inherits all seven. Those
# are exactly the four surfaces that would not read back.
import io
import re
import sys

NULL = -999.0


def parse_number(token):
    if token.startswith('0x') or token.startswith('-0x'):
        return float.fromhex(token)
    return float(token)


def block(body, name):
    """The text inside `name {` up to its matching `}`."""
    at = body.index(name + ' {')
    depth = 0
    i = body.index('{', at)
    start = i + 1
    while True:
        if body[i] == '{':
            depth += 1
        elif body[i] == '}':
            depth -= 1
            if depth == 0:
                return body[start:i]
        i += 1


def tins(text):
    for match in re.finditer(r'\n(full_tin|tin) \{', text):
        kind = match.group(1)
        depth = 0
        i = text.index('{', match.start() + 1)
        start = i + 1
        while True:
            if text[i] == '{':
                depth += 1
            elif text[i] == '}':
                depth -= 1
                if depth == 0:
                    break
            i += 1
        body = text[start:i]
        name = re.search(r'name\s+"([^"]*)"', body)
        yield kind, (name.group(1) if name else ''), body


def area2(a, b, c):
    return (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])


def census(path):
    raw = io.open(path, 'rb').read()
    text = raw.decode('utf-16' if raw[:2] in (b'\xff\xfe', b'\xfe\xff') else 'utf-8', 'replace')
    for kind, name, body in tins(text):
        points = []
        for line in block(body, 'points').strip().splitlines():
            parts = line.split()
            if len(parts) == 3:
                x, y, z = (parse_number(p) for p in parts)
                points.append((x, y, None if z == NULL else z))
        triangles = []
        for line in block(body, 'triangles').strip().splitlines():
            parts = line.split()
            if len(parts) == 3:
                triangles.append(tuple(int(p) - 1 for p in parts))
        visible = []
        if 'nulling {' in body:
            for line in block(body, 'nulling').strip().splitlines():
                visible.extend(token != '1' for token in line.split())

        kept, flat, smallest = 0, 0, None
        for index, tri in enumerate(triangles):
            if index < len(visible) and not visible[index]:
                continue
            if kind == 'full_tin' and min(tri) < 4:
                continue  # touches a construction point
            if any(points[v][2] is None for v in tri) or len(set(tri)) != 3:
                continue
            kept += 1
            a, b, c = (points[v] for v in tri)
            exact = area2(a, b, c)
            rounded = area2(*[(round(p[0], 8), round(p[1], 8)) for p in (a, b, c)])
            if (exact > 0.0) != (rounded > 0.0):
                flat += 1
            if smallest is None or abs(exact) < smallest:
                smallest = abs(exact)
        print('%-24s %-9s kept %6d  flat %3d  smallest |2A| %.3e'
              % (name, kind, kept, flat, smallest if smallest is not None else float('nan')))


if __name__ == '__main__':
    if len(sys.argv) < 2:
        sys.exit('usage: sliver_census.py <file.12da> [...]')
    for argument in sys.argv[1:]:
        print(argument)
        census(argument)
