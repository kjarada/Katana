# tools/customisation_census.py <customisation.json> [--strip WORD]...
#                               [--against <census.json>] [--output FILE]
#
# A census of a Katana customisation file (docs/customisation.md), using only
# the standard `json` module and none of Katana's code. It counts the same
# things tools/reference_census.py counts in the legacy files a customisation
# was converted from, under the same names, so that the two can be compared:
# a converted file that holds what its sources held gives the same figures.
#
# --against <census.json> compares with a census that script wrote, figure by
# figure, and exits 1 if any differs. Figures that describe the legacy FILES
# rather than what they hold (the order they were given in, blocks read and
# replaced, strokes read, rules a file, items without a key, what carried a
# stripped word, how a file was decoded, its notice, what its colour table
# says) have no counterpart in a customisation; they are listed and not
# compared. The order still decides what is compared: that script counts the
# definitions the order it was given keeps, so a customisation agrees only
# with the census of the order it was converted in.
#
# --strip WORD (repeatable) names the leading words the conversion was to
# take off: the census then counts the names, group paths and rule references
# that still begin with one, which should be none.
#
# How the two vocabularies meet, since the format renamed what the files call
# things (the format chapter has the whole table):
#   a definition in "symbols" / "linestyles"   came from the symbol / linestyle library
#   "sets": feature, symbol, surface, ...       the section map_data, vertex_symbol_data, ...
#   "layer", "draw": "line"                     <model>, <breakline>Line</breakline>
import collections
import json
import sys

# The section of a survey code file each `sets` word was converted from.
SECTION_OF = {
    'feature': 'map_data',
    'symbol': 'vertex_symbol_data',
    'surface': 'tinable_data',
    'text': 'vertex_textstyle_data',
    'pipe': 'pipe_data',
    'vertexPipe': 'vertex_pipe_data',
    'segmentPipe': 'segment_pipe_data',
    'attributes': 'string_attribute_data',
    'vertexAttributes': 'vertex_attribute_data',
}

# Figures of the reference census that are about the legacy files themselves.
NOT_IN_A_CUSTOMISATION = ('loadOrder', 'blocksRead', 'blocksPerFile', 'blocksReplaced',
                          'opsRead', 'rulesPerFile', 'keylessItems', 'carried', 'replaced',
                          'inferredEncoding', 'noticeLines', 'colours')


def said(rule, word):
    # What a rule says of `word`, by the reference census's names for the fields.
    if word == 'model':
        return rule.get('layer')
    if word == 'breakline':
        return rule['draw'].capitalize() if 'draw' in rule else None
    if word == 'symbol':
        return rule.get('symbol', {}).get('name')
    return rule.get(word)


def census_of(document, strip):
    linestyles = document.get('linestyles', [])
    symbols = document.get('symbols', [])
    definitions = linestyles + symbols
    by_name = {d['name']: d for d in definitions}
    if len(by_name) != len(definitions):
        raise SystemExit('a definition name is given twice')
    linestyle_names = {d['name'] for d in linestyles}
    symbol_names = {d['name'] for d in symbols}
    rules = document.get('codes', [])

    ops = collections.Counter()
    pens = collections.Counter()
    for definition in definitions:
        for stroke in definition.get('strokes', []):
            ops[stroke[0]] += 1
            if stroke[0] == 'pen':
                pens[stroke[1]] += 1

    referenced_linestyles = sorted({r['linestyle'] for r in rules if r.get('linestyle')})
    referenced_symbols = sorted({said(r, 'symbol') for r in rules if said(r, 'symbol')})
    referenced = sorted(set(referenced_linestyles) | set(referenced_symbols))
    # A rule's own colour and its symbol's; a text's colour is not in this
    # figure, as it is not in the reference census's.
    colours = collections.Counter()
    for rule in rules:
        for colour in (rule.get('colour'), rule.get('symbol', {}).get('colour')):
            if colour:
                colours[colour] += 1

    def first_match(code, word):
        # Katana's rule, restated: exact key first, then prefixes longest first,
        # then the bare '*'; among equals, the earlier rule; the first that says it.
        ranked = []
        for index, rule in enumerate(rules):
            key = rule['key']
            if key == code:
                ranked.append((0, 0, index))
            elif key.endswith('*') and code.startswith(key[:-1]):
                ranked.append((1, -len(key), index))
        for _, _, index in sorted(ranked):
            if said(rules[index], word):
                return said(rules[index], word)
        return None

    def begins_with_a_word(text):
        return any(text.startswith(word + ' ') for word in strip)

    still = sum(1 for d in definitions for text in (d['name'], d.get('group', ''))
                if begins_with_a_word(text))
    still += sum(1 for r in rules
                 for text in (r.get('linestyle', ''), said(r, 'symbol') or '', r.get('group', ''))
                 if begins_with_a_word(text))

    first_words = collections.Counter(
        d['group'].split(' ')[0] for d in definitions if d.get('group'))
    census = {
        'definitions': len(definitions),
        'fromSymbolsFile': len(symbols),
        'fromLinestylesFile': len(linestyles),
        'atVertices': sum(1 for d in definitions if d.get('atVertices', False)),
        'units': dict(collections.Counter(d.get('units', 'world') for d in definitions)),
        'groups': len({d['group'] for d in definitions if d.get('group')}),
        # The six commonest, a tie settled by name so that the census of one
        # file is always the same text.
        'groupFirstWords': dict(sorted(first_words.items(), key=lambda e: (-e[1], e[0]))[:6]),
        'opsInLibrary': dict(ops),
        'penNames': dict(pens),
        'rules': len(rules),
        'rulesPerSection': dict(collections.Counter(SECTION_OF[r['sets']] for r in rules)),
        'distinctKeys': len({r['key'] for r in rules}),
        'referencedLinestyles': len(referenced_linestyles),
        'referencedSymbols': len(referenced_symbols),
        'referenced': len(referenced),
        'unresolved': [n for n in referenced if n not in by_name],
        # Rules that name a definition of the other list: as their linestyle
        # one listed in "symbols", as their symbol one listed in "linestyles".
        'rulesNamingTheOtherKind': {
            'linestyleFromSymbolsFile': {
                'rules': sum(1 for r in rules if r.get('linestyle') in symbol_names),
                'names': len({r['linestyle'] for r in rules
                              if r.get('linestyle') in symbol_names})},
            'symbolFromLinestylesFile': {
                'rules': sum(1 for r in rules if said(r, 'symbol') in linestyle_names),
                'names': len({said(r, 'symbol') for r in rules
                              if said(r, 'symbol') in linestyle_names})},
        },
        'colourNames': dict(sorted(colours.items())),
        'namesBeginningWithAStrippedWord': still,
        'WM01': {w: first_match('WM01', w) for w in ('model', 'linestyle', 'breakline', 'colour')},
        'AC01': {w: first_match('AC01', w) for w in ('model', 'symbol')},
        'AC01symbolAtVertices': None,
    }
    symbol = census['AC01']['symbol']
    if symbol in by_name:
        census['AC01symbolAtVertices'] = by_name[symbol].get('atVertices', False)
    return census, first_words


