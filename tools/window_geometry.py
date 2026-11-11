#!/usr/bin/env python3
# tools/window_geometry.py --build <C++ build tree> [options]
#
# The headless window's geometry, measured from the C++ window's own
# screenshots (the step 7 plan, section 4.3): where the plan view and each
# dock lie in the 1360 x 860 window a headless run lays out, so that the Rust
# window (katana_window, the main window's headless layout) is laid out to the
# same pixels. Without it the plan view's pixel size differs, and so does every
# pointer record's model point (pointer: action=hover x=110.3 ...) through the
# view's transform, which the headless tests pin exactly.
#
# RUN BOOK (the integrator's, once after wave 0; it is run by nobody else)
#
#   In WSL, after a full build of the C++ (its bin/katana), as the suite runs
#   the window - under `xvfb-run -a`, the window itself offscreen:
#
#     xvfb-run -a python3 tools/window_geometry.py \
#         --build ~/katana-build/rust-port/linux-release
#
#   It writes into <output> (default <build>/window_geometry), one folder a
#   variant, every PNG and every run's streams, and prints its records, also
#   kept in <output>/geometry.txt and, as data, <output>/geometry.json. The
#   numbers go into step7_api.md and docs/rust.md with their source (this
#   tool, the build, the date). Exit 0 when every run of the window it needed
#   gave its picture; 1 when one did not (each said); 2 when it refuses to run.
#
# HOW IT RUNS THE WINDOW
#
#   As tools/check_screenshot.cmake runs it: <cpp-bin>/katana <a copy of the
#   project, or an empty folder> [files to import] [--command <line>]...
#   [--panel <dock>] --screenshot <png>, in the environment the build gives
#   its headless tests - read from the build's own qt_screenshot_headless
#   (--environment-test), its ENVIRONMENT_MODIFICATION applied by
#   ctest_tree.environment_of - so that the window is laid out as the tests
#   lay it out. A run that would not be offscreen is refused: the window must
#   never open on a person's screen (WSLg gives every process a display).
#   Each run opens a fresh copy of the project; the sample itself is never
#   opened, as opening can back a project up or migrate it in place.
#
#   For each variant (VARIANTS: no project, the sample, a second plan view,
#   a 3D, section and elevation view, a raster surface in 3D, an archive with
#   tins, the window at 125 %) it runs the window once whole, with
#   `--command VIEWS` last so that the views' records say which views there
#   are; then once for each dock (DOCKS, and View<id> for each view), the
#   same steps then `--panel <dock>`, which makes the dock the target, so the
#   PNG is the dock alone.
#
# WHAT IT MEASURES, in the PNG's pixels (device pixels: at 125 % they are
# 1.25 times the logical ones), x and y from the window's top left
#
#   window   the window PNG's size.
#   dock     a dock's rectangle: its size is its own PNG's; its place is where
#            that picture is found in the window's, row for row (the dock is
#            painted by the same code in both; "located=" says what share of
#            its rows matched exactly, 0.9 at least, else located=no - a dock
#            hidden or behind another tab is not in the window's picture).
#            A dock the window does not have is present=no.
#   view     a view's dock as above, and its viewport - the widget the
#            pointer's transform is - as the box of the view's background
#            colour inside the dock's picture (BACKGROUNDS: the C++'s own
#            colours), with the share of the dock it covers; and, for a plan
#            view, the size its VIEWS record implies, (x1 - x0) and (y1 - y0)
#            of area= times scale= (pixels per unit), as a check on the box.
#   viewport each large region (1 % of the window or more) of a view's
#            background colour in the window's picture, whatever dock it is in.
#   The box of a colour is a measurement, not a reading of the code: a drawing
#   that covers a viewport's whole edge would shrink it; the record size and
#   the PNGs (kept) are there to check it by.

import argparse
import concurrent.futures
import json
import os
import re
import shutil
import struct
import subprocess
import sys
import zlib

sys.dont_write_bytecode = True  # a probe leaves nothing behind in the tools folder
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import ctest_tree  # noqa: E402 - the shared reader, beside this file

CHECKOUT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


class Variant:
    def __init__(self, name, project, imports, commands, environment, why):
        self.name = name
        self.project = project  # below the checkout, or None for an empty folder
        self.imports = imports  # below the checkout
        self.commands = commands
        self.environment = environment
        self.why = why


