#!/usr/bin/env python3
# tools/check_rust_layering.py [--crates DIR] [--partial] [checkout-root]
#
# The `rust_layering` test: the Rust port's crates keep the layering rule of
# tools/check_layering.cmake (docs/rust.md, "The workspace").
#
# Each crate of rust/crates is one layer's: katana_<layer>, with two names
# that differ - katana_window is the qt layer's (the window is not Qt in the
# port) and katana_updater, the helper program, is the update layer's, as
# src/katana_updater is. It checks, against the table's KATANA_ALLOWED_<layer>
# lines, which it reads rather than repeats:
#   * every layer of the table has a crate, and every crate is a layer's
#     (--partial skips the first half, for a fixture of a few crates);
#   * a crate's katana_* [dependencies] and [build-dependencies] are its own
#     layer's crates or ones the table allows it. [dev-dependencies] are not
#     checked, as tests/ is not in the C++ check: a test may reach what its
#     layer may not;
#   * no crate re-exports a third-party crate it depends on (`pub use rusqlite
#     ...`), which would put a third-party type in its public interface - the
#     table's second rule (Rule 4).
#
# Prints every violation as "<where>: <what>" and exits 1 when there is one.
# Needs Python 3.11 or later (tomllib).

import argparse
import os
import re
import sys
import tomllib

# Crates whose name is not katana_<layer>.
CRATE_LAYER = {'katana_window': 'qt', 'katana_updater': 'update'}

ALLOWED = re.compile(r'set\(\s*KATANA_ALLOWED_(\w+)\s+"([^"]*)"\s*\)')
# Plain `pub` only: `pub(crate) use` keeps the name inside the crate.
REEXPORT = re.compile(r'^\s*pub\s+(?:use\s+(?:::)?|extern\s+crate\s+)([A-Za-z_]\w*)',
                      re.MULTILINE)


def read_table(path):
    with open(path, encoding='utf-8') as handle:
        text = handle.read()
    table = {layer: {d for d in deps.split(';') if d} for layer, deps in ALLOWED.findall(text)}
    if not table:
        sys.exit(f'{path}: no KATANA_ALLOWED_<layer> lines found')
    return table


def layer_of(crate):
    if crate in CRATE_LAYER:
        return CRATE_LAYER[crate]
    return crate[len('katana_'):] if crate.startswith('katana_') else None


def rust_sources(folder):
    for directory, _, names in os.walk(os.path.join(folder, 'src')):
        for name in sorted(names):
            if name.endswith('.rs'):
                yield os.path.join(directory, name)


def check(crates_dir, table, partial, root):
    problems = []
    crates = {}
    for name in sorted(os.listdir(crates_dir)):
        manifest = os.path.join(crates_dir, name, 'Cargo.toml')
        if os.path.isfile(manifest):
            with open(manifest, 'rb') as handle:
                crates[name] = tomllib.load(handle)

    present_layers = set()
    for crate, manifest in crates.items():
        package = manifest.get('package', {}).get('name', crate)
        if package != crate:
            problems.append(f'{crate}/Cargo.toml: the package is named {package}, not {crate}')
        layer = layer_of(crate)
        if layer not in table:
            problems.append(f'{crate}: is not a layer of tools/check_layering.cmake')
            continue
        present_layers.add(layer)
        own = {c for c in crates if layer_of(c) == layer}
        allowed = table[layer]
        third_party = set()
        for section in ('dependencies', 'build-dependencies'):
            for dependency in manifest.get(section, {}):
                normalised = dependency.replace('-', '_')
                if normalised.startswith('katana_'):
                    target = layer_of(normalised)
                    if normalised not in own and target not in allowed:
                        problems.append(f"{crate}/Cargo.toml: layer '{layer}' must not depend "
                                        f"on '{target}' ({section})")
                else:
                    third_party.add(normalised)
        for source in rust_sources(os.path.join(crates_dir, crate)):
            with open(source, encoding='utf-8', errors='replace') as handle:
                text = handle.read()
            for name in REEXPORT.findall(text):
                if name in third_party:
                    where = os.path.relpath(source, root).replace(os.sep, '/')
                    problems.append(f"{where}: re-exports the third-party crate '{name}', "
                                    'putting its types in the public interface')
    if not partial:
        for layer in sorted(set(table) - present_layers):
            problems.append(f"layer '{layer}' has no crate in rust/crates")
    return problems, len(crates)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('root', nargs='?', default='.')
    parser.add_argument('--crates', help='the crates folder (default: <root>/rust/crates)')
    parser.add_argument('--partial', action='store_true',
                        help='do not require a crate for every layer (fixtures)')
    args = parser.parse_args()
    root = os.path.abspath(args.root)
    crates_dir = os.path.abspath(args.crates or os.path.join(root, 'rust', 'crates'))
    table = read_table(os.path.join(root, 'tools', 'check_layering.cmake'))
    problems, count = check(crates_dir, table, args.partial, root)
    if problems:
        print('Rust layering violations:')
        for problem in problems:
            print('  ' + problem)
        return 1
    print(f'Rust layering check passed: {count} crates, {len(table)} layers.')
    return 0


if __name__ == '__main__':
    sys.exit(main())