def compare(census, first_words, reference):
    # One line a figure; the number that differ.
    different = 0
    for key in sorted(reference):
        if key in NOT_IN_A_CUSTOMISATION:
            print('not in a customisation  %s' % key)
            continue
        if key not in census:
            print('DIFFERS  %s: the reference census has it and this one does not' % key)
            different += 1
            continue
        here, there = census[key], reference[key]
        if key == 'groupFirstWords':
            # The reference lists its six commonest, and which of several
            # equally common it lists is an accident of the order it met
            # them in: each one it lists is looked up among ALL of these.
            here = {word: first_words.get(word, 0) for word in there}
        if key == 'WM01' and here.get('breakline') and there.get('breakline'):
            # The files write "Line" and "line" alike; the format has one word.
            here = dict(here, breakline=here['breakline'].lower())
            there = dict(there, breakline=there['breakline'].lower())
        if here == there:
            print('same     %s' % key)
        else:
            print('DIFFERS  %s: %s here, %s in the reference census'
                  % (key, json.dumps(here, sort_keys=True), json.dumps(there, sort_keys=True)))
            different += 1
    for key in sorted(set(census) - set(reference)):
        print('DIFFERS  %s: this census has it and the reference one does not' % key)
        different += 1
    print('different=%d' % different)
    return different


def main():
    args = sys.argv[1:]
    strip = []
    while '--strip' in args:
        at = args.index('--strip')
        strip.append(args[at + 1])
        del args[at:at + 2]
    options = {}
    for option in ('--against', '--output'):
        if option in args:
            at = args.index(option)
            options[option] = args[at + 1]
            del args[at:at + 2]
    if len(args) != 1:
        raise SystemExit('usage: customisation_census.py <customisation.json> [--strip WORD]... '
                         '[--against <census.json>] [--output FILE]')
    try:
        with open(args[0], encoding='utf-8-sig') as file:
            document = json.load(file)
    except ValueError:  # not UTF-8, or not JSON: a legacy file, most likely
        document = None
    if not isinstance(document, dict) or document.get('format') != 'katana-customisation':
        raise SystemExit('%s is not a Katana customisation file' % args[0])
    census, first_words = census_of(document, strip)

    sys.stdout.reconfigure(newline='\n')
    text = json.dumps(census, indent=1, sort_keys=True) + '\n'
    if '--output' in options:
        with open(options['--output'], 'w', encoding='utf-8', newline='\n') as out:
            out.write(text)
    elif '--against' not in options:
        sys.stdout.write(text)
    if '--against' in options:
        with open(options['--against'], encoding='utf-8') as file:
            reference = json.load(file)
        if compare(census, first_words, reference) != 0:
            raise SystemExit(1)


if __name__ == '__main__':
    main()