# The windows the headless tests lay out (counts from the build's CTest files
# on 2026-11-11, the step 7 plan, section 1).
VARIANTS = (
    Variant('empty', None, (), (), {}, 'no project, an empty folder: how 204 of the 267 headless tests start'),
    Variant('site_plan', 'samples/site_plan', (), (), {}, 'the sample project, as 38 tests open it'),
    Variant('site_plan_two_plans', 'samples/site_plan', (), ('VIEWS OPEN plan',), {},
            'a second plan view (VIEWS OPEN plan, in 12 tests): where it docks, and both viewports'),
    Variant('site_plan_3d', 'samples/site_plan', (), ('VIEWS OPEN 3d',), {},
            'a 3D view beside the plan view (VIEWS OPEN 3d, in 6 tests)'),
    Variant('site_plan_section', 'samples/site_plan', (), ('VIEWS OPEN section',), {},
            'a section view (VIEWS OPEN section, in 1 test)'),
    Variant('site_plan_elevation', 'samples/site_plan', (), ('VIEWS OPEN elevation',), {},
            'an elevation view, the fourth kind VIEWS OPEN takes'),
    Variant('terrain_3d', None, ('samples/gis/terrain.asc',), ('VIEWS OPEN 3d',), {},
            'a raster surface in a 3D view, as RenderView2 is driven in 20 test steps'),
    Variant('archive_tins', None, ('tests/archive12d/data/multiple_tins.12da',), (), {},
            'an archive (.12da) holding tins, which opens a 3D view in place of the plan view'),
    Variant('empty_at_125_percent', None, (), (), {'QT_SCALE_FACTOR': '1.25'},
            'the window at QT_SCALE_FACTOR=1.25, as one headless test runs it'),
)

# The window's own docks (main_window.cpp, terrain/surfaces_panel.cpp); a
# view's dock is View<id> (view_workspace.cpp).
DOCKS = ('LayersDock', 'PropertiesDock', 'SurfacesDock', 'ReferenceDataDock', 'CommandLineDock')

# Each view kind's background, as the C++ paints it.
BACKGROUNDS = {
    'plan': ((0x1e, 0x23, 0x29), 'theme::viewport(), theme.cpp; ViewportWidget fills with it'),
    '3d': ((28, 30, 36), 'kBackground, render_view_widget.cpp'),
    'elevation': ((28, 30, 36), 'kBackground, render_view_widget.cpp: an elevation is a render view'),
    'section': ((24, 26, 32), 'kBackground, section_view_widget.cpp'),
}

# ---- PNG, read as far as Qt writes it ---------------------------------------------------


class Image:
    """Rows of RGB bytes, three a pixel."""

    def __init__(self, width, height, rows):
        self.width = width
        self.height = height
        self.rows = rows


def paeth(a, b, c):
    p = a + b - c
    pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
    if pa <= pb and pa <= pc:
        return a
    return b if pb <= pc else c


