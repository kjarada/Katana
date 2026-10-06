# tools/reference_census.py <file> <file> <file> <file>
#                            [--strip WORD]... [--remove WORD]...
#                            [--colours TABLE] [--output FILE]
#
# An independent census of a customisation kept in the legacy formats: two
# style libraries (.4d), one of linestyles and one of symbols, and two survey
# code files (.mapfile), the survey code file and the names file.
#
# THE FOUR ARE GIVEN IN LOAD ORDER, and the order is part of what is counted,
# as it is part of what a conversion does: of two libraries that define one
# name the block of the one given LATER is the definition kept, and the rules
# of the survey code file given EARLIER come first, where order is precedence.
# So the files are named here as they are named to the converter
# (docs/customisation.md, "The reference customisation"), and the census says
# the order it was given, as `loadOrder`. The order was once written into this
# script - the linestyle library, then the symbol library - which made it the
# census of one order only: with the libraries given the other way round,
# three names are another definition each and the figures that count them
# differ.
#
# What each file IS is found by looking inside it, never by its place on the
# command line: a survey code file is XML that holds a <map_file>, and
# anything else that holds a paperstyle, worldstyle or twoptstyle block is a
# style library. Which library is the SYMBOL library is said by its file name
# holding "symbol", in any letter case - the format keeps one kind a library
# and says which nowhere else - and which survey code file is the NAMES file
# by its name holding "names". One of each, or there is no census.
#
# It uses none of Katana's code: a small tokenizer for the style library
# grammar and the standard library's XML parser for the survey code files. Its
# figures are what a converted Katana customisation is checked against
# (tools/customisation_census.py gives the same figures from the converted
# file; tests/archive12d/customisation/test_convert.cpp restates them), so
# that the converter is never only agreeing with the readers it was built on.
#
# --strip WORD (repeatable) takes a leading word, with the blanks after it,
# off the front of every definition name, every group path, every rule group
# and every linestyle and symbol name a rule uses - what the converter's
# --strip-leading-word does - so the figures can be had as the converted file
# will have them. --remove WORD (repeatable) is the converter's --remove-word:
# nothing is taken out here, the rule comments holding the word are counted.
# With either, the census gains `carried`: how many names, group paths, rule
# groups, rule references and rule comments held such a word - the figures
# the converter reports as what it renamed and changed.
#
# What else a conversion reports is counted too, so that each of its figures
# has a source that is not the converter:
#   replaced          the blocks another block of the same name took the place
#                     of - within one file and across the two, how many of them
#                     are word for word the block that replaced them, and how
#                     many rules name one replaced across the files as their
#                     linestyle or as their symbol; then what the ORDER made
#                     of those replaced across the files: which library's
#                     block each name kept, and how many rules name one as
#                     the kind that WENT - as their linestyle where the symbol
#                     library's block is the one kept, as their symbol where
#                     the linestyle library's is. Those rules would draw with
#                     a definition they were not written for;
#   inferredEncoding  for each file, the characters outside ASCII it holds when
#                     it is neither UTF-8 nor marked as UTF-16 and so is read
#                     as Windows-1252;
#   noticeLines       the lines of the // block each library opens with;
#   colours           with --colours TABLE (lines of `R G B <index> "name"`):
#                     of the colour names the rules use - their own, their
#                     symbols' and their texts' - how many are standard names,
#                     how many the table gives a colour, which are plot pens or
#                     in neither, and which standard names the table gives a
#                     colour other than the standard one.
#
# The census is printed as JSON, keys sorted; --output FILE writes it there
# instead, with lines ended by a line feed on every platform.
import collections
import json
import os
import re
import sys
import xml.etree.ElementTree as ET


def read_text_and_guess(path):
    # The text, and how many of its characters rest on a guess: those outside
    # ASCII in a file that is neither UTF-8 nor marked as UTF-16.
    raw = open(path, 'rb').read()
    if raw[:2] in (b'\xff\xfe', b'\xfe\xff'):
        return raw.decode('utf-16'), 0
    if raw[:3] == b'\xef\xbb\xbf':
        return raw[3:].decode('utf-8'), 0
    try:
        return raw.decode('utf-8'), 0
    except UnicodeDecodeError:
        text = raw.decode('cp1252')
        return text, sum(1 for c in text if ord(c) > 127)


def read_text(path):
    return read_text_and_guess(path)[0]


def leading_comment_block(text):
    # The run of // lines a file opens with, blank lines above it passed over.
    lines = []
    for line in text.splitlines():
        line = line.strip(' \t')
        if not line.startswith('//'):
            if not line and not lines:
                continue
            break
        lines.append(line)
    return lines


# The 27 standard colour names of the Katana customisation format
# (docs/customisation.md, "Colour names"): the RGB the same names have in the
# CSS colour list, with its two stated departures - `green` is the primary,
# and the dark and light grey sit either side of grey.
STANDARD_COLOURS = {
    'red': (255, 0, 0), 'green': (0, 255, 0), 'blue': (0, 0, 255), 'yellow': (255, 255, 0),
    'cyan': (0, 255, 255), 'magenta': (255, 0, 255), 'white': (255, 255, 255),
    'black': (0, 0, 0), 'grey': (128, 128, 128), 'orange': (255, 165, 0),
    'brown': (165, 42, 42), 'purple': (128, 0, 128), 'pink': (255, 192, 203),
    'violet': (238, 130, 238), 'dark red': (139, 0, 0), 'dark green': (0, 100, 0),
    'dark blue': (0, 0, 139), 'dark cyan': (0, 139, 139), 'dark magenta': (139, 0, 139),
    'dark orange': (255, 140, 0), 'dark grey': (64, 64, 64), 'light grey': (192, 192, 192),
    'light blue': (173, 216, 230), 'light green': (144, 238, 144),
    'light cyan': (224, 255, 255), 'light yellow': (255, 255, 224),
    'light pink': (255, 182, 193),
}


def fold_colour(name):
    # The format's one fold: ASCII lower case, `_` and `-` as a blank, no
    # blanks at the ends, and `gray` as `grey` where it is the last word.
    folded = ''.join(chr(ord(c) + 32) if 'A' <= c <= 'Z' else c for c in name)
    folded = folded.replace('_', ' ').replace('-', ' ').strip(' ')
    if folded == 'gray' or folded.endswith(' gray'):
        folded = folded[:-4] + 'grey'
    return folded


def is_plot_pen(folded):
    # `pen`, digits and at most one letter: a pen of a plotter, not a colour.
    return re.fullmatch(r'pen *[0-9]+[a-z]?', folded) is not None


def read_colour_table(path):
    table = {}
    for line in read_text(path).splitlines():
        match = re.match(r'\s*(\d+)\s+(\d+)\s+(\d+)\s+-?\d+\s+"([^"]*)"', line)
        if match:
            table.setdefault(fold_colour(match.group(4)),
                             tuple(int(match.group(i)) for i in (1, 2, 3)))
    return table


def tokens(text):
    # Words and quoted strings, with // comments dropped. Braces are tokens.
    out = []
    i, n = 0, len(text)
    while i < n:
        c = text[i]
        if c.isspace():
            i += 1
        elif text.startswith('//', i):
            j = text.find('\n', i)
            i = n if j < 0 else j
        elif c == '"':
            j = text.find('"', i + 1)
            out.append(('s', text[i + 1:j]))
            i = j + 1
        elif c in '{}':
            out.append((c, c))
            i += 1
        else:
            j = i
            while j < n and not text[j].isspace() and text[j] not in '{}"':
                j += 1
            out.append(('w', text[i:j]))
            i = j
    return out


KINDS = {'paperstyle': 'paper', 'worldstyle': 'world', 'twoptstyle': 'twoPoint'}