def read_png(path):
    """The PNG as an Image (8 bits a sample, not interlaced: what QImage writes)."""
    with open(path, 'rb') as stream:
        data = stream.read()
    if data[:8] != b'\x89PNG\r\n\x1a\n':
        raise ValueError(f'{path} is not a PNG')
    at = 8
    header = None
    palette = b''
    compressed = []
    while at < len(data):
        length, kind = struct.unpack('>I4s', data[at:at + 8])
        body = data[at + 8:at + 8 + length]
        at += 12 + length
        if kind == b'IHDR':
            header = struct.unpack('>IIBBBBB', body)
        elif kind == b'PLTE':
            palette = body
        elif kind == b'IDAT':
            compressed.append(body)
        elif kind == b'IEND':
            break
    width, height, depth, colour, _, _, interlace = header
    channels = {0: 1, 2: 3, 3: 1, 4: 2, 6: 4}.get(colour)
    if depth != 8 or channels is None or interlace:
        raise ValueError(f'{path}: depth {depth}, colour type {colour}, interlace {interlace} is not read here')
    raw = zlib.decompress(b''.join(compressed))
    stride = width * channels
    rows = []
    previous = bytearray(stride)
    at = 0
    seven = int.from_bytes(b'\x7f' * stride, 'big')
    eight = int.from_bytes(b'\x80' * stride, 'big')
    for _ in range(height):
        kind = raw[at]
        line = bytearray(raw[at + 1:at + 1 + stride])
        at += 1 + stride
        if kind == 1:
            for i in range(channels, stride):
                line[i] = (line[i] + line[i - channels]) & 255
        elif kind == 2:
            # Byte by byte addition without carries, on whole rows at once.
            x, y = int.from_bytes(line, 'big'), int.from_bytes(previous, 'big')
            line = bytearray((((x & seven) + (y & seven)) ^ ((x ^ y) & eight)).to_bytes(stride, 'big'))
        elif kind == 3:
            for i in range(stride):
                left = line[i - channels] if i >= channels else 0
                line[i] = (line[i] + ((left + previous[i]) >> 1)) & 255
        elif kind == 4:
            for i in range(stride):
                if i >= channels:
                    line[i] = (line[i] + paeth(line[i - channels], previous[i], previous[i - channels])) & 255
                else:
                    line[i] = (line[i] + previous[i]) & 255
        elif kind != 0:
            raise ValueError(f'{path}: a row filter {kind} PNG does not have')
        previous = line
        if channels == 3:
            rows.append(bytes(line))
        elif channels == 4:
            rgb = bytearray(line)
            del rgb[3::4]
            rows.append(bytes(rgb))
        elif channels == 2:
            grey = bytes(line[0::2])
            rows.append(bytes(value for value in grey for _ in range(3)))
        elif colour == 0:
            rows.append(bytes(value for value in line for _ in range(3)))
        else:
            rows.append(b''.join(palette[3 * index:3 * index + 3] for index in line))
    return Image(width, height, rows)


# ---- measures ---------------------------------------------------------------------------

RUN = re.compile(rb'\x01+')


def tables(colour):
    return [bytes(1 if value == part else 0 for value in range(256)) for part in colour]


def mask(row, lookups):
    """1 where the row's pixel is the colour, else 0: one byte a pixel."""
    count = len(row) // 3
    red = int.from_bytes(row[0::3].translate(lookups[0]), 'big')
    green = int.from_bytes(row[1::3].translate(lookups[1]), 'big')
    blue = int.from_bytes(row[2::3].translate(lookups[2]), 'big')
    return (red & green & blue).to_bytes(count, 'big')


def colour_box(image, colour, region=None):
    """(x, y, width, height, pixels) of every pixel of the colour in the
    region (x, y, width, height; the whole image by default), or None."""
    x0, y0, width, height = region or (0, 0, image.width, image.height)
    lookups = tables(colour)
    left, right, top, bottom, pixels = None, None, None, None, 0
    for y in range(y0, y0 + height):
        line = mask(image.rows[y][3 * x0:3 * (x0 + width)], lookups)
        first = line.find(b'\x01')
        if first < 0:
            continue
        last = line.rfind(b'\x01')
        pixels += line.count(b'\x01')
        left = first if left is None else min(left, first)
        right = last if right is None else max(right, last)
        top = y if top is None else top
        bottom = y
    if left is None:
        return None
    return (x0 + left, top, right - left + 1, bottom - top + 1, pixels)


def regions(image, colour, minimum):
    """Each 4-connected region of the colour of `minimum` pixels or more:
    [(x, y, width, height, pixels)], largest first."""
    lookups = tables(colour)
    parent = []
    boxes = []

    def find(index):
        while parent[index] != index:
            parent[index] = parent[parent[index]]
            index = parent[index]
        return index

    above = []
    for y in range(image.height):
        here = []
        for match in RUN.finditer(mask(image.rows[y], lookups)):
            start, end = match.span()
            index = len(parent)
            parent.append(index)
            boxes.append([start, y, end, y + 1, end - start])
            here.append((start, end, index))
        a = b = 0
        while a < len(above) and b < len(here):
            start_a, end_a, index_a = above[a]
            start_b, end_b, index_b = here[b]
            if start_a < end_b and start_b < end_a:
                root_a, root_b = find(index_a), find(index_b)
                if root_a != root_b:
                    parent[root_b] = root_a
            if end_a < end_b:
                a += 1
            else:
                b += 1
        above = here
    totals = {}
    for index, (x0, y0, x1, y1, pixels) in enumerate(boxes):
        root = find(index)
        box = totals.setdefault(root, [x0, y0, x1, y1, 0])
        box[0], box[1] = min(box[0], x0), min(box[1], y0)
        box[2], box[3] = max(box[2], x1), max(box[3], y1)
        box[4] += pixels
    found = [(x0, y0, x1 - x0, y1 - y0, pixels) for x0, y0, x1, y1, pixels in totals.values() if pixels >= minimum]
    return sorted(found, key=lambda box: -box[4])