def role_of(path):
    # What a file is - `linestyles`, `symbols`, `codes` or `names` - from what
    # it holds and, between two of one format, from its name (the head of this
    # script says why the name). The order on the command line says nothing
    # of it: that is the order the files are loaded in.
    text = read_text(path)
    name = os.path.basename(path).lower()
    if '<map_file' in text:
        return 'names' if 'names' in name else 'codes'
    if re.search(r'\b(?:%s)\s+"' % '|'.join(KINDS), text):
        return 'symbols' if 'symbol' in name else 'linestyles'
    raise SystemExit('%s: neither a style library nor a survey code file' % path)


# How many arguments each command takes, so a command's words are not mistaken
# for the next command.
ARGS = {'group': 1, 'mode': 1, 'length': 1, 'factor': 1, 'xorigin': 1, 'yorigin': 1,
        'xorigin1': 1, 'yorigin1': 1, 'xorigin2': 1, 'yorigin2': 1, 'stretch_mode': 1,
        'cycle_mode': 1, 'colour': 1, 'color': 1, 'move': 2, 'draw': 2, 'arc': 3,
        'circle': 1, 'dot': 1, 'text': 9}


def read_library(path):
    toks = tokens(read_text(path))
    blocks = []
    i = 0
    while i < len(toks):
        kind, value = toks[i]
        if kind == 'w' and value in KINDS and i + 2 < len(toks) and toks[i + 1][0] == 's' \
                and toks[i + 2][0] == '{':
            block = {'name': toks[i + 1][1], 'units': KINDS[value], 'group': '',
                     'atVertices': False, 'ops': collections.Counter(), 'pens': []}
            i += 3
            body = i
            while i < len(toks) and toks[i][0] != '}':
                word = toks[i][1]
                count = ARGS.get(word)
                if toks[i][0] != 'w' or count is None:
                    raise SystemExit('%s: unknown word %r in %r' % (path, word, block['name']))
                args = [t[1] for t in toks[i + 1:i + 1 + count]]
                if word == 'group':
                    block['group'] = args[0]
                elif word == 'mode':
                    block['atVertices'] = args[0] == 'vertex'
                elif word in ('colour', 'color'):
                    block['ops']['pen'] += 1
                    block['pens'].append(args[0])
                elif word in ('move', 'draw', 'arc', 'circle', 'dot', 'text'):
                    block['ops'][word] += 1
                i += 1 + count
            # What the block says, word for word: its kind and everything
            # between its braces. Two blocks with the same words are one
            # definition written twice.
            block['words'] = (value, tuple(toks[body:i]))
            blocks.append(block)
        i += 1
    return blocks


SECTIONS = ['map_data', 'vertex_symbol_data', 'vertex_symbol_data_v9', 'tinable_data',
            'vertex_textstyle_data', 'pipe_data', 'vertex_pipe_data', 'segment_pipe_data',
            'string_attribute_data', 'vertex_attribute_data']


def local(tag):
    return tag.rsplit('}', 1)[-1]


def read_rules(path):
    root = ET.fromstring(read_text(path).encode('utf-16'))
    map_file = next(e for e in root.iter() if local(e.tag) == 'map_file')
    rules = []
    keyless = 0
    for section in map_file:
        name = local(section.tag)
        if name not in SECTIONS:
            continue
        for item in section:
            if local(item.tag) != 'item':
                continue
            fields = {local(c.tag): c for c in item}
            key = (fields['key'].text or '').strip() if 'key' in fields else ''
            if not key:
                keyless += 1
                continue
            rule = {'key': key, 'section': name.replace('_v9', '')}
            for word in ('model', 'colour', 'linestyle', 'group', 'comment', 'breakline', 'weight'):
                if word in fields and (fields[word].text or '').strip():
                    rule[word] = fields[word].text.strip()
            if 'symbol_data' in fields:
                symbol = {local(c.tag): (c.text or '').strip() for c in fields['symbol_data']}
                if symbol.get('style'):
                    rule['symbol'] = symbol['style']
                    rule['symbolColour'] = symbol.get('colour', '')
            if 'textstyle_data' in fields:
                style = {local(c.tag): (c.text or '').strip() for c in fields['textstyle_data']}
                rule['textColour'] = style.get('colour', '')
            rules.append(rule)
    return rules, keyless


def strip_word(text, words):
    # The first word given that begins the text goes, with the blanks after
    # it; a text that is nothing but the word keeps it.
    for word in words:
        rest = text[len(word):]
        if text.startswith(word + ' ') and rest.strip(' '):
            return rest.lstrip(' ')
    return text


def begins_with_a_word(text, words):
    return any(text.startswith(word + ' ') for word in words)


def whole_word(text, word):
    return re.search(r'(?<![0-9A-Za-z_])' + re.escape(word) + r'(?![0-9A-Za-z_])', text) is not None


def main():
    args = sys.argv[1:]
    strip = []
    while '--strip' in args:
        at = args.index('--strip')
        strip.append(args[at + 1])
        del args[at:at + 2]
    remove = []
    while '--remove' in args:
        at = args.index('--remove')
        remove.append(args[at + 1])
        del args[at:at + 2]
    output = None
    if '--output' in args:
        at = args.index('--output')
        output = args[at + 1]
        del args[at:at + 2]
    colour_table = None
    if '--colours' in args:
        at = args.index('--colours')
        colour_table = args[at + 1]
        del args[at:at + 2]
    if len(args) != 4:
        raise SystemExit('usage: reference_census.py <file> <file> <file> <file> '
                         '[--strip WORD]... [--remove WORD]... [--colours TABLE] '
                         '[--output FILE]\n'
                         'the four files - a linestyle library, a symbol library, a survey '
                         'code file and a names file - in LOAD ORDER')
    # In the order given: the load order.
    files = [(role_of(path), path) for path in args]
    load_order = [role for role, _ in files]
    if sorted(load_order) != ['codes', 'linestyles', 'names', 'symbols']:
        raise SystemExit('reference_census.py: the four files are to be one linestyle library, '
                         'one symbol library, one survey code file and one names file; these '
                         'are, in the order given: %s' % ', '.join(load_order))
    path_of = {role: path for role, path in files}
    linestyles, codes, names, symbols = (path_of[role] for role in
                                         ('linestyles', 'codes', 'names', 'symbols'))

    library = {}
    origin = {}
    blocks = 0
    replaced = 0
    # Each replacement: the name, the file of the block that went, the file of
    # the block that took its place, and whether the two say the same words.
    replacements = []
    per_file_blocks = {}
    ops_read = collections.Counter()
    # The libraries in the order they were given, a later block taking the
    # place of an earlier one of its name.
    for label, path in files:
        if label not in ('linestyles', 'symbols'):
            continue
        read = read_library(path)
        per_file_blocks[label] = collections.Counter(b['units'] for b in read)
        for block in read:
            blocks += 1
            ops_read.update(block['ops'])
            read_as = (block['name'], block['group'])
            block['name'] = strip_word(block['name'], strip)
            block['group'] = strip_word(block['group'], strip)
            block['carried'] = (block['name'] != read_as[0], block['group'] != read_as[1])
            if block['name'] in library:
                replaced += 1
                replacements.append((block['name'], origin[block['name']], label,
                                     library[block['name']]['words'] == block['words']))
            library[block['name']] = block
            origin[block['name']] = label

    rules = []
    per_file_rules = {}
    keyless = {}
    # ... and the survey code files in the order THEY were given, the rules of
    # the earlier one first.
    for label, path in files:
        if label not in ('codes', 'names'):
            continue
        read, skipped = read_rules(path)
        per_file_rules[label] = len(read)
        keyless[label] = skipped
        for rule in read:
            rule['carried'] = collections.Counter()
            for word in ('linestyle', 'symbol', 'group'):
                if word in rule:
                    read_as = rule[word]
                    rule[word] = strip_word(rule[word], strip)
                    rule['carried'][word] += rule[word] != read_as
            rules.append(rule)

    referenced_linestyles = sorted({r['linestyle'] for r in rules if r.get('linestyle')})
    referenced_symbols = sorted({r['symbol'] for r in rules if r.get('symbol')})
    referenced = sorted(set(referenced_linestyles) | set(referenced_symbols))
    colours = collections.Counter()
    for rule in rules:
        for word in ('colour', 'symbolColour'):
            if rule.get(word):
                colours[rule[word]] += 1
    pens = collections.Counter(p for b in library.values() for p in b['pens'])
    final_ops = collections.Counter()
    for block in library.values():
        final_ops.update(block['ops'])

    def first_match(code, word):
        # Katana's rule, restated: exact key first, then prefixes longest first,
        # then the bare '*'; among equals, the earlier rule; the first that says it.
        ranked = []
        for index, rule in enumerate(rules):
            key = rule['key']
            if key == code:
                ranked.append((0, 0, index, rule))
            elif key.endswith('*') and code.startswith(key[:-1]):
                ranked.append((1, -len(key), index, rule))
        for _, _, _, rule in sorted(ranked, key=lambda r: r[:3]):
            if rule.get(word):
                return rule[word]
        return None

    # What still begins with a word that was to be stripped: every name, group
    # path and reference, after the stripping. 0 unless a word stood twice.
    still = sum(1 for b in library.values() for text in (b['name'], b['group'])
                if begins_with_a_word(text, strip))
    still += sum(1 for r in rules for word in ('linestyle', 'symbol', 'group')
                 if begins_with_a_word(r.get(word, ''), strip))

    census = {
        'loadOrder': load_order,
        'blocksRead': blocks,
        'blocksPerFile': {k: dict(v) for k, v in per_file_blocks.items()},
        'blocksReplaced': replaced,
        'definitions': len(library),
        'fromSymbolsFile': sum(1 for v in origin.values() if v == 'symbols'),
        'fromLinestylesFile': sum(1 for v in origin.values() if v == 'linestyles'),
        'atVertices': sum(1 for b in library.values() if b['atVertices']),
        'units': dict(collections.Counter(b['units'] for b in library.values())),
        'groups': len({b['group'] for b in library.values() if b['group']}),
        'groupFirstWords': dict(collections.Counter(
            b['group'].split(' ')[0] for b in library.values() if b['group']).most_common(6)),
        'opsRead': dict(ops_read),
        'opsInLibrary': dict(final_ops),
        'penNames': dict(pens),
        'rules': len(rules),
        'rulesPerFile': per_file_rules,
        'keylessItems': keyless,
        'rulesPerSection': dict(collections.Counter(r['section'] for r in rules)),
        'distinctKeys': len({r['key'] for r in rules}),
        'referencedLinestyles': len(referenced_linestyles),
        'referencedSymbols': len(referenced_symbols),
        'referenced': len(referenced),
        'unresolved': [n for n in referenced if n not in library],
        # Rules that name a definition of the OTHER library's kind: as their
        # linestyle one the symbol library gave, as their symbol one the
        # linestyle library gave. It is counted over every name the rules
        # use, not only those both libraries define, and is what the load
        # order decides for those: a line drawn with a symbol's strokes.
        'rulesNamingTheOtherKind': {
            'linestyleFromSymbolsFile': {
                'rules': sum(1 for r in rules if origin.get(r.get('linestyle')) == 'symbols'),
                'names': len({r['linestyle'] for r in rules
                              if origin.get(r.get('linestyle')) == 'symbols'})},
            'symbolFromLinestylesFile': {
                'rules': sum(1 for r in rules if origin.get(r.get('symbol')) == 'linestyles'),
                'names': len({r['symbol'] for r in rules
                              if origin.get(r.get('symbol')) == 'linestyles'})},
        },
        'colourNames': dict(sorted(colours.items())),
        'namesBeginningWithAStrippedWord': still,
        'WM01': {w: first_match('WM01', w) for w in ('model', 'linestyle', 'breakline', 'colour')},
        'AC01': {w: first_match('AC01', w) for w in ('model', 'symbol')},
        'AC01symbolAtVertices': None,
    }
    symbol = census['AC01']['symbol']
    if symbol in library:
        census['AC01symbolAtVertices'] = library[symbol]['atVertices']
    # What a conversion reports beyond the counts above.
    across = {name for name, was, now, _ in replacements if was != now}
    # What the ORDER made of those: `origin` is the library whose block each
    # name has now, the one given later. A rule that names one as the kind of
    # the OTHER library - as its linestyle where the symbol library's block
    # was kept, as its symbol where the linestyle library's was - names a
    # definition it was not written for.
    kept_as_symbol = {name for name in across if origin[name] == 'symbols'}
    kept_as_linestyle = across - kept_as_symbol
    astray = ([r['linestyle'] for r in rules if r.get('linestyle') in kept_as_symbol] +
              [r['symbol'] for r in rules if r.get('symbol') in kept_as_linestyle])
    census['replaced'] = {
        'withinAFile': sum(1 for _, was, now, _ in replacements if was == now),
        'acrossFiles': sum(1 for _, was, now, _ in replacements if was != now),
        'sameWords': sum(1 for _, _, _, same in replacements if same),
        'acrossFilesNamedAsLinestyle': {
            'rules': sum(1 for r in rules if r.get('linestyle') in across),
            'names': len({r['linestyle'] for r in rules if r.get('linestyle') in across})},
        'acrossFilesNamedAsSymbol': {
            'rules': sum(1 for r in rules if r.get('symbol') in across),
            'names': len({r['symbol'] for r in rules if r.get('symbol') in across})},
        'acrossFilesKept': {'linestyles': len(kept_as_linestyle),
                            'symbols': len(kept_as_symbol)},
        'acrossFilesNamedAsTheKindThatWent': {'rules': len(astray), 'names': len(set(astray))},
    }
    census['inferredEncoding'] = {
        label: read_text_and_guess(path)[1]
        for label, path in (('linestyles', linestyles), ('codes', codes), ('names', names),
                            ('symbols', symbols))}
    notices = {label: leading_comment_block(read_text(path))
               for label, path in (('linestyles', linestyles), ('symbols', symbols))}
    census['noticeLines'] = {'linestyles': len(notices['linestyles']),
                             'symbols': len(notices['symbols']),
                             'same': notices['linestyles'] == notices['symbols']}
    if colour_table is not None:
        table = read_colour_table(colour_table)
        used = {}
        for rule in rules:
            for word in ('colour', 'symbolColour', 'textColour'):
                if rule.get(word):
                    used.setdefault(fold_colour(rule[word]), rule[word])
        standard = sorted(f for f in used if f in STANDARD_COLOURS)
        others = sorted(f for f in used if f not in STANDARD_COLOURS)
        census['colours'] = {
            'used': len(used),
            'standard': len(standard),
            'fromTable': sum(1 for f in others if not is_plot_pen(f) and f in table),
            'plotPens': [used[f] for f in others if is_plot_pen(f)],
            'inNeither': [used[f] for f in others if not is_plot_pen(f) and f not in table],
            'standardWithAnotherColour': [
                used[f] for f in standard if f in table and table[f] != STANDARD_COLOURS[f]],
            'inferredEncoding': read_text_and_guess(colour_table)[1],
        }
    # What held a word that was stripped or is to be removed. The definitions
    # are those that survive, as everywhere above: a block another replaced
    # is not counted.
    if strip or remove:
        census['carried'] = {
            'definitionNames': sum(1 for b in library.values() if b['carried'][0]),
            'groupPaths': sum(1 for b in library.values() if b['carried'][1]),
            'ruleGroups': sum(r['carried']['group'] for r in rules),
            'ruleReferences': sum(r['carried']['linestyle'] + r['carried']['symbol']
                                  for r in rules),
            'ruleComments': sum(1 for r in rules
                                if any(whole_word(r.get('comment', ''), w) for w in remove)),
        }
    text = json.dumps(census, indent=1, sort_keys=True) + '\n'
    if output is None:
        # Line feeds on every platform, so a redirected census is the same file.
        sys.stdout.reconfigure(newline='\n')
        sys.stdout.write(text)
    else:
        with open(output, 'w', encoding='utf-8', newline='\n') as out:
            out.write(text)


if __name__ == '__main__':
    main()