def distinct(row):
    return len({row[i:i + 3] for i in range(0, len(row), 3)})


def locate(window, part):
    """Where the part's picture lies in the window's: (x, y, share of rows
    that match exactly), the share 0.9 at least; or None."""
    if part.width > window.width or part.height > window.height:
        return None
    width = 3 * part.width
    sample = range(0, part.height, max(1, part.height // 64))
    anchors = sorted(sample, key=lambda j: -distinct(part.rows[j]))[:5]
    for j in anchors:
        needle = part.rows[j]
        for y in range(j, window.height - part.height + j + 1):
            row = window.rows[y]
            start = 0
            while True:
                at = row.find(needle, start)
                if at < 0:
                    break
                start = at + 1
                if at % 3:
                    continue
                top = y - j
                matched = sum(1 for k in range(part.height) if window.rows[top + k][at:at + width] == part.rows[k])
                if matched >= 0.9 * part.height:
                    return (at // 3, top, matched / part.height)
    return None


# ---- the views' records -----------------------------------------------------------------

VIEW_RECORD = re.compile(r'\bview=([0-9]+) kind=([a-z0-9]+)\b.*')
FIELD = re.compile(r'([A-Za-z_]+)=("(?:[^"\\]|\\.)*"|\S*)')


def view_records(text):
    """{id: {key: value}} of the VIEWS records the run printed; the last of each id."""
    records = {}
    for line in text.split('\n'):
        match = VIEW_RECORD.search(line)
        if match:
            records[int(match.group(1))] = dict(FIELD.findall(match.group(0)))
    return records


def record_size(record):
    """A plan view's size in pixels from its record: area times scale, or None."""
    try:
        scale = float(record['scale'])
        x0, y0, x1, y1 = (float(value) for value in record['area'].split(','))
    except (KeyError, ValueError):
        return None
    return ((x1 - x0) * scale, (y1 - y0) * scale)


# ---- the runs ---------------------------------------------------------------------------

class Run:
    def __init__(self, variant, dock):
        self.variant = variant
        self.dock = dock  # None for the whole window
        self.name = dock or 'window'
        self.code = None
        self.error = ''
        self.text = ''
        self.png = None


def run_window(program, environment, output, variant, dock, timeout):
    """One run of the C++ window, as check_screenshot.cmake runs it."""
    run = Run(variant, dock)
    folder = os.path.join(output, variant.name)
    copy = os.path.join(folder, run.name + '_project')
    shutil.rmtree(copy, ignore_errors=True)
    if variant.project:
        shutil.copytree(os.path.join(CHECKOUT, variant.project), copy)
    else:
        os.makedirs(copy)
    png = os.path.join(folder, run.name + '.png')
    if os.path.exists(png):
        os.remove(png)
    command = [program, copy] + [os.path.join(CHECKOUT, path) for path in variant.imports]
    for line in variant.commands + ('VIEWS',):
        command += ['--command', line]
    if dock:
        command += ['--panel', dock]
    command += ['--screenshot', png]
    run_environment = dict(environment)
    run_environment.update(variant.environment)
    try:
        done = subprocess.run(command, env=run_environment, cwd=folder, capture_output=True, timeout=timeout)
        run.code = done.returncode
        out, err = done.stdout, done.stderr
    except subprocess.TimeoutExpired as expired:
        run.code = 'timeout'
        out, err = expired.stdout or b'', expired.stderr or b''
    for suffix, data in (('out', out), ('err', err)):
        with open(os.path.join(folder, f'{run.name}.{suffix}'), 'wb') as stream:
            stream.write(data)
    with open(os.path.join(folder, f'{run.name}.command'), 'w', encoding='utf-8') as stream:
        json.dump(command, stream, indent=1)
    run.text = (out + b'\n' + err).decode('utf-8', 'replace')
    shutil.rmtree(copy, ignore_errors=True)
    if run.code == 0 and os.path.isfile(png):
        run.png = png
    else:
        lines = [line for line in err.decode('utf-8', 'replace').split('\n') if line.strip()]
        run.error = lines[-1] if lines else f'exit {run.code}, nothing on standard error'
    return run


def rect(box):
    return ','.join(str(value) for value in box[:4])


def hexed(colour):
    return '#' + ''.join(f'{part:02x}' for part in colour)


def measure(variant, window_run, dock_runs, scale):
    """The records and the data of one variant, from its runs' PNGs."""
    records = []
    data = {'why': variant.why, 'scale': scale, 'docks': {}, 'views': {}, 'viewports': []}
    if window_run.png is None:
        records.append(f'window variant={variant.name} present=no error={json.dumps(window_run.error)}')
        return records, data
    window = read_png(window_run.png)
    views = view_records(window_run.text)
    data['window'] = [window.width, window.height]
    listed = ','.join(f'{key}:{views[key].get("kind")}' for key in sorted(views)) or 'none'
    records.append(f'window variant={variant.name} scale={scale} size={window.width}x{window.height} views={listed}')
    minimum = window.width * window.height // 100
    seen = set()
    for kind, (colour, _) in BACKGROUNDS.items():
        if colour in seen:
            continue
        seen.add(colour)
        for box in regions(window, colour, minimum):
            records.append(f'viewport variant={variant.name} colour={hexed(colour)} rect={rect(box)} pixels={box[4]}')
            data['viewports'].append({'colour': hexed(colour), 'rect': list(box[:4]), 'pixels': box[4]})
    for run in dock_runs:
        entry = {}
        if run.png is None:
            records.append(f'dock variant={variant.name} name={run.dock} present=no error={json.dumps(run.error)}')
            data['docks'][run.dock] = {'present': False, 'error': run.error}
            continue
        part = read_png(run.png)
        place = locate(window, part)
        entry['size'] = [part.width, part.height]
        if place:
            entry['rect'] = [place[0], place[1], part.width, part.height]
            entry['located'] = round(place[2], 3)
            where = f'rect={place[0]},{place[1]},{part.width},{part.height} located={place[2]:.3f}'
        else:
            where = f'rect=none size={part.width}x{part.height} located=no'
        view = re.fullmatch(r'View([0-9]+)', run.dock)
        if not view:
            records.append(f'dock variant={variant.name} name={run.dock} {where}')
            data['docks'][run.dock] = entry
            continue
        identity = int(view.group(1))
        record = views.get(identity, {})
        kind = record.get('kind', 'unknown')
        line = f'view variant={variant.name} view={identity} kind={kind} dock={run.dock} {where}'
        background = BACKGROUNDS.get(kind)
        if background:
            box = colour_box(part, background[0])
            if box:
                share = box[4] / (part.width * part.height)
                inside = list(box[:4])
                entry['viewport_in_dock'] = inside
                entry['background'] = hexed(background[0])
                entry['share'] = round(share, 3)
                line += f' viewport_in_dock={rect(box)}'
                if place:
                    absolute = [place[0] + box[0], place[1] + box[1], box[2], box[3]]
                    entry['viewport'] = absolute
                    line += f' viewport={rect(absolute)}'
                line += f' background={hexed(background[0])} share={share:.3f}'
            else:
                line += f' viewport=none background={hexed(background[0])}'
        if kind == 'plan':
            size = record_size(record)
            if size:
                entry['record_size'] = [round(size[0], 3), round(size[1], 3)]
                line += f' record_size={size[0]:.3f}x{size[1]:.3f}'
            if 'framed' in record:
                line += f' framed={record["framed"]}'
        entry['record'] = record
        records.append(line)
        data['views'][str(identity)] = entry
    return records, data


def main():
    parser = argparse.ArgumentParser(description='The headless window\'s geometry, measured from the C++ '
                                     'window\'s screenshots (the top of this file).')
    parser.add_argument('--build', required=True, help='the C++ build tree: its bin/katana and CTest files')
    parser.add_argument('--program', help='the C++ window (default <cpp-bin>/katana)')
    parser.add_argument('--output', help='where the PNGs and records go (default <build>/window_geometry)')
    parser.add_argument('--environment-test', default='qt_screenshot_headless',
                        help='the headless test whose environment the window is run in')
    parser.add_argument('--variants', help='the variants to run, by name, comma-separated (default all)')
    parser.add_argument('--jobs', type=int, default=4, help='windows run at once, and pictures measured at once')
    parser.add_argument('--timeout', type=int, default=120, help='seconds a run may take (check_screenshot.cmake\'s)')
    arguments = parser.parse_args()

    build = arguments.build
    program = arguments.program or ctest_tree.default_cpp_bin(build).rstrip('/') + '/katana'
    output = arguments.output or os.path.join(build, 'window_geometry')
    if not os.path.isfile(program):
        print(f'window_geometry: refused: {program} is not there; build the C++ window first')
        return 2
    try:
        tree = ctest_tree.read_tree(build)
    except ctest_tree.CMakeError as error:
        print(f'window_geometry: refused: a CTest file cannot be read: {error}')
        return 2
    test = tree.tests.get(arguments.environment_test)
    if test is None:
        print(f'window_geometry: refused: the build has no test {arguments.environment_test} to take the '
              'headless environment from')
        return 2
    try:
        environment = ctest_tree.environment_of(test, os.environ)
    except ctest_tree.CMakeError as error:
        print(f'window_geometry: refused: {error}')
        return 2
    variants = list(VARIANTS)
    if arguments.variants:
        chosen = arguments.variants.split(',')
        unknown = [name for name in chosen if name not in {variant.name for variant in VARIANTS}]
        if unknown:
            print(f'window_geometry: refused: no variant {", ".join(unknown)}; the variants are '
                  + ', '.join(variant.name for variant in VARIANTS))
            return 2
        variants = [variant for variant in VARIANTS if variant.name in chosen]
    for variant in variants:
        platform = dict(environment, **variant.environment).get('QT_QPA_PLATFORM')
        if platform != 'offscreen':
            print(f'window_geometry: refused: variant {variant.name} would run the window with '
                  f'QT_QPA_PLATFORM={platform}, not offscreen: it would open on a screen')
            return 2
    for variant in variants:
        shutil.rmtree(os.path.join(output, variant.name), ignore_errors=True)
        os.makedirs(os.path.join(output, variant.name))
    print(f'== window_geometry: {program} in the environment of {test.name}, {len(variants)} variants, '
          f'into {output}', flush=True)

    with concurrent.futures.ThreadPoolExecutor(arguments.jobs) as pool:
        windows = dict(zip((variant.name for variant in variants), pool.map(
            lambda variant: run_window(program, environment, output, variant, None, arguments.timeout), variants)))
        jobs = []
        for variant in variants:
            views = view_records(windows[variant.name].text)
            for dock in DOCKS + tuple(f'View{identity}' for identity in sorted(views)):
                jobs.append((variant, dock))
        docks = list(pool.map(lambda job: run_window(program, environment, output, job[0], job[1],
                                                     arguments.timeout), jobs))

    failed = [run for run in windows.values() if run.png is None]
    records = []
    data = {'program': program, 'build': build, 'environment_test': test.name,
            'backgrounds': {kind: {'colour': hexed(colour), 'source': source}
                            for kind, (colour, source) in BACKGROUNDS.items()},
            'variants': {}}
    with concurrent.futures.ProcessPoolExecutor(arguments.jobs) as pool:
        measured = pool.map(measure, variants, [windows[variant.name] for variant in variants],
                            [[run for run in docks if run.variant.name == variant.name] for variant in variants],
                            [variant.environment.get('QT_SCALE_FACTOR', environment.get('QT_SCALE_FACTOR', '1'))
                             for variant in variants])
        for variant, (lines, variant_data) in zip(variants, measured):
            records += lines
            data['variants'][variant.name] = variant_data
    for line in records:
        print(line)
    with open(os.path.join(output, 'geometry.txt'), 'w', encoding='utf-8', newline='\n') as stream:
        stream.write(''.join(line + '\n' for line in records))
    with open(os.path.join(output, 'geometry.json'), 'w', encoding='utf-8', newline='\n') as stream:
        json.dump(data, stream, indent=1)
        stream.write('\n')
    for run in failed:
        print(f'window_geometry: failed: {run.variant.name}: the window gave no picture: {run.error}')
    print(f'window_geometry: {len(variants)} variants, {len(variants) + len(docks)} runs of the window; '
          f'{len(failed)} without a picture of the window; records in {os.path.join(output, "geometry.txt")}')
    return 1 if failed else 0


if __name__ == '__main__':
    sys.exit(main())
