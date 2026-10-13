# Katana Standard

Katana Standard is a survey feature library written for Katana: one Katana customisation file, `resources/customisation/katana-standard.customisation.json`, with 98 linestyles, 123 symbols and 632 survey codes (1103 rules), a palette of 28 colours and 161 layers. Its survey code **keys** are the NSW customisation's, byte for byte, and each key has the same kind of rules there as here, so a field file coded for one is read by the other and makes the same kinds of entity in both. That is the owner's decision, and it is all that is shared: every linestyle and symbol, every layer path, colour, pen weight, text style and legend label is Katana's own, designed and written for it ("What is shared and what is not", below). This document is its catalogue: how to load it, how to read a code, every definition and every code, and what Katana does and does not yet do with what the file says.

Everything below that is a table, a count or a description is generated from the data by `tools/katana_standard/make_katana_standard.py --docs`, so it cannot disagree with the file; the prose around it is written by hand in `tools/katana_standard/catalogue.py`. "How it is made and held" says what keeps the two true.

![An invented street corner drawn entirely by Katana Standard, in Katana's plan view](images/katana-standard-showcase.png)

*An invented street corner, drawn in Katana's plan view with nothing but the library: 192 of its 632 codes are in it, kerbs and lots, houses, fences, trees, the services under and over the road, a stretch of rail on a bridge, contours and a creek, control marks and the labels. Every mark is the size it is on paper and sits where its code put it (`tools/katana_standard/showcase.py`). Katana does not yet apply a code's pen weight, so every line is drawn at one width here and the weight ladder of "Pen weights" is not shown.*

**Contents.** [At a glance](#at-a-glance) · [Using it](#using-it) · [Reading a code](#reading-a-code) · [What is shared and what is not](#what-is-shared-and-what-is-not) · [The palette](#the-palette) · [Linestyles](#linestyles) · [Symbols](#symbols) · [The codes](#the-codes) · [Alphabetical index](#the-codes-in-alphabetical-order) · [Decisions](#decisions-and-what-was-rejected) · [How it is made and held](#how-it-is-made-and-held) · [Not done](#not-done)

## At a glance

|  |  |
|---|---|
| Name | `Katana Standard`, edition 1, Katana customisation format version 1 |
| Definitions | 221: 98 linestyles and 123 symbols, in 48 groups, all in paper units, so a mark keeps its size at any zoom and plot scale |
| Survey codes | 632 keys: 422 prefix keys that end in `*`, 209 exact keys and the bare `*`; in 13 classes and 57 groups; 1103 rules |
| The keys | the other built-in's, byte for byte; the rule kinds, draw modes, text and pipe rules and surface flags of each key follow the other built-in's too ("What is shared and what is not") |
| Layers | 161, always three levels (`class/group/family`), lower case |
| Palette | 28 named colours, each at least 3:1 against the plan view's ground and against white paper as it prints (yellow gas and cream sewer, which are AS 5488's, print light); the service colours are AS 5488's and `UTILITY DRAW`'s |
| Says nothing of | linework control words and import automation: loading it never resets a colleague's spellings or switches |
| Notice | Katana Standard is original work made for Katana. Its survey code keys follow those of the other built-in library, reused for field-data compatibility on the owner's decision; everything else in it is Katana's own. Its licence is that of the Katana repository. |

## Using it

What is true now. Katana Standard **is** the built-in (the owner's ruling, 2026-10-08): a program starts with it, and the NSW customisation is compiled in beside it as an alternate that Settings (Built-in library, then Use) and `CUSTOMISE BUILTIN NSW` choose. It also loads like any customisation file (`docs/customisation.md`, "Using the customisation").

1. **Load it alone.** `CUSTOMISE REPLACE resources/customisation/katana-standard.customisation.json` in the window's command line, in `katana_cli` or through `katana_mcp`; or File > Settings > Import with "Replace instead of merging" ticked. Its definitions become the whole library and its rules the whole of the codes. It brings no linework spellings and no import switches, so the session keeps its own.
2. **Merging works, with one caution.** `CUSTOMISE <file>`, or Import without the tick, merges by name. The most specific rule wins field by field, so a broader prefix rule already in the session can still supply a field these rules leave unsaid (a symbol, a text). Merged onto the NSW customisation it shares every key with it, so the result is neither library but a field-by-field mixture of the two: load it alone when you want its library and nothing else.
3. **Start with it, or go back to it.** It is what a program starts with when nothing else is said. After another built-in was chosen, `CUSTOMISE BUILTIN "Katana Standard"` or Settings > Built-in library > Use puts it back, and `CUSTOMISE RESET` does too unless the run was started with another built-in as its own: `KATANA_BUILTIN_CUSTOMISATION=NSW` starts with the other, and RESET then gives that one (`docs/headless.md`, "The customisation a run starts with"). `KATANA_BUILTIN_CUSTOMISATION=none` starts with no built-in.
4. **Compiled in**: `-DKATANA_BUILTIN_CUSTOMISATION=<file>` at configure time embeds a different default, and `-DKATANA_ALTERNATE_CUSTOMISATIONS=<files>` further built-ins (`docs/building.md`, "The built-in customisation and the reference folder"). A build directory configured before this became the default that still has the earlier default is moved to this one by its next configure, once, with a line saying so.
5. **Then work as usual.** `SURVEY IMPORT <file>` or Survey > Import Survey Points codes and strings the points by these rules in one undo step; `CODE`, `LINEWORK`, `CODE EXPLAIN <code>`, `CODE LIST` and `CODE CHECK` read them. `CODE CHECK` reports no problems on the file: 1103 rules checked, no error and no warning.

A worked example, run for this document and held by `cli.katana_standard_imports_a_field_file_and_reports_the_mistyped_code`. The invented field file `tests/data/katana_standard/street_corner.fld` has twenty entered coordinates: a lot boundary coded `BT` in string 1 and closed by opcode 20, a kerb `KB`, a fence `FE`, a water main `WM` and a buried power line `EU`, four single points (an isolating valve `PWSV`, a tree trunk `PT04`, a gully pit `PGUL` and a bench mark `PBMK`) and one mistyped code, `PWSW`. With the library loaded by `CUSTOMISE REPLACE`, `SURVEY IMPORT` draws 20 points and 5 lines: 19 points are coded onto 9 layers in 9 styles, and the 20th, `PWSW`, is reported as a code no rule answers, since only the bare `*` fits it and that rule names no layer, no linestyle and no symbol. The lot boundary is a closed line of 100 m round, 600 square metres; the kerb and the water main are 24 m, the fence 10 m and the power line 20 m. Worked out by hand from the codes below before it was run.

![The street corner field file after SURVEY IMPORT, in the plan view](images/katana-standard-plan.png)

*After the import, from the top: the closed lot boundary with its bench mark and its tree trunk; the wire fence beside it; the kerb with a gully pit; the water main with its valve; the buried power line. The plain cross inside the lot is `PWSW`, the point no rule answers. Pen weights are not yet applied, so the boundary is no heavier than the fence. The words beside each feature are laid on the screenshot afterwards, from the library's own legend labels.*

## Reading a code

A code is a **key**, and the keys are the NSW customisation's: `AC*` is a bollard, `PT04` a tree trunk 400 mm across, `WM*` a water main. In the field a point's code is the key and a string number, then control words: `WM1 ST`, `WM1`, `WM1 END` is one water main. The grammar, in full:

1. THE KEYS ARE ANOTHER LIBRARY'S, AND NOTHING ELSE OF THEM IS. Katana has two built-in customisations, this one and the NSW one (resources/customisation/nsw.customisation.json). By the owner's decision they use the SAME survey code keys, byte for byte, so that a field file coded for either draws in both. The keys are all that is taken: every layer, colour, weight, linestyle, symbol, text style, group and legend label, and every sentence of the meanings and of this document, is Katana's own. No key is ours: the keys are the other library's identifiers, reused for field-data compatibility, and the licence notes say so (LICENSING.md).
2. A key is an exact code or a prefix. `PBMK` matches the code PBMK and no other; `AC*` matches every code that begins AC (AC, AC1, AC01, ACE); the bare `*` matches every code. A code meets every key that fits it, and for each field the most specific rule that says anything about it wins: an exact key before a prefix, a longer prefix before a shorter, `*` last (`SurveyMap::lookup`). So the keys `PT04` and `PT04*` are two parts of one code: the first gives a trunk its symbol and the second keeps it out of the ground surface; and `Z0*` is more specific than `Z*`. A key is matched as written, in capitals: `ac1` matches no rule (`CODE EXPLAIN ac1` says the key `AC*` differs only in letter case), so set the data collector to send capitals.
3. Keys are capital letters and digits. A digit that is part of a key belongs to it: `U1*` is a 225 mm stormwater pipe and `PT04` a 400 mm trunk. The STRING NUMBER follows the key, and the linework reads it: `WM1` and `WM2` are two water mains, both answered by `WM*`. The keys `1`, `2`, `3`, `4`, `5`, `7` and `8` are text sizes (1.5, 2.5, 3.5, 5, 7 and 10 mm of paper, and the parcel label's 2.5), and `1*` to `8*` give that text its layer.
4. Control words follow the code, after a space: `ST` first point, `END` last point, `CL` last point and close, `BC` and `EC` begin and end a curve (three points fix an arc), `JPN n` also join to point number n, `RECT` the third point of a rectangle. These are Katana's default spellings; the library says nothing about them, so loading it never resets a colleague's own. The first token of a point's code is always the key, so a point coded `CL` is a string called CL, answered by the key `CL*`, even though four keys (`BC*`, `CL*`, `EC*` and `ST*`) are spelled like a default control word: a control word is read only after the key.
5. There is no left or right suffix: the rule engine matches a key exactly or by prefix and has nothing else. A handed pair is two keys (`CV*` and `CY*` chevrons, `DA*` and `DC*` dragons teeth, `LH*` and `LR*` broken barrier lines, `HE*` and `HR*` wall hatching). Otherwise handedness is by walking direction: ticks, barbs, triangles, scallops and chevrons fall to the LEFT of travel. Symbols are drawn upright and are not turned to a bearing (Katana reads a rule's rotation and does not yet apply it), so a symbol's position is its message.
6. The STRUCTURE of every key is the other library's: the same ordered kinds of rule (feature, symbol, text, pipe, vertexPipe, segmentPipe, attributes, vertexAttributes, surface), the same draw mode (line or point), the same hide and surface flags, text rules where it has text rules and pipe rules where it has pipe rules, so that one field file makes the same kinds of entity (points, lines, text, pipes, surface members) in either library. `tools/katana_standard/parity.py` compares the two files and the tests hold it. What a key LOOKS like is Katana's.
7. One clean rule set per key. The other library repeats rules and shadows them, names pen colours that resolve to nothing, and draws a symbol through a linestyle slot; this one writes one rule of each kind per key, resolves every name, and says a symbol in a symbol rule. Where the two differ for that reason the key is listed in `parity_exceptions.json` with the reason, and `parity.py` fails the day an entry explains nothing.
8. What a code sets, in this order. `feature`: layer, colour, draw (line or point, none for a layer-only key), linestyle (lines only), weight, group and comment. `symbol` for points and for lines that carry a symbol at every vertex. `text` for the text keys. `pipe`, `vertexPipe` and `segmentPipe` for the bare `*`. `attributes` and `vertexAttributes`. `surface`, written where the other library writes it: false for what must stay out of a ground surface, true where the code says it belongs in one. Layers are three levels, lower case (`class/group/family`), and a family is what one switches on and off together.
9. Attributes. An attribute with an EMPTY value is a PROMPT: the code asks for it. One with a value is applied. Names carry their unit (`Depth (m)`, `Diameter (mm)`). The other library's attribute rules are kept as they are (a model zone to check, a point code), and one more is added to a code that stands for a buried or strung service, last: `utility.type` set to the service word, then the prompts Owner, Depth (m) (Height (m) for a line strung overhead) and Condition; a pipe also Material and Diameter (mm), a box culvert Width (mm) and Height (mm). Where the code states the size (a 300 mm pipe, a culvert 1200 mm high) the size is applied. Those are the words Katana's utility tools read, so a coded string is already typed for UTILITY DRAW. Stormwater pits and pipes are services too, though they are catalogued with drainage. The prompts use the library's own names, not `utility.owner`, so an empty prompt never reads as "recorded". The added rule makes no entity and `parity.py` counts it apart.
10. `comment` is the LEGEND LABEL: a noun phrase in sentence case, 1 to 36 characters, which the plot legend prints in capitals. When several codes share one linestyle the legend takes the linestyle's name instead, unless every coded entity agrees on the label. The longer text is `meaning`.
11. Weights, in millimetres of pen: 0.13 hairline, 0.18 fine, 0.25 light, 0.35 medium, 0.50 strong (0.70 is reserved). The hairline is for hatching, joints, hidden detail, tree outlines, text and design strings; nothing in the planting or ground classes is lighter than 0.18, since those colours are the faintest on paper. Katana stores `weight` and does not yet apply it; the scale is still the documented intent.
12. What Katana applies today and what it only stores. `CODE` applies layer, colour, linestyle, symbol name and size, and the string attributes (a value starting with `$` is deferred, not written), and the legend prints the comment. It stores, lists and lints, and does not yet apply: weight, group, hide, surface, text rules, pipe rules (justify, shape, sizes taken from the attributes), rotation, offset and raise. The rules are written for the day it does.
13. Load the library alone, or with Replace. Merged onto another customisation, a broader prefix rule there can supply a field these rules leave unsaid (a symbol, a text), because the most specific rule wins field by field. Merged onto the other library it shares every key, so each key's rules are replaced or added to field by field and the result is neither.
14. The classes. The 13 classes and their groups are this library's own way of arranging the keys for a reader and for the layer tree: a class letter is a label in the catalogue and is not part of any key. The keys that the other library labels as design strings (road, lane, earthworks, kerb, bridge, drainage and barrier design, master strings) are class D, Design and Setting Out; the bare `*` belongs to no class.

### The thirteen classes

The classes and groups arrange the keys for a reader and fix the first two levels of a layer. A class letter is a label for this table and for the picture scripts, and no key is built from one.

| Letter | Class | Codes | Groups | What it holds |
|---|---|---|---|---|
| `C` | Boundaries and Cadastre | 30 | Lot Boundaries, Easements and Reserves, Boundary Marks | Where one owner's land ends and the next begins, and the marks that prove it. |
| `R` | Roads and Pavements | 36 | Carriageway, Markings, Paths and Access | The made surface: its edges, its paint and the paths beside it. |
| `S` | Street Furniture and Signs | 36 | Signs, Lighting, Posts and Barriers, Amenities, Traffic Control | What people read, sit on, light their way by or steer round. |
| `K` | Kerbs and Drainage | 82 | Kerbs, Pits and Structures, Pipes and Culverts, Channels and Waterways | Where the street meets the water: kerbs, pits, pipes and channels. |
| `T` | Rail and Transit | 13 | Track, Overhead and Signals, Platforms | Track, the lineside fittings and the platforms beside it. |
| `U` | Utilities | 118 | Water, Sewer, Gas, Electricity, Communications, Recycled Water, Fire Service, Fuel, Traffic Systems, Unknown Services | Everything buried or strung that carries a service, one family per service. |
| `V` | Vegetation and Landscape | 58 | Trees, Ground Cover | Trees and the edges of planted and natural ground cover. |
| `B` | Buildings and Structures | 35 | Buildings, Structures, Steps and Access | What stands on the ground: buildings, bridges, masts, tanks and steps. |
| `F` | Fences and Walls | 8 | Fences, Walls, Gates and Posts | What divides land: fences, walls, and the posts and gates between. |
| `G` | Terrain and Breaklines | 25 | Ground Points, Breaklines, Contours, Water Edges, Rock and Cliff | The shape of the ground: shots, breaklines, contours, water edges and rock. |
| `M` | Survey Control and Annotation | 54 | Control Marks, Investigation Points, Text and Notes, Plan Marks | The survey's own marks, its investigations and the words on the plan. |
| `X` | Miscellaneous | 21 | Hazards and Heritage, General, Land Use Areas | Hazards, heritage, land use and the catch-alls. |
| `D` | Design and Setting Out | 115 | Road Design Strings, Lane Marking Design, Earthworks Design, Kerb and Carriageway Design, Bridge Design, Drainage Design, Barrier and Fence Design, Master Strings, Design General | Strings that carry a design, not a survey: set out from them, do not read the ground from them. |

The bare `*` belongs to no class. What each kind of code draws, by count:

| Kind | Codes | Meaning |
|---|---|---|
| line | 359 | a line, joined from the points |
| point | 205 | a point, with a symbol |
| both | 1 | a line with a symbol at every vertex |
| text | 7 | text of a size |
| layer | 7 | the layer of the text codes |
| surface | 52 | a surface flag and nothing else |
| pipe | 1 | the round pipe section of any string that carries a diameter |

### Pen weights

Every code names one of five weights, in millimetres of pen; a sixth is reserved. Katana stores a rule's `weight` and does not yet apply it to what it draws, so the scale is the documented intent, for the day it does.

| Weight | Role | For |
|---|---|---|
| 0.13 | hairline | hatching, joints, hidden detail, tree outlines, design strings, text |
| 0.18 | fine | vegetation, ground shots and levels, intermediate contours, markings, minor services |
| 0.25 | light | everyday detail: fences, paths, branches, structures |
| 0.35 | medium | primary detail: kerbs, building walls, service mains, index contours |
| 0.50 | strong | title boundary, bridge decks, limit of survey |
| 0.70 | bold | reserved; not used by a code in edition 1 |

**What Katana applies today.** `CODE` and a survey import apply layer, colour, linestyle, symbol name and the string attributes, and the plot legend prints the comment. Katana stores, lists and lints, and does not yet apply: `weight`, `group`, `hide`, `surface`, text rules, pipe rules, and a symbol's rotation, offset and raise. So the four hidden symbols still draw, the seven text codes draw a plus until text rules are applied, and the plain continuous lines of one colour on one layer, such as the kerb top `KB*` and the kerb edge of a median `TM*`, look the same on the plan until the weight is applied. The rules are written for the day it does.

## What is shared and what is not

**Shared, by the owner's decision: the survey code keys, and what each key does.** Katana Standard has the same 632 keys as the NSW customisation, including every prefix rule ending in `*`, the numbered families and the bare `*`; and for each key the same kinds of rule in the same order, the same draw mode (line or point), the same hide and surface flags, text rules where the other has them and pipe rules where the other has them. A field file therefore makes the same kinds of entity in either. The keys are the other library's identifiers, reused for field-data compatibility; they are the only thing taken (`LICENSING.md`). `tools/katana_standard/parity.py` compares the two files and the tests hold the result.

**Katana's own: everything you can see.** Linestyles and symbols (every one drawn here, from a written brief and looked at in Katana's own renderer), group paths, layer paths, colours and the colour table, pen weights, text styles, legend labels, and every sentence of every meaning and of this document. `tools/katana_standard/originality.py` scans the two files for what a person could copy and the test `KatanaStandard.NothingButTheKeysIsSharedWithTheNswFileTrackedBesideIt` holds the result: no definition name, group path, layer path, legend label, colour name or colour value (but white) is the same in both, and no stroke list is, exactly, moved or scaled, beyond the dash, the plus and the square that any library draws.

**Where the other library is not clean, this one is.** The NSW customisation holds 1624 rules over 632 keys. 446 of the rules repeat an earlier rule of their key exactly and 258 add nothing to an earlier one, in 465 of the keys; 4 more add only a `hide` that the key's first symbol rule leaves unsaid, so a lookup hides the symbol; 3 keys name a pen colour that resolves to nothing; and 1 draws its symbol through the linestyle slot, which `CODE CHECK` reports as a linestyle that is a symbol. Katana Standard keeps one clean rule set per key: one rule of each kind, every name resolved, the symbol said in a symbol rule, `hide` said where the lookup makes it true. Those 667 differences are listed in `tools/katana_standard/parity_exceptions.json`, one entry for each key and reason, and `parity.py` fails on an entry that explains nothing or a difference that has none.

| Reason | Keys | What the other library does and this one does not |
|---|---|---|
| nsw-duplicate-collapsed | 445 | repeats a rule of the key field for field |
| nsw-shadowed-collapsed | 214 | has a later rule of a kind that says nothing the earlier one of the key did not |
| nsw-fields-merged | 4 | has a later rule that says a field the first leaves unsaid; here the one rule says it |
| nsw-unknown-colour-replaced | 3 | names a pen colour that resolves to nothing |
| nsw-linestyle-symbol-resolved | 1 | names a symbol as a linestyle; here a symbol rule follows the feature rule |

**What this library adds.** 186 keys carry one more attributes rule than the other library has for them, last: the service word `utility.type` and the prompts Owner, Depth (m) and Condition (grammar rule 9). The rule makes no entity, so the same field file still makes the same kinds of entity; it is why a coded string is typed for `UTILITY DRAW`. `parity.py` counts these apart (186 rules) and `--no-extra-attributes` makes them differences.

## The palette

Twenty-eight colours, each named `katana <role>` so a rule reads as the thing it colours, and none can fold onto one of the 27 standard names Katana already knows. Each is at least 3:1 against the plan view's ground (`#1E2329`, a blue-grey that is almost black) and against white paper, since one library is plotted and read on the screen (two AS 5488 colours are excused on paper, below); and no two are closer than 0.05 in OKLab, about where two thin lines of different colour stop being told apart. That leaves only a narrow band of lightness, so the six warm classes (kerb, wall, contour, fuel, fence, breakline) were placed by a search, each at least 0.06 from every other colour, and the six classes drawn with the thinnest pens (tree, planting, ground, building, wall, communications) keep at least 3.25 : 1 on both grounds, as they print. Colour does not carry a class alone: for a reader who confuses red with green some warm pairs stay close whatever their values, so every pipe has its letter and every line class its own pattern.

**The service colours are AS 5488's.** AS 5488.1 has a primary code and line colour table (its Appendix B) that names a colour for each service: water blue, electricity orange, communications white (black when drawn on a white ground), gas yellow, sewer cream, stormwater and drainage green, recycled water purple, fire service red and unidentified services pink; published guides add petroleum brown. The standard names the colours and gives no sRGB, and it leaves the colours of plans to whoever draws them (clause 1.2.2 of the 2013 edition), so each value here is Katana's rendering of the name. The sources, the names and the values are in `docs/subsurface_utilities.md`, "Colours". Traffic systems have no AS 5488 colour and are Katana's choice. The US convention (electricity red, communications orange, sewer green), which this library first used, is not Australian, so it is rejected: the library follows AS 5488.

**The two utility conventions agree.** `UTILITY DRAW` draws its layers in these same values (`utilityTypeColour`), and `KatanaStandard.EveryServiceColourIsTheColourUtilityDrawUses` holds the library and that table equal, service by service, and holds every utility code to its service's colour. A drawing that mixes coded survey strings and `UTILITY DRAW` output has one convention in it. Hazard is the one other colour that moved: AS 5488's electricity orange left the old hazard orange 0.043 away, under the 0.05 floor, so hazard is a red-orange (`#FF5A37`) 0.077 from electricity.

**Yellow and cream print light.** On white paper yellow gas (1.42 : 1) and cream sewer (1.30 : 1) cannot reach 3:1 and still be yellow and cream. The colours are the standard's, so they are not darkened: they keep their 3:1 against the plan ground and print light, and the tests name them as the only two excused. Communications is white, which the plot prints black (`cad::paperColour`: every channel at 230 or more), so it is held to the floor as printed (21.00 : 1). A plot that must show gas and sewer firmly plots monochrome, which prints every pen black, or recolours those layers.

![The 28 colours of Katana Standard on the plan ground and on white paper](images/katana-standard-palette.png)

**Buried services**

| Colour | Value | Used for | AS 5488 | On the ground | On white |
|---|---|---|---|---|---|
| `katana water` | `#2F80ED` | potable water | blue | 4.09 : 1 | 3.87 : 1 |
| `katana sewer` | `#F0E2B0` | sewer | cream | 12.21 : 1 | 1.30 : 1 (prints light) |
| `katana gas` | `#FFD500` | gas | yellow | 11.12 : 1 | 1.42 : 1 (prints light) |
| `katana electricity` | `#E07000` | electricity | orange | 4.89 : 1 | 3.24 : 1 |
| `katana telecom` | `#FFFFFF` | communications | white | 15.82 : 1 | 21.00 : 1 (prints black) |
| `katana recycled` | `#9B59D0` | recycled water | purple | 3.57 : 1 | 4.44 : 1 |
| `katana fire` | `#E53935` | fire service | red | 3.74 : 1 | 4.23 : 1 |
| `katana fuel` | `#A36215` | fuel and petroleum | brown | 3.25 : 1 | 4.87 : 1 |
| `katana its` | `#0B9BA8` | traffic systems | Katana's choice | 4.72 : 1 | 3.35 : 1 |
| `katana unknown` | `#F2559B` | unidentified service | pink | 4.93 : 1 | 3.21 : 1 |

**Water and drainage**

| Colour | Value | Used for | AS 5488 | On the ground | On white |
|---|---|---|---|---|---|
| `katana stormwater` | `#00A800` | stormwater pipes and pits | green | 4.97 : 1 | 3.18 : 1 |
| `katana waterway` | `#6399BE` | creeks, open drains and water edges |  | 5.14 : 1 | 3.07 : 1 |

**Roads, street and rail**

| Colour | Value | Used for | AS 5488 | On the ground | On white |
|---|---|---|---|---|---|
| `katana pavement` | `#789295` | road edges, markings and paths |  | 4.78 : 1 | 3.31 : 1 |
| `katana kerb` | `#907368` | kerbs |  | 3.64 : 1 | 4.34 : 1 |
| `katana street` | `#A2759D` | street furniture and signs |  | 4.19 : 1 | 3.77 : 1 |
| `katana rail` | `#816FA3` | track, overhead wire and platforms |  | 3.57 : 1 | 4.43 : 1 |

**Land, ground and boundaries**

| Colour | Value | Used for | AS 5488 | On the ground | On white |
|---|---|---|---|---|---|
| `katana boundary` | `#796EDA` | lot boundaries, easements and boundary marks |  | 3.81 : 1 | 4.15 : 1 |
| `katana contour` | `#B9704F` | contours, rock and cliff |  | 4.14 : 1 | 3.82 : 1 |
| `katana breakline` | `#BE7D83` | breaklines |  | 4.86 : 1 | 3.26 : 1 |
| `katana ground` | `#688F68` | ground shots and levels |  | 4.31 : 1 | 3.67 : 1 |

**Vegetation**

| Colour | Value | Used for | AS 5488 | On the ground | On white |
|---|---|---|---|---|---|
| `katana tree` | `#5C8627` | trees |  | 3.69 : 1 | 4.29 : 1 |
| `katana planting` | `#779624` | the edges of timber, scrub, grass and crops |  | 4.65 : 1 | 3.40 : 1 |

**Built form**

| Colour | Value | Used for | AS 5488 | On the ground | On white |
|---|---|---|---|---|---|
| `katana building` | `#627B8C` | buildings and structures |  | 3.56 : 1 | 4.44 : 1 |
| `katana fence` | `#A18C65` | fences and gates |  | 4.86 : 1 | 3.25 : 1 |
| `katana wall` | `#BA5556` | walls |  | 3.41 : 1 | 4.64 : 1 |

**Control, notes and hazards**

| Colour | Value | Used for | AS 5488 | On the ground | On white |
|---|---|---|---|---|---|
| `katana control` | `#0999C9` | survey control and investigation points |  | 4.83 : 1 | 3.28 : 1 |
| `katana note` | `#9A8EA4` | text and plan marks |  | 5.11 : 1 | 3.10 : 1 |
| `katana hazard` | `#FF5A37` | hazards |  | 5.10 : 1 | 3.10 : 1 |

## Linestyles

98 linestyles, each a repeating cell. A linestyle named by a rule replaces the line, so the gaps are the library's job; every one is drawn so that it reads at plan scale on screen and on a plotted sheet.

![All 98 linestyles of Katana Standard, drawn by Katana](images/katana-standard-linestyles.png)

*One sample of each, drawn by a code that uses it. Pen weights are not yet applied, so every line is the same width here. This is a plot on white paper: the communications cable prints black, and the sewer and gas pipes print light, since AS 5488's cream and yellow are under 3:1 on white (see the palette).*

**How they are drawn.** The rules the data's own check enforces:

- Units are paper millimetres, so a pattern keeps its size at any plan scale.  +x runs along the line and +y is to its LEFT.  Instance k starts at k times `length`; every stroke lies inside 0..length in x and inside plus or minus 3.4 mm in y, so a pattern closes on itself and nothing overshoots a join.
- A cell is laid out from x = 0.  A dash starts it, so a line opens with a full dash, and every dash is one stroke inside its cell: nothing abuts across the join, which would show as a faint seam where two antialiased ends meet.  Marks stand at the exact middle of a gap or of the cell.  Waves start on the line.
- The repeating pattern is mirror-symmetric along the line, about the middle of one of its dashes or marks, so read from either end it has the same marks in the same places: only where a mark stands to one side (a tick, a hump) does the digitising direction show, always as the LEFT.  The few patterns with a direction of their own (the flow chevrons, the dot-dot-dash drain) or an irregular tuft are named in SYMMETRY with "none", and the barrier hatch, which leans, with "point"; the check proves the symmetry of the rest.
- Katana lays a definition by giving every stroke POINT its own distance along the line, so a long straight stroke is a chord and cuts a corner.  Every straight run along the line is drawn in chords of at most one millimetre (`_Pen.run`), which keeps a dash or a baseline within about 0.35 mm of a right-angle corner.
- A dot is a 0.6 mm dash.  Katana draws the `dot` stroke as one point the width of the pen: a speck a third of the line's weight on a plot and a single pixel on the screen, so that a dash-dot line reads as dashes.
- Posts stand ON the line, which runs through them: guard rails, fence posts, chain mesh.  Beads are THREADED on it, the line stopping at each side of the bead and the inside left open: the contour diamond and the insulator ring.  The two kinds of mark are then told apart at a glance even where their outlines match.
- Letters are 2.4 mm and `middle-centre`, in a gap of 5 mm or more.  Numbers carry at most three decimals. No stroke names a pen: the code's rule gives the colour.
- Paint is drawn as lines, so a "solid" shape (an arrowhead, a block) is a closed outline ruled with fine lines parallel to its axis, 0.4 to 0.5 mm apart, which reads as ink at plan scale.  A road arrow lies along the line and points the way you shot; a turn or a branch stands to the LEFT or RIGHT of the line, as its name says.  A mark that leans (an arrow, a hatch tick) is named in SYMMETRY with "none".

Reading a table: *Period* is the length of one cell in millimetres of paper; *Used by* lists the codes that name it. A linestyle shared by several codes is the same line in each: the layer and the colour tell them apart.

### Boundaries and Cadastre/Lot Boundaries

| Linestyle | Period (mm) | What it looks like and when to use it | Used by |
|---|---|---|---|
| `Title Boundary` | 25 | Unbroken line crossed by a short tick every 25 mm: the registered title boundary, the firmest line in the cadastre. | `BT*` |
| `Compiled Boundary` | 14 | Long dash, then one dot: a boundary compiled from plans and not measured in the field. | `BC*` |
| `Estimated Boundary` | 12 | Unbroken line with a small ring standing against it on the left every 12 mm: a boundary known only to about half a metre. | `BN*` |
| `Digital Boundary` | 3.5 | A sparse dotted line, one dot every 3.5 mm: a boundary read from the digital cadastre, the least certain of the boundary lines. | `BM*` |
| `Acquisition Boundary` | 14 | Long dashes with the letter A standing in each gap: the proposed boundary of land to be acquired, not yet on title. | `BY*` |
| `Stratum Parcel Edge` | 6 | A square wave 1.2 mm high with 3 mm steps and no baseline, like levels stacked one above the next: the limit of a stratum parcel. | `BZ*` |
| `Survey Join Line` | 9 | Fine dashes with a short bar across the line in each gap: a line that joins two points of a plan and is not itself a boundary. | `JL*` |

### Boundaries and Cadastre/Easements and Reserves

| Linestyle | Period (mm) | What it looks like and when to use it | Used by |
|---|---|---|---|
| `Easement Boundary` | 9 | Paired short dashes, 2.6 on, 0.9 off, 2.6 on, then 2.9 off: the edge of an easement or covenant area; the code says what it carries. | `BE*` |

### Roads and Pavements/Carriageway

| Linestyle | Period (mm) | What it looks like and when to use it | Used by |
|---|---|---|---|
| `Unsealed Road Edge` | 2.5 | A fine stipple of short specks, like gravel thrown on the edge: the edge of an unsealed road. | `EK*` `UR*` `VT*` |
| `Road Centreline` | 14 | Long dashes with a cross tick standing in each gap: the centre of the carriageway, shot at the crown. | `OF*` `RC*` `NL*` |

### Roads and Pavements/Markings

| Linestyle | Period (mm) | What it looks like and when to use it | Used by |
|---|---|---|---|
| `Dashed Lane Marking` | 12 | Short painted dashes with long gaps, 4 on and 8 off: the dashed line between traffic lanes. | `LL*` `L3*` `L4*` `L6*` `LN*` |
| `Double Barrier Marking` | 10 | Two close unbroken lines, 1 mm apart: the double continuous barrier line painted on a road. | `DB*` `LD*` |
| `Zebra Crossing Marking` | 3 | A row of bars across the line with no baseline, like the stripes of a zebra crossing; shoot it along the crossing's width. | `CZ*` `TL*` `LZ*` |
| `Lane Continuity Marking` | 8 | Short paint dashes, 2 on and 6 off: the broken line that carries a lane, a turn or a give-way line through a junction. | `CC*` `LA*` `LF*` `LJ*` `LT*` `LW*` |
| `Equal Dash Marking` | 10 | Paint dashes and gaps of equal length, 5 on and 5 off: the broken line of separation and clearway marking. | `CN*` `CW*` |
| `Dividing Line Marking` | 12 | Long paint dashes, 9 on and 3 off: the broken line that divides opposing traffic, told from the lane line by its long dash. | `L2*` `L8*` `L9*` |
| `Give Way Marking` | 4 | Unbroken line with a row of small triangles standing on it to the left, one every 4 mm: the give-way line across a turning lane. | `CG*` |
| `Left Chevron Marking` | 5 | A row of open chevrons 3 mm wide centred on the line, each with its point to the left of travel: painted hatching on an island or gore. | `CV*` |
| `Right Chevron Marking` | 5 | A row of open chevrons 3 mm wide centred on the line, each with its point to the right of travel: painted hatching on an island or gore. | `CY*` |
| `Crossing Approach Marking` | 8 | A sharp zigzag 3 mm high and 8 mm long with no baseline: the zigzag painted along the approach to a pedestrian crossing. | `CA*` |
| `Signal Crossing Marking` | 8 | Two parallel rows of dashes 4 mm apart, 4 on and 4 off: the lines that bound a pedestrian crossing controlled by signals. | `CX*` |
| `Left Broken Barrier Marking` | 9 | An unbroken line with a broken line 1 mm to its left, 6 on and 3 off: a barrier line on one side and a separation line on the other. | `BS*` `LH*` |
| `Right Broken Barrier Marking` | 9 | An unbroken line with a broken line 1 mm to its right, 6 on and 3 off: the mirror of the left broken barrier marking. | `SB*` `LR*` |
| `Left Dragons Teeth Marking` | 6 | A row of painted triangles 4 mm along the line and 3 mm deep, standing to its left, one every 6 mm, with no baseline: dragons teeth. | `DA*` |
| `Right Dragons Teeth Marking` | 6 | A row of painted triangles 4 mm along the line and 3 mm deep, standing to its right, one every 6 mm, with no baseline: dragons teeth. | `DC*` |
| `Bicycle Lane Marking` | 10 | Dashes with a pair of small rings, like bicycle wheels, in each gap: the broken line beside a bicycle lane or along a bicycle path. | `L0*` `L7*` |
| `Bicycle Crossing Marking` | 2.4 | Two rows of small squares 3 mm apart with no baseline, the line running between them: a bicycle crossing painted across the carriageway. | `LY*` |
| `Straight Ahead Arrow Marking` | 22 | A painted lane arrow along the line, a shaft ending in a ruled arrowhead, one every 22 mm and pointing the way you shot. | `AS*` |
| `Left Turn Arrow Marking` | 14 | A painted lane arrow along the line: a shaft that turns 90 degrees to the left into a ruled arrowhead, one every 14 mm. | `AL*` |
| `Right Turn Arrow Marking` | 14 | A painted lane arrow along the line: a shaft that turns 90 degrees to the right into a ruled arrowhead, one every 14 mm. | `AR*` |
| `Straight and Left Arrow Marking` | 22 | A painted lane arrow along the line: a straight arrow with a second, smaller arrow branching to the left, one every 22 mm. | `AE*` |
| `Straight and Right Arrow Marking` | 22 | A painted lane arrow along the line: a straight arrow with a second, smaller arrow branching to the right, one every 22 mm. | `AI*` |

### Street Furniture and Signs/Signs

| Linestyle | Period (mm) | What it looks like and when to use it | Used by |
|---|---|---|---|
| `Hundred Speed Zone Line` | 30 | Unbroken line broken every 30 mm by the number 100: a stretch of road signed with a speed limit of 100 km/h. | `S1*` |
| `Hundred and Ten Speed Zone Line` | 30 | Unbroken line broken every 30 mm by the number 110: a stretch of road signed with a speed limit of 110 km/h. | `S2*` |
| `Forty Speed Zone Line` | 30 | Unbroken line broken every 30 mm by the number 40: a stretch of road signed with a speed limit of 40 km/h. | `S4*` |
| `Fifty Speed Zone Line` | 30 | Unbroken line broken every 30 mm by the number 50: a stretch of road signed with a speed limit of 50 km/h. | `S5*` |
| `Sixty Speed Zone Line` | 30 | Unbroken line broken every 30 mm by the number 60: a stretch of road signed with a speed limit of 60 km/h. | `S6*` |
| `Seventy Speed Zone Line` | 30 | Unbroken line broken every 30 mm by the number 70: a stretch of road signed with a speed limit of 70 km/h. | `S7*` |
| `Eighty Speed Zone Line` | 30 | Unbroken line broken every 30 mm by the number 80: a stretch of road signed with a speed limit of 80 km/h. | `S8*` |
| `Ninety Speed Zone Line` | 30 | Unbroken line broken every 30 mm by the number 90: a stretch of road signed with a speed limit of 90 km/h. | `S9*` |

### Street Furniture and Signs/Posts and Barriers

| Linestyle | Period (mm) | What it looks like and when to use it | Used by |
|---|---|---|---|
| `Guard Rail Line` | 12 | Unbroken line with a small square post every 12 mm, the rail running through it: a roadside guard rail or crash barrier. | `FG*` `TG*` `BF*` `BG*` `MS*` |
| `Wire Rope Barrier Line` | 3 | Two fine waves in opposite phase that cross every 1.5 mm like a twisted rope, with no posts: a safety barrier of tensioned cable. | `FW*` |
| `Concrete Barrier Line` | 2.4 | Two parallel lines 1.6 mm apart ruled with a close diagonal hatch, like concrete in section: a solid safety barrier, shot along its base. | `KJ*` |

### Kerbs and Drainage/Kerbs

| Linestyle | Period (mm) | What it looks like and when to use it | Used by |
|---|---|---|---|
| `Gutter Lip Line` | 9 | Long dashes with a narrow gap, 7.6 on and 1.4 off, like the joints of a concrete gutter: the flow line at the foot of a kerb face. | `FI*` `LP*` |
| `Kerb Back Line` | 3.3 | Fine short dashes, 2.2 on and 1.1 off, stitched along the line: the back edge of a kerb, where it meets the verge or the path. | `BK*` |
| `Dish Drain Edge` | 8 | Unbroken line with a shallow V standing to its left every 8 mm: a V-shaped concrete dish drain. | `DD*` `ID*` |

### Kerbs and Drainage/Pipes and Culverts

| Linestyle | Period (mm) | What it looks like and when to use it | Used by |
|---|---|---|---|
| `Stormwater Pipe Line` | 16 | A line broken by the letter D every 16 mm: a stormwater pipe, shot at the surface or inside the pits. | `U1*` `U2*` `U3*` `U4*` `U5*` `U6*` `U7*` `U9*` `UU*` `UX*` `UZ*` `V1*` `V2*` `V3*` `V5*` `V6*` `V8*` `DI*` |
| `Subsoil Drain Line` | 9 | Two dots and a dash in turn, written in the digitising direction: a perforated subsoil drain. | `SH*` |
| `Culvert Outline` | 12 | Two parallel lines 1.8 mm apart joined by a tie every 12 mm, like a ladder: a rectangular culvert. | `B0*` `B1*` `B2*` `B3*` `B4*` `B6*` `B7*` `B9*` `BX*` `D0*` `D1*` `D2*` `D3*` `D4*` `D5*` `D6*` `D7*` `D8*` `D9*` `DZ*` `UB*` `DV*` |

### Kerbs and Drainage/Channels and Waterways

| Linestyle | Period (mm) | What it looks like and when to use it | Used by |
|---|---|---|---|
| `Open Drain Invert` | 14 | Unbroken line with an open chevron that points the way you shot, downstream: an open drain, a swale or a flow path. | `DF*` `DT*` `DL*` `DU*` |
| `Creek Centreline` | 12 | A smooth continuous wave 1.8 mm high and 6 mm long: the centre of a creek or river bed. | `WC*` |

### Rail and Transit/Track

| Linestyle | Period (mm) | What it looks like and when to use it | Used by |
|---|---|---|---|
| `Running Rail` | 4 | Unbroken line crossed by a tie every 4 mm, like a ladder: the head of a running rail. | `RA*` |
| `Track Centreline` | 14 | Long dashes with a small square standing in each gap: the centre of the track between the rails. | `RT*` |
| `Track Formation Edge` | 7 | Unbroken line with a pair of close short ticks to the left every 7 mm: the edge of the formation that carries the ballast. | `RF*` |

### Rail and Transit/Platforms

| Linestyle | Period (mm) | What it looks like and when to use it | Used by |
|---|---|---|---|
| `Platform Edge Line` | 5 | Unbroken line with a short tick to the left every 5 mm: the edge of a platform. | `RP*` `LB*` |

### Utilities/Water

| Linestyle | Period (mm) | What it looks like and when to use it | Used by |
|---|---|---|---|
| `Water Supply Pipe` | 16 | A line broken by the letter W every 16 mm: a pressure main that carries drinking water. | `UO*` `WI*` `WM*` `WZ*` |

### Utilities/Sewer

| Linestyle | Period (mm) | What it looks like and when to use it | Used by |
|---|---|---|---|
| `Sewer Gravity Pipe` | 20 | A line broken by the letter S and a small flow chevron: a gravity sewer. The chevron points the way you shot, so shoot downhill. | `SM*` `SZ*` |
| `Sewer Pressure Pipe` | 20 | A line broken by the letters SR every 20 mm: a pressure sewer that carries flow uphill. | `SR*` |

### Utilities/Gas

| Linestyle | Period (mm) | What it looks like and when to use it | Used by |
|---|---|---|---|
| `Gas Supply Pipe` | 16 | A line broken by the letter G every 16 mm: a pipe that carries natural gas. | `HA*` `HG*` `LG*` `NG*` `YG*` `ZG*` |

### Utilities/Electricity

| Linestyle | Period (mm) | What it looks like and when to use it | Used by |
|---|---|---|---|
| `Underground Cable` | 12 | A line broken by the letter E every 12 mm: a buried electricity cable. | `EA*` `EU*` `EZ*` |
| `Overhead Line` | 14 | Unbroken line with a small ring, an insulator, threaded on it every 14 mm: an overhead power or communications line. | `SC*` `UE*` `UL*` `OA*` |

### Utilities/Communications

| Linestyle | Period (mm) | What it looks like and when to use it | Used by |
|---|---|---|---|
| `Communications Cable` | 12 | A line broken by the letter C every 12 mm: a buried telephone, data or fibre cable. | `OU*` `OZ*` `TN*` `TZ*` |

### Utilities/Recycled Water

| Linestyle | Period (mm) | What it looks like and when to use it | Used by |
|---|---|---|---|
| `Recycled Water Pipe` | 16 | A line broken by the letter R every 16 mm: a main that carries recycled water. | `RM*` |

### Utilities/Fuel

| Linestyle | Period (mm) | What it looks like and when to use it | Used by |
|---|---|---|---|
| `Fuel Supply Pipe` | 16 | A line broken by the letter P every 16 mm: a pipeline for fuel or oil. | `FU*` `FZ*` `HO*` `HZ*` |

### Utilities/Traffic Systems

| Linestyle | Period (mm) | What it looks like and when to use it | Used by |
|---|---|---|---|
| `Traffic Systems Cable` | 18 | A line broken by the letters TS every 18 mm: a buried cable of the traffic systems service, which serves signals and cameras. | `IT*` `SD*` |

### Utilities/Unknown Services

| Linestyle | Period (mm) | What it looks like and when to use it | Used by |
|---|---|---|---|
| `Unknown Service Line` | 12 | A line broken by a question mark every 12 mm: a buried line whose service is not yet known. | `UP*` |

### Utilities/All Services

| Linestyle | Period (mm) | What it looks like and when to use it | Used by |
|---|---|---|---|
| `Buried Duct` | 10 | Two close parallel dashed lines, 1.2 mm apart: a duct or conduit that carries cable, whatever the service. | `RS*` `ED*` `OD*` `TD*` |
| `Service Connection Line` | 6 | Fine even dashes, 3 on and 3 off: the small property connection between a main and a lot. | `WY*` `SY*` `DG*` `EY*` `TY*` |

### Vegetation and Landscape/Ground Cover

| Linestyle | Period (mm) | What it looks like and when to use it | Used by |
|---|---|---|---|
| `Bush Edge Line` | 7 | Unbroken line with three uneven ticks to the left, like a tuft: the edge of scrub or bush. | `JD*` |
| `Medium Bush Edge` | 12 | Unbroken line with one tuft of three uneven ticks to its left every 12 mm: the edge of medium-density timber or scrub. | `JM*` |
| `Scattered Bush Edge` | 20 | Dashes with one short tick to the left in each gap: the edge of scattered timber or scrub, the sparsest of the three bush edges. | `JS*` |
| `Grass Edge` | 6 | Unbroken line with a small fan of three blades to the left every 6 mm: the edge of mown grass. | `JG*` `OG*` `OP*` |

### Buildings and Structures/Buildings

| Linestyle | Period (mm) | What it looks like and when to use it | Used by |
|---|---|---|---|
| `Eave Line` | 10.5 | A long dash and a short dash in turn, 1.5 mm apart: the edge of a roof overhang, a verandah or an awning. | `AW*` `BV*` |
| `Hidden Outline` | 3 | Fine short dashes, 1 on and 2 off: a building edge seen only on a plan, or a building not yet finished. | `CB*` `BW*` `OR*` `UC*` `UT*` `SL*` `XB*` |

### Buildings and Structures/Structures

| Linestyle | Period (mm) | What it looks like and when to use it | Used by |
|---|---|---|---|
| `Abutment Foot Line` | 5 | Unbroken line with a small ruled block against it on the left every 5 mm, like the toe of a footing: the foot of a bridge abutment. | `AB*` |

### Buildings and Structures/Steps and Access

| Linestyle | Period (mm) | What it looks like and when to use it | Used by |
|---|---|---|---|
| `Steps Edge` | 3.5 | Unbroken line crossed by a row of treads 2.4 mm long, one every 3.5 mm: the nosing of a flight of steps. | `SO*` |

### Fences and Walls/Fences

| Linestyle | Period (mm) | What it looks like and when to use it | Used by |
|---|---|---|---|
| `Chain Mesh Fence` | 8 | Unbroken line with a small cross every 8 mm: a wire mesh fence on steel posts. | `FN*` |
| `Post and Rail Fence` | 12 | Two parallel rails 1 mm apart with a square post every 12 mm: a timber or steel post and rail fence. | `SA*` |
| `Post and Wire Fence` | 10 | Unbroken line with a small ring for a post every 10 mm: a rural fence of plain wire on posts. | `FE*` |
| `Metal Palisade Fence` | 2 | Unbroken line crossed by a close row of short bars, one every 2 mm: a steel palisade or pool fence. | `FM*` `SF*` |

### Fences and Walls/Walls

| Linestyle | Period (mm) | What it looks like and when to use it | Used by |
|---|---|---|---|
| `Earth Retaining Wall` | 4 | Unbroken line with a small triangle to the left every 4 mm: a wall that holds back earth; the barbs point to the lower side. | `RW*` `MW*` |

### Fences and Walls/Gates and Posts

| Linestyle | Period (mm) | What it looks like and when to use it | Used by |
|---|---|---|---|
| `Gate Leaf Outline` | 4 | Two close rails joined by a rung and a sloping brace, like the frame of a field gate: a gate leaf shot from its hinge to its latch. | `AG*` |

### Terrain and Breaklines/Breaklines

| Linestyle | Period (mm) | What it looks like and when to use it | Used by |
|---|---|---|---|
| `Bank Top Edge` | 6 | Unbroken line with a long tick to the left every 6 mm: the top edge of a slope, the ticks pointing downhill. | `CU*` `TC*` `VL*` `IB*` |
| `Bank Toe Edge` | 3 | Unbroken line with a short tick to the left every 3 mm: the bottom edge of a slope, the ticks standing away from the slope. | `BB*` `OH*` `IA*` |
| `Ridge Line` | 10 | Dashes with a small caret pointing to the left in each gap: a line along a crest. | `NB*` |
| `Soft Breakline` | 4 | Fine short dashes, 2.5 on and 1.5 off: a gentle change in grade that a surface should still follow. | `NS*` |

### Terrain and Breaklines/Contours

| Linestyle | Period (mm) | What it looks like and when to use it | Used by |
|---|---|---|---|
| `Index Contour` | 20 | Unbroken line with a small diamond bead threaded on it every 20 mm: every fifth contour, marked so it can be followed. | `Z*` `Z0*` |
| `Depression Contour` | 8 | Unbroken line with a short tick to the left every 8 mm: a contour round a hollow, the ticks pointing downhill. | `XQ*` `ZQ*` |

### Terrain and Breaklines/Water Edges

| Linestyle | Period (mm) | What it looks like and when to use it | Used by |
|---|---|---|---|
| `Water Edge` | 10 | Dashes with a small ripple arc to the left in each gap: the edge of water, or the line of the highest recent water. | `WE*` `WL*` `WR*` |
| `Marsh Edge` | 16 | Dashes with a two-stroke reed tuft standing to the left on every second dash: the edge of a swamp or marsh. | `JW*` |

### Terrain and Breaklines/Rock and Cliff

| Linestyle | Period (mm) | What it looks like and when to use it | Used by |
|---|---|---|---|
| `Rock Outcrop Edge` | 5 | A sharp zigzag 1.6 mm high with no baseline: the edge of exposed rock. | `JR*` |
| `Cliff Edge` | 4 | Unbroken line with ticks to the left, a long one and a short one in turn: the top edge of a cliff, pointing downhill. | `VC*` |

### Survey Control and Annotation/Plan Marks

| Linestyle | Period (mm) | What it looks like and when to use it | Used by |
|---|---|---|---|
| `Limit of Survey Line` | 24 | Long dashes with an open triangle in each gap: the edge of the surveyed area; nothing beyond it was measured. | `FS*` `NF*` |
| `Photo Detail Boundary` | 14 | Long dashes with a small target, a ring crossed by a plus, in each gap: the limit of detail taken from photography, not the field. | `DE*` |

### Miscellaneous/Hazards and Heritage

| Linestyle | Period (mm) | What it looks like and when to use it | Used by |
|---|---|---|---|
| `Hazard Boundary` | 10 | Dashes with an exclamation mark in each gap: the boundary of a hazardous area. | `HL*` |

### Design and Setting Out/Master Strings

| Linestyle | Period (mm) | What it looks like and when to use it | Used by |
|---|---|---|---|
| `Temporary Works Line` | 10 | Dashes with the letter T in each gap: temporary works, such as a hoarding or the edge of a work zone, that the finished road does not keep. | `MX*` |

### Design and Setting Out/Design General

| Linestyle | Period (mm) | What it looks like and when to use it | Used by |
|---|---|---|---|
| `Left Wall Face Hatch Line` | 1.5 | Unbroken line with close slanting ticks to its left, leaning back along the line: shows which side is the face of a designed wall. | `HE*` |
| `Right Wall Face Hatch Line` | 1.5 | Unbroken line with close slanting ticks to its right, leaning back along the line: shows which side is the face of a designed wall. | `HR*` |

## Symbols

123 symbols, every one drawn at its vertex, unrotated, in paper millimetres, with its origin at the centre of its box unless its description says where else.

![All 123 symbols of Katana Standard, drawn by Katana](images/katana-standard-symbols.png)

**The house style**, which makes each symbol learnt once:

- **Cell.** Three size classes by larger extent: S about 3 mm (small furniture, posts, marks), M about 4.4 mm (most objects), L about 6 mm (canopies, tanks, stations, primary control).  The origin is the centre of the bounding box, so a symbol sits on its vertex without a rule offsetting it.  Where the surveyed point is NOT the box centre (a valve beside its letter, a doorway's wall line, a building corner, a camera's pole, a station's dot, a north arrow's shaft) the origin is within 0.6 mm of the box centre and the description says where it is; the check holds both.
- **Weight.** One weight: every symbol is outline.  The renderer applies the entity's own pen, so a symbol never carries a colour or a width.  Gaps are fixed: 0.4 mm between the rings of a double ring, 0.3 mm between a ring and a ray or tick, and a letter is always 2.4 mm high.
- **Fill.** There are no fills.  A solid centre mark (a very small ring with a dot in it, because Katana draws a `dot` as one pen-width point whatever its radius) means "a physical mark or stem is here and this is its exact point": pegs, pins, pipes, stations, poles, trunks.  A shape WITHOUT one is a cover, a sign or an observation.  Hatching (at most four strokes) means "solid": a borehole's quadrants. A dashed ring means a temporary or former state: a pothole, or a trunk whose width was not taken.  Rays mean a light (street light, bollard light, signal lamp).
- **Services.** One grammar for every service, learnt once; the service colour comes from the rule and the letter names the service (W water, S sewer, G gas, E electricity, C communications, R recycled, F fire, P fuel, I traffic systems, ? unknown).  Square = pit or box; single ring = round cover or pole; double ring = maintenance hole; bow-tie = valve; diamond = regulator or marker; ring with a bar = meter.  A letter is inside a pit, a ring or a hole, and BELOW a valve.  A fitting that several services share (air release valve, scour valve, test point, earthing point, tap) carries no letter: the rule's colour and layer name the service.  A fitting with a lettered family keeps its letter (a vent, a flow meter, a maintenance hole, a pit with two or three lids).
- **Trees.** A tree is shot by its canopy or by its trunk, and both are drawn as plain shapes with a centre dot.  A measured crown is a plain ring with a trunk dot and a radius line, in six sizes; a measured trunk is an eight-sided outline round a trunk dot, in six sizes, dashed when no width was taken.  The sizes step by about 0.8 mm so each is told from the next at screen size, and they are not to scale: a symbol keeps its paper size.
- **Control.** Distinguished by class: triangle with ring and dot = primary station; ring with long cross-hairs and dot = secondary mark; ring with a bar and arrow = benchmark; diamond with dot = fixed mark; open diamond with a plus = temporary; ring and square frame with a plus = the centre of an aerial photograph.
- **Direction.** A symbol is not rotated, so a symbol that points (an arrow, a lens, a gate) points to +x or +y; the rule and the line it sits on say the rest.

Reading a table: *Size* is the symbol's width and height in millimetres of paper, measured from its strokes.

### Boundaries and Cadastre/Boundary Marks

| Symbol | Size (mm) | What it looks like and when to use it | Used by |
|---|---|---|---|
| `Corner Peg` | 3.2 x 3.2 | Square with a centre dot. A peg or stake set at a boundary corner. | `PBPG` `PRMB` |
| `Iron Pin` | 3.2 x 3.2 | Ring with a centre dot. A round iron pin or rod set at a corner. | `PBDY` |
| `Iron Pipe` | 3.4 x 3.4 | Double ring with a centre dot. An iron pipe: the inner ring is its bore. | `PGPI` |
| `Survey Spike` | 2.6 x 2.6 | Small ring held by four ticks. A spike or nail driven into a hard surface; no dot, the ring is the head. | `PGIN` `PNAL` `PSKI` `PBLT` `PSSE` |
| `Winged Drill Hole` | 3.1 x 2.0 | Drilled Hole with an open wedge running out to +x, the wing cut beside it to lead the eye to the hole. The origin is the hole. | `PDHL` |
| `Star Picket` | 2.7 x 2.3 | Three arms at 120 degrees round a centre dot: the section of a steel picket driven in as a mark. The origin is its centre dot. | `PSTA` |

### Roads and Pavements/Markings

| Symbol | Size (mm) | What it looks like and when to use it | Used by |
|---|---|---|---|
| `Raised Pavement Marker` | 3.1 x 1.3 | Ring with a small solid mark each side. A domed marker fixed to the road, the dome between its two reflectors. | `PTMX` |

### Street Furniture and Signs/Signs

| Symbol | Size (mm) | What it looks like and when to use it | Used by |
|---|---|---|---|
| `Bus Stop Sign` | 3.2 x 3.2 | Round-cornered square holding B. A bus stop sign face; the origin is its middle, a little off the post it stands on. | `PBUS` |
| `Sign Post` | 2.0 x 1.8 | Ring with a centre dot and a plate touching its right side. The post of a sign whose face is not classified. | `PSIN` |
| `Double Sided Sign Post` | 2.4 x 1.8 | Ring with a centre dot and a plate touching each side. A post carrying a sign face on both of its sides. | `PSDS` |

### Street Furniture and Signs/Lighting

| Symbol | Size (mm) | What it looks like and when to use it | Used by |
|---|---|---|---|
| `Street Light` | 4.4 x 4.4 | Ring with a centre dot and eight rays. A street light pole and its lantern. | `PLPL` |
| `Bollard Light` | 3.1 x 3.1 | Small ring with four short rays. A light built into a bollard. | `PLGN` |
| `Hanging Light` | 4.0 x 3.4 | Ring hung from a wire by a short hanger, with five rays shining down. A light hung from an overhead wire; the origin is the ring centre. | `PLSU` |

### Street Furniture and Signs/Posts and Barriers

| Symbol | Size (mm) | What it looks like and when to use it | Used by |
|---|---|---|---|
| `Bollard` | 2.0 x 2.0 | Ring inside a ring. A fixed bollard. | `AC*` `AD*` |

### Street Furniture and Signs/Amenities

| Symbol | Size (mm) | What it looks like and when to use it | Used by |
|---|---|---|---|
| `Litter Bin` | 2.3 x 2.6 | Hexagon with a centre dot. A litter or waste bin. | `BI*` `PBIN` |
| `Fireplace` | 3.0 x 3.0 | Square hearth holding a flame with a tongue inside it. A built fireplace or barbecue hearth. | `PFPL` |
| `Parking Meter` | 1.9 x 2.2 | Ring with a centre dot carrying a flat head on its top. A kerbside parking meter; the origin is the pole, the ring centre. | `PKME` |
| `Letterbox` | 2.8 x 2.5 | Wide box with a half-round lid and a slot across its face. A letterbox at a property frontage. | `PMBX` |

### Street Furniture and Signs/Traffic Control

| Symbol | Size (mm) | What it looks like and when to use it | Used by |
|---|---|---|---|
| `Signal Pole` | 3.5 x 2.8 | Ring with a dot beside a tall box holding three lamp dots. A traffic signal pole and its signal head. | `PSGL` |
| `Roadside Camera` | 3.8 x 2.2 | Ring with a triangular lens pointing +x. A speed, red-light or traffic camera; the origin is its pole, the ring's centre. | `PCAM` `PCTV` |
| `Signal Controller Cabinet` | 3.2 x 3.2 | Square holding the letter T. The traffic signal controller cabinet. | `PSCL` |
| `Camera Strobe` | 3.2 x 3.2 | Ring holding a lightning bolt. The flash unit of an enforcement camera, shot at its pole. | `PCFU` |

### Kerbs and Drainage/Pits and Structures

| Symbol | Size (mm) | What it looks like and when to use it | Used by |
|---|---|---|---|
| `Gully Pit` | 3.2 x 3.2 | Square with three grate bars. A stormwater gully pit with its grating. | `PGUL` |
| `Junction Pit` | 3.2 x 3.2 | Square with both diagonals. A stormwater junction pit. | `PJBX` |
| `Stormwater Manhole` | 3.4 x 3.4 | Double ring holding the letter D. A stormwater maintenance hole (D for drainage). | `PDJM` |
| `Headwall` | 5.3 x 3.8 | Three sides of a box, open to -x, with wings flaring outward. A pipe headwall; the pipe arrives from the open side. | `PHWB` `PHWT` |
| `Subsoil Inspection Point` | 2.2 x 2.2 | Small ring holding a smaller square, the cap of the riser. An inspection point on a subsoil drain. | `PSFP` |
| `Wingwall End` | 2.4 x 2.4 | Short wall stub at 45 degrees, two lines apart and closed at the outer end. The free end of a wing wall; the origin is the middle of the stub. | `PEWW` |
| `Sump Inlet` | 3.2 x 3.2 | Square holding a downward arrow that lands on a bar. Drainage dropping into a sump. | `PILT` |

### Rail and Transit/Track

| Symbol | Size (mm) | What it looks like and when to use it | Used by |
|---|---|---|---|
| `Rail Control Box` | 2.8 x 2.8 | Square holding two parallel bars, the rails. A control or relay box beside the track. | `PRBX` |
| `Rail Cable Trough` | 3.4 x 1.2 | Long narrow box cut into three lid slabs by two cross lines. A lineside trough carrying signalling cables. | `PRCC` |

### Rail and Transit/Overhead and Signals

| Symbol | Size (mm) | What it looks like and when to use it | Used by |
|---|---|---|---|
| `Signal Mast` | 4.7 x 1.9 | Pole ring with a dot, joined to a lamp ring with rays. A railway signal on its mast. | `PRSG` |
| `Overhead Mast` | 4.4 x 2.4 | Square with a centre dot and two arms. A mast carrying overhead wires. | `PSTR` |

### Utilities/Water

| Symbol | Size (mm) | What it looks like and when to use it | Used by |
|---|---|---|---|
| `Water Isolation Valve` | 3.6 x 4.4 | Bow-tie with the letter W below. A water main valve; the origin is between valve and letter. | `PWSV` |
| `Water Flow Meter` | 3.4 x 3.4 | Ring holding W above a bar. A water meter on the line. | `PWMR` |

### Utilities/Sewer

| Symbol | Size (mm) | What it looks like and when to use it | Used by |
|---|---|---|---|
| `Sewer Maintenance Hole` | 3.8 x 3.8 | Double ring holding S. A sewer maintenance hole. | `PSMH` |
| `Sewer Vent` | 2.8 x 4.0 | Ring holding S with a chevron above. A sewer vent shaft. | `PSVP` |
| `Sewer Isolation Valve` | 3.6 x 4.4 | Bow-tie with the letter S below. A sewer stop valve; the origin is between valve and letter. | `PSSV` |
| `Sewer Inspection Opening` | 3.2 x 3.2 | Single ring holding S. A small round cover giving a lamp a view along a sewer; the double ring is the man-entry hole. | `PSLH` |

### Utilities/Gas

| Symbol | Size (mm) | What it looks like and when to use it | Used by |
|---|---|---|---|
| `Gas Pit` | 3.4 x 3.4 | Square holding G. A gas valve or meter pit. | `PGAS` |
| `Gas Flow Meter` | 3.4 x 3.4 | Ring holding G above a bar. A gas meter. | `PGMR` |
| `Gas Pressure Regulator` | 3.8 x 3.8 | Diamond holding G. A gas pressure regulating set. | `PGRB` |
| `Gas Maintenance Hole` | 3.8 x 3.8 | Double ring holding the letter G. A gas maintenance hole. | `PGHL` |
| `Gas Vent` | 2.8 x 4.0 | Ring holding G with a chevron above. A vent riser on a gas main or casing. | `PGVP` |
| `High Pressure Marker Post` | 3.0 x 3.0 | Marker Post ring holding two stacked triangles. A pipeline marker for a high pressure gas main. | `PGHM` |

### Utilities/Electricity

| Symbol | Size (mm) | What it looks like and when to use it | Used by |
|---|---|---|---|
| `Power Pole` | 3.1 x 3.1 | Ring, inner ring and centre dot. A power pole: the rings are the pole and its footing. | `PPPL` `PTPL` |
| `Guy Anchor` | 3.6 x 1.2 | Line with an arrowhead at -x and an anchor bar at +x. A pole's stay wire running to its ground anchor. | `PSAP` |
| `Transformer` | 3.8 x 2.4 | Two overlapping rings. A pole or pad transformer. | `PETC` `PPTR` |
| `Electrical Pillar` | 3.0 x 3.0 | Square with one diagonal. A pillar, kiosk or service cabinet; the slash marks a pillar, as on the communications one. | `PEUP` |
| `Electrical Pit` | 3.4 x 3.4 | Square holding E. An electrical pit. | `PEJB` |
| `Electrical Maintenance Hole` | 3.8 x 3.8 | Double ring holding the letter E. A maintenance hole giving access to underground cables. | `PEMH` |
| `Electrical Fuse` | 3.2 x 1.1 | Narrow box with a line through its length that runs past both ends. A fuse on a distribution cable, in a pole or a cabinet. | `PEFP` |
| `Power and Light Pole` | 4.0 x 3.6 | Power Pole with two short sprays of rays at its upper left and right. A pole carrying both power lines and a lantern. | `PPLP` |
| `Stay Pole` | 2.6 x 2.6 | Small ring with a centre dot and a brace running to the upper right, ending in a cross tick. A short pole carrying a stay wire; the origin is the ring centre. | `PSPL` |
| `Transmission Pylon` | 5.8 x 5.8 | Four leg squares at the corners joined by sides and diagonals round a small ring. The footprint of a lattice tower on high voltage lines. | `PHTT` |

### Utilities/Communications

| Symbol | Size (mm) | What it looks like and when to use it | Used by |
|---|---|---|---|
| `Communications Pit` | 3.4 x 3.4 | Square holding C. A communications pit. | `POFP` `PTSP` |
| `Communications Pillar` | 3.2 x 3.2 | Square holding C with a broken diagonal. A communications pillar or cabinet. | `POFJ` `PTDP` |
| `Communications Twin Pit` | 4.7 x 3.0 | Box of two square cells, each holding C. A communications pit with two lids side by side. | `PTTP` |
| `Communications Triple Pit` | 6.3 x 3.0 | Box of three square cells, each holding C. A communications pit with three lids in a row. | `PT3P` |
| `Communications Main Pit` | 6.0 x 6.0 | Large square holding a smaller square lid with C. A main pit at a major cable junction, twice the size of the ordinary pit. | `PTMP` |
| `Telephone Box` | 4.4 x 4.4 | Square holding a telephone handset, an arch with a ring at each end. A public call box on the footpath. | `PTBX` |

### Utilities/Recycled Water

| Symbol | Size (mm) | What it looks like and when to use it | Used by |
|---|---|---|---|
| `Recycled Isolation Valve` | 3.6 x 4.4 | Bow-tie with the letter R below. A recycled water valve; the origin is between valve and letter. | `PRSV` |
| `Recycled Flow Meter` | 3.4 x 3.4 | Ring holding R above a bar. A recycled water meter. | `PRMR` |

### Utilities/Fire Service

| Symbol | Size (mm) | What it looks like and when to use it | Used by |
|---|---|---|---|
| `Fire Hydrant` | 4.6 x 3.0 | Ring with a plus and a nozzle tick each side. A fire hydrant. | `PWHY` `PRHY` `PWFB` |

### Utilities/Fuel

| Symbol | Size (mm) | What it looks like and when to use it | Used by |
|---|---|---|---|
| `Petrol Pump` | 3.5 x 3.2 | Tall box holding P, with a hose running out of its side to a nozzle tick. A fuel dispenser; the origin is the middle of its body. | `PPET` |

### Utilities/Traffic Systems

| Symbol | Size (mm) | What it looks like and when to use it | Used by |
|---|---|---|---|
| `Loop Detector` | 3.4 x 3.4 | Square holding a three-peak zigzag. A traffic detector loop. | `PSDR` |
| `Traffic Systems Pit` | 3.4 x 3.4 | Square holding I. A traffic systems or ITS pit. | `PSJX` |

### Utilities/Unknown Services

| Symbol | Size (mm) | What it looks like and when to use it | Used by |
|---|---|---|---|
| `Unknown Pit` | 3.4 x 3.4 | Square holding a question mark. A pit whose service is not known. | `PGAT` |

### Utilities/All Services

| Symbol | Size (mm) | What it looks like and when to use it | Used by |
|---|---|---|---|
| `Pumping Station` | 5.4 x 5.4 | Square holding a ring with a flow arrow. A pumping station of any service. | `PWEP` `PSPS` |
| `Marker Post` | 2.0 x 2.0 | Ring holding an upward triangle. A route marker post for any service. | `PWMM` `PSMM` `PGPM` `PECM` `POFM` `PTCM` `PTJP` `PRMM` `PFPM` `POHM` |
| `Air Release Valve` | 3.2 x 2.8 | Bow-tie with a stem and an arrowhead rising from it. A valve that lets air out at a high point of a pressure main. | `PWAV` `PRAV` |
| `Scour Valve` | 3.2 x 2.8 | Bow-tie with a stem and an arrowhead falling from it. A valve that empties a main at a low point. | `PWSR` |
| `Test Point` | 1.6 x 2.3 | Small ring with a stem and a cross bar rising from it. A fitting on a pipeline where pressure or protection is tested; the origin is the ring centre. | `PWCP` `PGTP` |
| `Earthing Point` | 1.8 x 2.5 | Small ring on a stem that ends in three bars of falling length. The point where a pipe is bonded to earth; the origin is the middle of the sign. | `PWET` `PRET` |
| `Tap` | 1.7 x 2.3 | Small ring with a spout that rises and turns down. A tap, outlet or standpipe; the origin is the ring centre. | `PWTP` `PRTP` |

### Vegetation and Landscape/Trees

| Symbol | Size (mm) | What it looks like and when to use it | Used by |
|---|---|---|---|
| `Tiny Tree Canopy` | 2.2 x 2.2 | Plain ring 2.2 mm across with a trunk dot and a radius line. A tree with a measured spread of one or two metres; not to scale. | `PF01` `PF02` |
| `Small Tree Canopy` | 3.0 x 3.0 | Plain ring 3.0 mm across with a trunk dot and a radius line. A tree with a measured spread of three or four metres; not to scale. | `PF03` `PF04` |
| `Medium Tree Canopy` | 3.8 x 3.8 | Plain ring 3.8 mm across with a trunk dot and a radius line. A tree with a measured spread of five or six metres; not to scale. | `PF05` `PF06` |
| `Large Tree Canopy` | 4.6 x 4.6 | Plain ring 4.6 mm across with a trunk dot and a radius line. A tree with a measured spread of seven or eight metres; not to scale. | `PF07` `PF08` |
| `Broad Tree Canopy` | 5.4 x 5.4 | Plain ring 5.4 mm across with a trunk dot and a radius line. A tree with a measured spread of nine or ten metres; not to scale. | `PF09` `PF10` |
| `Giant Tree Canopy` | 6.2 x 6.2 | Plain ring 6.2 mm across with a trunk dot and a radius line. A tree with a measured spread of twelve metres or more; not to scale. | `PF12` `PF15` |
| `Hundred Millimetre Tree Trunk` | 1.8 x 1.8 | Eight-sided outline 1.8 mm across round a centre dot. A tree trunk about 100 mm across, shot at its middle. | `PT01` |
| `Two Hundred Millimetre Tree Trunk` | 2.6 x 2.6 | Eight-sided outline 2.6 mm across round a centre dot. A tree trunk about 200 mm across, shot at its middle. | `PT02` |
| `Three Hundred Millimetre Tree Trunk` | 3.4 x 3.4 | Eight-sided outline 3.4 mm across round a centre dot. A tree trunk about 300 mm across, shot at its middle. | `PT03` |
| `Narrow Tree Trunk` | 4.2 x 4.2 | Eight-sided outline 4.2 mm across round a centre dot. A tree trunk from 400 to 600 mm across, shot at its middle. | `PT04` `PT05` `PT06` |
| `Medium Tree Trunk` | 5.2 x 5.2 | Eight-sided outline 5.2 mm across round a centre dot. A tree trunk from 700 to 900 mm across, shot at its middle. | `PT07` `PT08` `PT09` |
| `Wide Tree Trunk` | 6.2 x 6.2 | Eight-sided outline 6.2 mm across round a centre dot. A tree trunk of 1000 mm or more across, shot at its middle. | `PT10` `PT12` `PT15` |
| `Unmeasured Tree Trunk` | 4.2 x 4.2 | Dashed eight-sided outline 4.2 mm across round a centre dot. A tree trunk whose width was not measured. | `PTRE` |

### Buildings and Structures/Buildings

| Symbol | Size (mm) | What it looks like and when to use it | Used by |
|---|---|---|---|
| `Building Corner` | 1.2 x 1.2 | Right-angle bracket with its corner at the origin. A building corner; it opens to +x and -y. | `PCBU` |
| `Awning Point` | 3.2 x 1.6 | Box whose lower edge is a row of four scallops. A point on an awning or canopy over a footpath. | `PAWN` |

### Buildings and Structures/Structures

| Symbol | Size (mm) | What it looks like and when to use it | Used by |
|---|---|---|---|
| `Bridge Column` | 3.2 x 3.2 | Ring crossed by an X. A bridge pier or column. | `PBPI` |
| `Mast` | 2.6 x 4.0 | A-frame with a crossbar and a small ring on its apex. A communications or lighting mast. | `PMPT` |
| `Storage Tank` | 5.8 x 5.8 | Ring inside a ring. A storage tank or silo seen from above. | `PUTP` |
| `Flagpole` | 2.9 x 1.2 | Ring with a pennant flying to +x. A flagpole. | `PFLG` |
| `Top Abutment Point` | 2.8 x 2.8 | Square with a hatched band along its upper edge. A point on the top of a bridge abutment. | `PABT` |
| `Bottom Abutment Point` | 2.8 x 2.8 | Square with a hatched band along its lower edge. A point at the foot of a bridge abutment. | `PABB` |
| `Pile Cap` | 3.0 x 3.0 | Square holding a ring. A bridge pile standing in its cap, seen from above. | `PBCP` |
| `Windmill` | 5.9 x 5.9 | Four leaning kite-shaped vanes round a hub ring. A windmill, shot at the middle of its tower. | `PWML` |

### Fences and Walls/Gates and Posts

| Symbol | Size (mm) | What it looks like and when to use it | Used by |
|---|---|---|---|
| `Fence Post` | 1.2 x 1.2 | Small square. A fence post at every vertex of a fence line. | `POST` |

### Terrain and Breaklines/Ground Points

| Symbol | Size (mm) | What it looks like and when to use it | Used by |
|---|---|---|---|
| `Ground Shot` | 1.5 x 1.5 | Dot with a small plus. A surveyed ground point. | `PNSS` |
| `Spot Level` | 3.0 x 3.0 | Plus through a ring a millimetre and a half across. A spot level on the ground or a structure. | `PRAL` `PFLR` `PSHT` |
| `Invert Level` | 2.0 x 2.0 | Ring holding a downward triangle. The invert of a pipe or channel. | `PSDO` `PI01` `PI02` `PI03` `PI04` `PI05` `PI06` `PI07` `PI09` `PI10` `PI12` `PI13` `PI15` `PI16` `PI18` `PINV` |
| `Breakline Point` | 3.0 x 0.5 | Dot between two bars, a dash-dot-dash. A ground point that lies on a breakline. | `PBRK` |
| `Obvert Level` | 2.2 x 2.2 | Ring holding an arch over a bar. The obvert of a pipe, its highest inside point; the pair of Invert Level. | `POBV` |

### Terrain and Breaklines/Water Edges

| Symbol | Size (mm) | What it looks like and when to use it | Used by |
|---|---|---|---|
| `Water Level Mark` | 2.2 x 2.9 | Downward triangle standing on two wavy lines. The level of a water surface, or of a flood; the origin is the triangle's point. | `PFHT` `PWLP` |
| `Swamp` | 3.6 x 1.9 | Two broken dashes with three reed ticks standing on the upper one. A swamp or marsh marked by a point. | `PSWA` |

### Survey Control and Annotation/Control Marks

| Symbol | Size (mm) | What it looks like and when to use it | Used by |
|---|---|---|---|
| `Trigonometric Station` | 5.6 x 4.8 | Large triangle holding a ring and a dot. A primary control station; the origin is its dot. | `PSSF` `PTSS` |
| `Control Mark` | 4.6 x 4.6 | Ring with long cross-hairs and a dot. A secondary or traverse control mark. | `PCMK` `PHCP` `PMCP` `PSSA` `PSSG` `PSSM` |
| `Level Benchmark` | 3.2 x 3.2 | Ring with a bar across it and an upward arrow above the bar. A levelling benchmark. | `PBMK` `PBMZ` `PSSB` `PVCP` |
| `Fixed Survey Mark` | 3.6 x 3.6 | Diamond with a centre dot. A permanent survey mark. | `PCRM` `PMSM` `PPMK` `PRTM` `PDSM` `PPSM` `PSSD` |
| `Temporary Control` | 3.6 x 3.6 | Open diamond with a plus and no dot. A temporary control point. | `PDPY` `PSSC` |
| `Alignment Pin` | 4.3 x 1.7 | Ring with a centre dot between two bars that run along the alignment. A pin set on a design alignment for setting out. | `PAPN` |
| `Photo Centre` | 3.6 x 3.6 | Ring holding a square frame with a small plus at its middle. The centre of an aerial photograph. | `PPCN` |

### Survey Control and Annotation/Investigation Points

| Symbol | Size (mm) | What it looks like and when to use it | Used by |
|---|---|---|---|
| `Borehole` | 3.4 x 3.4 | Ring split into quadrants, two opposite ones evenly hatched. A borehole. | `PBHX` |
| `Test Pit` | 3.4 x 3.4 | Square with a diamond joining the middles of its sides. A test pit or trench. | `PGEO` |
| `Pothole` | 3.6 x 3.6 | Ring of eight dashes holding P. A pothole dug to expose a service. | `PHNL` `PHRL` |
| `Seismic Shot Point` | 2.8 x 1.7 | Dot with two arcs on each side, the pulse leaving it. A seismic test shot. | `PSTS` |

### Miscellaneous/Hazards and Heritage

| Symbol | Size (mm) | What it looks like and when to use it | Used by |
|---|---|---|---|
| `Hazard Point` | 3.8 x 3.3 | Warning triangle holding an exclamation mark. Any hazard the crew flags: unstable ground, asbestos, a live line. | `PHAZ` |
| `Heritage Item` | 3.8 x 3.6 | Five-point star outline. A protected heritage item such as a marker stone, a post or a plaque. | `PHIA` `PHIS` `PHNI` |
| `Headstone` | 1.8 x 2.8 | Upright slab with an arched top and a small cross cut on its face. A grave headstone, shot at the middle of its base. | `PHST` |
| `Habitat Marker` | 3.4 x 3.4 | Ring holding a leaf. A habitat or protected-species marker. | `PHEC` `PHEE` |

### Miscellaneous/General

| Symbol | Size (mm) | What it looks like and when to use it | Used by |
|---|---|---|---|
| `Generic Point` | 2.0 x 2.0 | Plus. A point that no other symbol describes; the code and notes say what it is. | `PVID` |
| `Query Point` | 3.4 x 3.4 | Ring holding a question mark. A point whose identity is in doubt and must be checked in the field. | `PPOL` `PUSR` |

## The codes

632 codes by class and group. *Draws* is the linestyle or symbol, the colour and the pen weight; *Also* is what the code does besides drawing: whether its points join a surface, and the attributes it applies (in backticks) or asks the surveyor for. Where a code draws the plain continuous line, Katana's own word `continuous` is its linestyle. A field-book code is a key and a string number, `WM1 ST, WM1, WM1 END`.

### Every code: the bare `*`

The bare `*` matches every code, so it holds only what applies to any string.

| Code | Draws | Layer | Also | What it is |
|---|---|---|---|---|
| `*` | a round pipe from `Diameter (mm)` |  |  | Any string that carries a Diameter attribute is also drawn as a round pipe section, measured down from its top (the obvert). |

### Class C: Boundaries and Cadastre

Where one owner's land ends and the next begins, and the marks that prove it.

#### Lot Boundaries

Title, compiled, digital, proposed and stratum boundaries, and the lines that join them. (8 codes)

| Code | Draws | Layer | Also | What it is |
|---|---|---|---|---|
| `BC*` | line `Compiled Boundary`; `katana boundary` (#796EDA); 0.25 mm | `cadastre/lots/compiled` |  | A cadastral boundary compiled from plans and positioned to about one metre. |
| `BM*` | line `Digital Boundary`; `katana boundary` (#796EDA); 0.18 mm | `cadastre/lots/compiled` |  | A boundary taken from the digital cadastral map, whose position can be anywhere from one to twenty metres out. |
| `BN*` | line `Estimated Boundary`; `katana boundary` (#796EDA); 0.25 mm | `cadastre/lots/compiled` |  | A cadastral boundary whose position is good to about half a metre. |
| `BO*` | line `continuous`; `katana boundary` (#796EDA); 0.35 mm | `cadastre/lots/title` |  | A cadastral boundary whose position is good to about a tenth of a metre. |
| `BT*` | line `Title Boundary`; `katana boundary` (#796EDA); 0.50 mm | `cadastre/lots/title` |  | A title boundary measured on the ground and good to about two centimetres. |
| `BY*` | line `Acquisition Boundary`; `katana boundary` (#796EDA); 0.35 mm | `cadastre/lots/special` |  | A proposed boundary that marks land to be acquired. |
| `BZ*` | line `Stratum Parcel Edge`; `katana boundary` (#796EDA); 0.25 mm | `cadastre/lots/special` |  | The limit of a stratum parcel, which is fixed by a height or depth and not only by a line on the ground. |
| `JL*` | line `Survey Join Line`; `katana boundary` (#796EDA); 0.13 mm | `cadastre/lots/join` |  | A join line between two boundary points, giving a bearing and distance; it is not itself a boundary. |

#### Easements and Reserves

Land that carries another party's right or a public purpose. (1 code)

| Code | Draws | Layer | Also | What it is |
|---|---|---|---|---|
| `BE*` | line `Easement Boundary`; `katana boundary` (#796EDA); 0.25 mm | `cadastre/easements/general` |  | The limit of an easement, land that carries another party's right of use. |

#### Boundary Marks

Pegs, pins, nails and the other marks found or placed at corners, and the marks that witness them. (21 codes)

| Code | Draws | Layer | Also | What it is |
|---|---|---|---|---|
| `PBDY` | point, symbol `Iron Pin` (hidden); `katana boundary` (#796EDA); 0.25 mm | `cadastre/marks/found` |  | A boundary mark of any kind, shot as a point. |
| `PBPG` | point, symbol `Corner Peg`; `katana boundary` (#796EDA); 0.25 mm | `cadastre/marks/found` |  | A peg set at a boundary corner, shot at its top. |
| `PBPG*` | nothing |  | kept out of surfaces | Keeps boundary pegs out of the ground surface when the code carries a string number. |
| `PCRM` | point, symbol `Fixed Survey Mark` (hidden); `katana boundary` (#796EDA); 0.25 mm | `cadastre/marks/found` |  | A mark set to reference cadastral corners, so that the corners can be re-established if their own marks are lost. |
| `PDHL` | point, symbol `Winged Drill Hole`; `katana boundary` (#796EDA); 0.25 mm | `cadastre/marks/found` |  | A hole drilled in rock or concrete, with a chiselled wing cut beside it to make it easy to find. |
| `PDHL*` | nothing |  | kept out of surfaces | Keeps winged drill holes out of the ground surface when the code carries a string number. |
| `PGIN` | point, symbol `Survey Spike`; `katana boundary` (#796EDA); 0.25 mm | `cadastre/marks/found` |  | A galvanised iron nail driven into a hard surface as a survey mark. |
| `PGIN*` | nothing |  | kept out of surfaces | Keeps galvanised nails out of the ground surface when the code carries a string number. |
| `PGPI` | point, symbol `Iron Pipe`; `katana boundary` (#796EDA); 0.25 mm | `cadastre/marks/found` |  | A galvanised iron pipe driven in as a survey mark. |
| `PGPI*` | nothing |  | kept out of surfaces | Keeps galvanised pipes out of the ground surface when the code carries a string number. |
| `PMSM` | point, symbol `Fixed Survey Mark`; `katana boundary` (#796EDA); 0.25 mm | `cadastre/marks/found` |  | A survey mark of a kind no other code covers, shot at its centre. |
| `PMSM*` | nothing |  | kept out of surfaces | Keeps miscellaneous survey marks out of the ground surface. |
| `PNAL` | point, symbol `Survey Spike`; `katana boundary` (#796EDA); 0.25 mm | `cadastre/marks/found` | each point `Point Code=PNAL` | A nail driven into a hard surface as a survey mark; each shot also records its code. |
| `POCP` | point; `katana boundary` (#796EDA); 0.25 mm | `cadastre/marks/found` |  | A post standing at a boundary corner, taken as the corner mark. |
| `PPMK` | point, symbol `Fixed Survey Mark`; `katana boundary` (#796EDA); 0.25 mm | `cadastre/marks/found` |  | A permanent mark set in the ground to last, which later surveys can rely on to find a position. |
| `PRMB` | point, symbol `Corner Peg`; `katana boundary` (#796EDA); 0.25 mm | `cadastre/marks/reference` |  | A reference mark set in a concrete block, used to re-establish a survey mark if it is lost. |
| `PRMB*` | nothing |  | kept out of surfaces | Keeps concrete reference blocks out of the ground surface, since they fix a position and not the ground. |
| `PRTR` | point; `katana boundary` (#796EDA); 0.25 mm | `cadastre/marks/reference` |  | A tree used as a witness to a survey mark or a boundary corner, shown as a plain point. |
| `PSKI` | point, symbol `Survey Spike`; `katana boundary` (#796EDA); 0.25 mm | `cadastre/marks/found` |  | A spike or nail driven into a hard surface and used as a survey mark. |
| `PSTA` | point, symbol `Star Picket`; `katana boundary` (#796EDA); 0.25 mm | `cadastre/marks/found` |  | A star picket, a steel picket with a star-shaped section, driven in as a survey mark. |
| `PSTA*` | nothing |  | kept out of surfaces | Keeps star pickets out of the ground surface, since they mark a position and not the ground. |

### Class R: Roads and Pavements

The made surface: its edges, its paint and the paths beside it.

#### Carriageway

The centre, the edges and the offsets of the road itself, sealed or not. (9 codes)

| Code | Draws | Layer | Also | What it is |
|---|---|---|---|---|
| `EJ*` | line `continuous`; `katana pavement` (#789295); 0.25 mm | `roads/carriageway/edge` |  | Outer edge of the graded formation that carries a road or track, before any pavement is laid. |
| `EK*` | line `Unsealed Road Edge`; `katana pavement` (#789295); 0.18 mm | `roads/carriageway/edge` |  | Edge of a vehicle track, worn or cut into the ground and not made up as a road. |
| `EM*` | line `continuous`; `katana pavement` (#789295); 0.35 mm | `roads/carriageway/edge` | sets `Check Zone=1` | Edge of the central median that separates opposing lanes of traffic. |
| `EP*` | line `continuous`; `katana pavement` (#789295); 0.35 mm | `roads/carriageway/edge` |  | Outer edge of the sealed road surface, shot where the seal meets the shoulder or kerb. |
| `OF*` | line `Road Centreline`; `katana pavement` (#789295); 0.25 mm | `roads/carriageway/centreline` | sets `Check Zone=1` | Crown of a carriageway shot where it is offset from the road centreline, checked in model zone 1. |
| `RC*` | line `Road Centreline`; `katana pavement` (#789295); 0.18 mm | `roads/carriageway/centreline` |  | Centre of the road, shot along the middle of the carriageway. |
| `SG*` | line `continuous`; `katana pavement` (#789295); 0.25 mm | `roads/carriageway/grid` |  | Grid of bars across a road or gateway that stops stock but lets vehicles through. |
| `UR*` | line `Unsealed Road Edge`; `katana pavement` (#789295); 0.25 mm | `roads/carriageway/edge` |  | Edge of the formed surface of an unsealed road. |
| `VT*` | line `Unsealed Road Edge`; `katana pavement` (#789295); 0.18 mm | `roads/carriageway/track` |  | Edge of a track that vehicles use, formed by wheels and not built. |

#### Markings

Painted lines, arrows, crossings and chevrons, and the raised markers set in the road. (24 codes)

| Code | Draws | Layer | Also | What it is |
|---|---|---|---|---|
| `AE*` | line `Straight and Left Arrow Marking`; `katana pavement` (#789295); 0.18 mm | `roads/markings/arrow` | kept out of surfaces | A painted pavement arrow for a lane that goes straight on or turns left, shot along its axis. |
| `AI*` | line `Straight and Right Arrow Marking`; `katana pavement` (#789295); 0.18 mm | `roads/markings/arrow` | kept out of surfaces | A painted pavement arrow for a lane that goes straight on or turns right, shot along its axis. |
| `AL*` | line `Left Turn Arrow Marking`; `katana pavement` (#789295); 0.18 mm | `roads/markings/arrow` | kept out of surfaces | A painted pavement arrow for traffic that turns left, shot along its axis. |
| `AR*` | line `Right Turn Arrow Marking`; `katana pavement` (#789295); 0.18 mm | `roads/markings/arrow` | kept out of surfaces | A painted pavement arrow for traffic that turns right, shot along its axis. |
| `AS*` | line `Straight Ahead Arrow Marking`; `katana pavement` (#789295); 0.18 mm | `roads/markings/arrow` | kept out of surfaces | A painted pavement arrow for traffic that goes straight on, shot along its axis. |
| `BS*` | line `Left Broken Barrier Marking`; `katana pavement` (#789295); 0.18 mm | `roads/markings/line` |  | A painted double line with an unbroken barrier line on one side and a broken separation line on the other. |
| `CA*` | line `Crossing Approach Marking`; `katana pavement` (#789295); 0.18 mm | `roads/markings/crossing` | kept out of surfaces | The zigzag line painted along the approach to a pedestrian crossing. |
| `CC*` | line `Lane Continuity Marking`; `katana pavement` (#789295); 0.18 mm | `roads/markings/line` | sets `Check Zone=1` | A short-dash line that carries a lane through a junction, checked in check zone 1 (roads, bridges and drainage). |
| `CD*` | line `continuous`; `katana pavement` (#789295); 0.18 mm | `roads/markings/line` |  | An unbroken painted line on the pavement. |
| `CG*` | line `Give Way Marking`; `katana pavement` (#789295); 0.18 mm | `roads/markings/line` |  | The give-way line painted across a turning lane. |
| `CN*` | line `Equal Dash Marking`; `katana pavement` (#789295); 0.18 mm | `roads/markings/line` | sets `Check Zone=1` | A lane separation line with dashes and gaps of equal length, checked in check zone 1 (roads, bridges and drainage). |
| `CV*` | line `Left Chevron Marking`; `katana pavement` (#789295); 0.18 mm | `roads/markings/line` |  | A row of painted chevrons that point to the left of travel, used to fill a painted island. |
| `CW*` | line `Equal Dash Marking`; `katana pavement` (#789295); 0.18 mm | `roads/markings/line` |  | The painted edge of a clearway, drawn as dashes and gaps of equal length. |
| `CX*` | line `Signal Crossing Marking`; `katana pavement` (#789295); 0.18 mm | `roads/markings/crossing` |  | The painted lines that bound a pedestrian crossing controlled by traffic signals. |
| `CY*` | line `Right Chevron Marking`; `katana pavement` (#789295); 0.18 mm | `roads/markings/line` |  | A row of painted chevrons that point to the right of travel, used to fill a painted island. |
| `CZ*` | line `Zebra Crossing Marking`; `katana pavement` (#789295); 0.18 mm | `roads/markings/line` | kept out of surfaces | Painted zebra stripes where pedestrians have right of way across the carriageway. |
| `DA*` | line `Left Dragons Teeth Marking`; `katana pavement` (#789295); 0.18 mm | `roads/markings/line` |  | Row of painted dragon's teeth with the points standing to the left of the way the line is shot. |
| `DB*` | line `Double Barrier Marking`; `katana pavement` (#789295); 0.18 mm | `roads/markings/line` | sets `Check Zone=1` | Pair of unbroken painted lines down a road that forbids crossing in either direction. |
| `DC*` | line `Right Dragons Teeth Marking`; `katana pavement` (#789295); 0.18 mm | `roads/markings/line` |  | Row of painted dragon's teeth with the points standing to the right of the way the line is shot. |
| `LL*` | line `Dashed Lane Marking`; `katana pavement` (#789295); 0.18 mm | `roads/markings/line` |  | Lane line painted in 3 metre dashes with 9 metre gaps, shot as built. |
| `PTMX` | point, symbol `Raised Pavement Marker`; `katana pavement` (#789295); 0.25 mm | `roads/markings/stud` |  | Raised domed marker fixed to the road, called a silent cop, that guides traffic. |
| `PTMX*` | nothing |  | kept out of surfaces | Keeps silent-cop road markers out of the ground surface. |
| `SB*` | line `Right Broken Barrier Marking`; `katana pavement` (#789295); 0.18 mm | `roads/markings/line` |  | A painted double line with a broken separation line on one side and an unbroken barrier line on the other, the barrier on the right. |
| `TL*` | line `Zebra Crossing Marking`; `katana pavement` (#789295); 0.18 mm | `roads/markings/tactile` |  | Raised bars painted across the edge of a lane that rattle a tyre and warn a driver who drifts. |

#### Paths and Access

Footpath and driveway edges, and the ramps beside them. (3 codes)

| Code | Draws | Layer | Also | What it is |
|---|---|---|---|---|
| `DW*` | line `continuous`; `katana pavement` (#789295); 0.25 mm | `roads/paths/edge` |  | Edge of a driveway that gives a lot access across the footpath to the road. |
| `FP*` | line `continuous`; `katana pavement` (#789295); 0.25 mm | `roads/paths/edge` | sets `Check Zone=3` | Edge of a footpath for pedestrians beside the road. |
| `KR*` | line `continuous`; `katana pavement` (#789295); 0.25 mm | `roads/paths/ramp` |  | A pedestrian kerb ramp, or pram ramp, shot as a line along its edge. |

### Class S: Street Furniture and Signs

What people read, sit on, light their way by or steer round.

#### Signs

Sign posts and sign outlines, and the lines that carry a speed limit. (12 codes)

| Code | Draws | Layer | Also | What it is |
|---|---|---|---|---|
| `PSDS` | point, symbol `Double Sided Sign Post`; `katana street` (#A2759D); 0.25 mm | `street/signs/sign` |  | A post that carries a sign face on each of its two sides. |
| `PSIN` | point, symbol `Sign Post`; `katana street` (#A2759D); 0.25 mm | `street/signs/sign` |  | A post that carries a single sign face. |
| `S1*` | line `Hundred Speed Zone Line`; `katana street` (#A2759D); 0.25 mm | `street/signs/speed-zone` | kept out of surfaces | Stretch of road signed with a speed limit of 100 km/h; the string runs along the zone. |
| `S2*` | line `Hundred and Ten Speed Zone Line`; `katana street` (#A2759D); 0.25 mm | `street/signs/speed-zone` | kept out of surfaces | Stretch of road signed with a speed limit of 110 km/h; the string runs along the zone. |
| `S4*` | line `Forty Speed Zone Line`; `katana street` (#A2759D); 0.25 mm | `street/signs/speed-zone` | kept out of surfaces | Stretch of road signed with a speed limit of 40 km/h; the string runs along the zone. |
| `S5*` | line `Fifty Speed Zone Line`; `katana street` (#A2759D); 0.25 mm | `street/signs/speed-zone` | kept out of surfaces | Stretch of road signed with a speed limit of 50 km/h; the string runs along the zone. |
| `S6*` | line `Sixty Speed Zone Line`; `katana street` (#A2759D); 0.25 mm | `street/signs/speed-zone` | kept out of surfaces | Stretch of road signed with a speed limit of 60 km/h; the string runs along the zone. |
| `S7*` | line `Seventy Speed Zone Line`; `katana street` (#A2759D); 0.25 mm | `street/signs/speed-zone` | kept out of surfaces | Stretch of road signed with a speed limit of 70 km/h; the string runs along the zone. |
| `S8*` | line `Eighty Speed Zone Line`; `katana street` (#A2759D); 0.25 mm | `street/signs/speed-zone` | kept out of surfaces | Stretch of road signed with a speed limit of 80 km/h; the string runs along the zone. |
| `S9*` | line `Ninety Speed Zone Line`; `katana street` (#A2759D); 0.25 mm | `street/signs/speed-zone` | kept out of surfaces | Stretch of road signed with a speed limit of 90 km/h; the string runs along the zone. |
| `SI*` | line `continuous`; `katana street` (#A2759D); 0.25 mm | `street/signs/sign` |  | Plan outline of a large sign face, such as a gantry or hoarding. |
| `SX*` | line `continuous`; `katana street` (#A2759D); 0.25 mm | `street/signs/sign` | kept out of surfaces | Plan outline of a sign carried on an arm that reaches out over the road. |

#### Lighting

Street, garden and hanging lights, and the arms that reach out to them. (4 codes)

| Code | Draws | Layer | Also | What it is |
|---|---|---|---|---|
| `LI*` | line `continuous`; `katana street` (#A2759D); 0.18 mm | `street/lighting/light` | kept out of surfaces | Outreach arm of a street light, shot from the pole to the lantern; it is kept out of the ground surface. |
| `PLGN` | point, symbol `Bollard Light`; `katana street` (#A2759D); 0.25 mm | `street/lighting/light` |  | A low light standing in a garden bed or on a lawn, shot at its base. |
| `PLPL` | point, symbol `Street Light`; `katana street` (#A2759D); 0.25 mm | `street/lighting/light` |  | A pole that carries a light, shot at the centre of the pole at ground level. |
| `PLSU` | point, symbol `Hanging Light`; `katana street` (#A2759D); 0.25 mm | `street/lighting/light` |  | A light hung from an overhead wire or bracket, shot on the ground directly below it. |

#### Posts and Barriers

Bollards, guard rails, and cable and concrete barriers. (5 codes)

| Code | Draws | Layer | Also | What it is |
|---|---|---|---|---|
| `AC*` | point, symbol `Bollard`; `katana street` (#A2759D); 0.25 mm | `street/barriers/post` |  | A fixed bollard, shot as a single point and drawn with the bollard symbol. |
| `FG*` | line `Guard Rail Line`; `katana street` (#A2759D); 0.35 mm | `street/barriers/guard-rail` |  | Steel beam barrier on posts that guards the edge of a road. |
| `FW*` | line `Wire Rope Barrier Line`; `katana street` (#A2759D); 0.35 mm | `street/barriers/guard-rail` |  | Safety barrier of tensioned steel cables on posts beside or between traffic lanes. |
| `KJ*` | line `Concrete Barrier Line`; `katana street` (#A2759D); 0.35 mm | `street/barriers/guard-rail` |  | A concrete safety barrier of the F-shaped profile, shot as a line along its base. |
| `TG*` | line `Guard Rail Line`; `katana street` (#A2759D); 0.25 mm | `street/barriers/guard-rail` |  | Line along the top of a guard rail. |

#### Amenities

Bins, seats, tables, shelters, letterboxes, parking meters and fireplaces. (9 codes)

| Code | Draws | Layer | Also | What it is |
|---|---|---|---|---|
| `BH*` | line `continuous`; `katana street` (#A2759D); 0.25 mm | `street/amenities/furniture` | kept out of surfaces | The roof outline of a bus shelter, shot round its edge. |
| `BI*` | line `continuous`; symbol `Litter Bin` at every vertex; `katana street` (#A2759D); 0.25 mm | `street/amenities/furniture` |  | A large waste bin such as a skip, shot round its outline and marked with the bin symbol. |
| `PBIN` | point, symbol `Litter Bin`; `katana street` (#A2759D); 0.25 mm | `street/amenities/furniture` |  | A rubbish bin, shot at its centre. |
| `PBUS` | point, symbol `Bus Stop Sign`; `katana street` (#A2759D); 0.25 mm | `street/amenities/furniture` |  | The position of a bus stop, shot at the foot of its sign post. |
| `PFPL` | point, symbol `Fireplace`; `katana street` (#A2759D); 0.25 mm | `street/amenities/furniture` |  | A built fireplace, such as a barbecue hearth, in a park, camp or picnic area. |
| `PKME` | point, symbol `Parking Meter`; `katana street` (#A2759D); 0.25 mm | `street/amenities/furniture` |  | A kerbside meter that collects the fee for a parking bay. |
| `PMBX` | point, symbol `Letterbox`; `katana street` (#A2759D); 0.25 mm | `street/amenities/furniture` |  | A letterbox at the property frontage, shot at the middle of its base. |
| `SE*` | line `continuous`; `katana street` (#A2759D); 0.25 mm | `street/amenities/furniture` | kept out of surfaces | Plan outline of a fixed seat or bench. |
| `TA*` | line `continuous`; `katana street` (#A2759D); 0.25 mm | `street/amenities/furniture` |  | Plan outline of a picnic table. |

#### Traffic Control

Signal posts and controller cabinets, and enforcement cameras. (6 codes)

| Code | Draws | Layer | Also | What it is |
|---|---|---|---|---|
| `PCAM` | point, symbol `Roadside Camera`; `katana street` (#A2759D); 0.25 mm | `street/traffic/equipment` |  | A speed, red-light or traffic enforcement camera, shot at its pole. |
| `PCFU` | point, symbol `Camera Strobe`; `katana street` (#A2759D); 0.25 mm | `street/traffic/equipment` |  | The flash lamp that lights a vehicle for an enforcement camera, shot at its pole. |
| `PCTV` | point, symbol `Roadside Camera`; `katana street` (#A2759D); 0.25 mm | `street/traffic/equipment` |  | A closed-circuit television camera watching the road, shot at the pole that carries it. |
| `PSCL` | point, symbol `Signal Controller Cabinet`; `katana street` (#A2759D); 0.25 mm | `street/traffic/equipment` |  | The cabinet that houses the controller of a set of traffic signals. |
| `PSGL` | point, symbol `Signal Pole`; `katana street` (#A2759D); 0.25 mm | `street/traffic/equipment` |  | A traffic signal post with its lanterns, at an intersection or a crossing. |
| `TO*` | line `continuous`; `katana street` (#A2759D); 0.25 mm | `street/traffic/equipment` | kept out of surfaces | Plan outline of a signal pole together with the arm that reaches out over the road. |

### Class K: Kerbs and Drainage

Where the street meets the water: kerbs, pits, pipes and channels.

#### Kerbs

Kerb top and back, gutter lines, medians and dish drains. (6 codes)

| Code | Draws | Layer | Also | What it is |
|---|---|---|---|---|
| `BK*` | line `Kerb Back Line`; `katana kerb` (#907368); 0.18 mm | `drainage/kerbs/back` |  | The back edge of a kerb, where it meets the verge or the footpath. |
| `DD*` | line `Dish Drain Edge`; `katana kerb` (#907368); 0.25 mm | `drainage/kerbs/dish-drain` |  | Shallow V-shaped concrete drain beside a road, shot along its lowest line. |
| `FI*` | line `Gutter Lip Line`; `katana kerb` (#907368); 0.25 mm | `drainage/kerbs/gutter` | sets `Check Zone=3` | Lowest line of the gutter beside a kerb, along which stormwater runs. |
| `KB*` | line `continuous`; `katana kerb` (#907368); 0.35 mm | `drainage/kerbs/top` | sets `Check Zone=3` | Top edge of a kerb, shot on the road side and checked in model zone 3. |
| `LP*` | line `Gutter Lip Line`; `katana kerb` (#907368); 0.25 mm | `drainage/kerbs/gutter` | sets `Check Zone=1` | Lip of the gutter beside a kerb, shot along its line and checked in model zone 1. |
| `TM*` | line `continuous`; `katana kerb` (#907368); 0.35 mm | `drainage/kerbs/median` |  | Top edge of a raised median or traffic island. |

#### Pits and Structures

Gully, junction and subsoil pits, headwalls and wing walls, and the lines that outline them. (17 codes)

| Code | Draws | Layer | Also | What it is |
|---|---|---|---|---|
| `DP*` | line `continuous`; `katana stormwater` (#00A800); 0.25 mm | `drainage/structures/pit` | sets `utility.type=stormwater`; asks Owner, Depth (m), Condition | Outline of a stormwater pit, drawn at its true size on the plan. |
| `DX*` | line `continuous`; `katana stormwater` (#00A800); 0.25 mm | `drainage/structures/pit` | sets `utility.type=stormwater`; asks Owner, Depth (m), Condition | Outline of a rectangular box in the drainage system, such as a junction or inlet box. |
| `HB*` | line `continuous`; `katana stormwater` (#00A800); 0.18 mm | `drainage/structures/outlet` | sets `utility.type=stormwater`; asks Owner, Condition | Line along the foot of a headwall where it meets the ground or the pipe invert. |
| `HW*` | line `continuous`; `katana stormwater` (#00A800); 0.25 mm | `drainage/structures/outlet` | sets `utility.type=stormwater`; asks Owner, Condition | Line along the top of a headwall that surrounds the end of a pipe or culvert. |
| `IP*` | line `continuous`; `katana stormwater` (#00A800); 0.25 mm | `drainage/structures/pit` | sets `utility.type=stormwater`; asks Owner, Depth (m), Condition | Outline of a stormwater gully pit shot as a closed line instead of a single point. |
| `KI*` | line `continuous`; `katana stormwater` (#00A800); 0.25 mm | `drainage/structures/pit` | sets `utility.type=stormwater`; asks Owner, Depth (m), Condition | A kerb inlet shot as a line along the kerb: the opening where gutter water enters the stormwater system. |
| `PDJM` | point, symbol `Stormwater Manhole`; `katana stormwater` (#00A800); 0.25 mm | `drainage/structures/manhole` | sets `utility.type=stormwater`; asks Owner, Depth (m), Condition | A stormwater maintenance hole at the point where two or more drainage lines meet. |
| `PEWW` | point, symbol `Wingwall End`; `katana stormwater` (#00A800); 0.25 mm | `drainage/structures/outlet` | sets `utility.type=stormwater`; asks Owner, Condition | The free end of a wing wall that flanks a culvert or a drain outlet. |
| `PGUL` | point, symbol `Gully Pit`; `katana stormwater` (#00A800); 0.25 mm | `drainage/structures/pit` | sets `utility.type=stormwater`; asks Owner, Depth (m), Condition | A stormwater gully pit that takes surface runoff, shot at the middle of its grate. |
| `PHWB` | point, symbol `Headwall`; `katana stormwater` (#00A800); 0.25 mm | `drainage/structures/outlet` | sets `utility.type=stormwater`; asks Owner, Condition | The foot of a pipe headwall, shot where the wall meets the pipe or the ground. |
| `PHWT` | point, symbol `Headwall`; `katana stormwater` (#00A800); 0.25 mm | `drainage/structures/outlet` | sets `utility.type=stormwater`; asks Owner, Condition | The top edge of a pipe headwall, shot on its coping. |
| `PILT` | point, symbol `Sump Inlet`; `katana stormwater` (#00A800); 0.25 mm | `drainage/structures/pit` | sets `utility.type=stormwater`; asks Owner, Depth (m), Condition | The point where drainage flows into a sump, shot at the mouth of the inlet. |
| `PJBX` | point, symbol `Junction Pit`; `katana stormwater` (#00A800); 0.25 mm | `drainage/structures/pit` | sets `utility.type=stormwater`; asks Owner, Depth (m), Condition | The top of a concrete junction box on a stormwater line, where pipes meet. |
| `PSDO` | point, symbol `Invert Level`; `katana stormwater` (#00A800); 0.25 mm | `drainage/structures/outlet` | sets `utility.type=stormwater`; asks Owner, Depth (m), Condition | The invert level where a subsoil drain discharges at its outlet. |
| `PSDO*` | nothing |  | kept out of surfaces | Keeps subsoil drain outlet levels out of the ground surface, since an invert is not the ground. |
| `PSFP` | point, symbol `Subsoil Inspection Point`; `katana stormwater` (#00A800); 0.25 mm | `drainage/structures/pit` | sets `utility.type=stormwater`; asks Owner, Depth (m), Condition | The flushing point of a subsoil drain, a riser through which the pipe can be cleared. |
| `PSFP*` | nothing |  | kept out of surfaces | Keeps subsoil flushing points out of the ground surface. |

#### Pipes and Culverts

Stormwater pipes and box culverts by size, and the inverts and obverts shot in them. (55 codes)

| Code | Draws | Layer | Also | What it is |
|---|---|---|---|---|
| `B0*` | line `Culvert Outline`; `katana stormwater` (#00A800); 0.35 mm | `drainage/pipes/culvert` | kept out of surfaces; sets `utility.type=stormwater`, `Height (mm)=150`; asks Owner, Depth (m), Condition, Material, Width (mm) | A box culvert 150 mm high, drawn as its outline in plan. |
| `B1*` | line `Culvert Outline`; `katana stormwater` (#00A800); 0.35 mm | `drainage/pipes/culvert` | kept out of surfaces; sets `utility.type=stormwater`, `Height (mm)=225`; asks Owner, Depth (m), Condition, Material, Width (mm) | A box culvert 225 mm high, drawn as its outline in plan. |
| `B2*` | line `Culvert Outline`; `katana stormwater` (#00A800); 0.35 mm | `drainage/pipes/culvert` | kept out of surfaces; sets `utility.type=stormwater`, `Height (mm)=300`; asks Owner, Depth (m), Condition, Material, Width (mm) | A box culvert 300 mm high, drawn as its outline in plan. |
| `B3*` | line `Culvert Outline`; `katana stormwater` (#00A800); 0.35 mm | `drainage/pipes/culvert` | kept out of surfaces; sets `utility.type=stormwater`, `Height (mm)=375`; asks Owner, Depth (m), Condition, Material, Width (mm) | A box culvert 375 mm high, drawn as its outline in plan. |
| `B4*` | line `Culvert Outline`; `katana stormwater` (#00A800); 0.35 mm | `drainage/pipes/culvert` | kept out of surfaces; sets `utility.type=stormwater`, `Height (mm)=450`; asks Owner, Depth (m), Condition, Material, Width (mm) | A box culvert 450 mm high, drawn as its outline in plan. |
| `B6*` | line `Culvert Outline`; `katana stormwater` (#00A800); 0.35 mm | `drainage/pipes/culvert` | kept out of surfaces; sets `utility.type=stormwater`, `Height (mm)=600`; asks Owner, Depth (m), Condition, Material, Width (mm) | A box culvert 600 mm high, drawn as its outline in plan. |
| `B7*` | line `Culvert Outline`; `katana stormwater` (#00A800); 0.35 mm | `drainage/pipes/culvert` | kept out of surfaces; sets `utility.type=stormwater`, `Height (mm)=750`; asks Owner, Depth (m), Condition, Material, Width (mm) | A box culvert 750 mm high, drawn as its outline in plan. |
| `B9*` | line `Culvert Outline`; `katana stormwater` (#00A800); 0.35 mm | `drainage/pipes/culvert` | kept out of surfaces; sets `utility.type=stormwater`, `Height (mm)=900`; asks Owner, Depth (m), Condition, Material, Width (mm) | A box culvert 900 mm high, drawn as its outline in plan. |
| `BX*` | line `Culvert Outline`; `katana stormwater` (#00A800); 0.35 mm | `drainage/pipes/culvert` | sets `utility.type=stormwater`; asks Owner, Depth (m), Condition, Material, Width (mm), Height (mm) | A box culvert of any height, drawn as its outline in plan. |
| `D0*` | line `Culvert Outline`; `katana stormwater` (#00A800); 0.35 mm | `drainage/pipes/culvert` | kept out of surfaces; sets `utility.type=stormwater`, `Height (mm)=1050`; asks Owner, Depth (m), Condition, Material, Width (mm) | Rectangular box culvert with a clear opening 1050 mm high. |
| `D1*` | line `Culvert Outline`; `katana stormwater` (#00A800); 0.35 mm | `drainage/pipes/culvert` | kept out of surfaces; sets `utility.type=stormwater`, `Height (mm)=1200`; asks Owner, Depth (m), Condition, Material, Width (mm) | Rectangular box culvert with a clear opening 1200 mm high. |
| `D2*` | line `Culvert Outline`; `katana stormwater` (#00A800); 0.35 mm | `drainage/pipes/culvert` | kept out of surfaces; sets `utility.type=stormwater`, `Height (mm)=1500`; asks Owner, Depth (m), Condition, Material, Width (mm) | Rectangular box culvert with a clear opening 1500 mm high. |
| `D3*` | line `Culvert Outline`; `katana stormwater` (#00A800); 0.35 mm | `drainage/pipes/culvert` | kept out of surfaces; sets `utility.type=stormwater`, `Height (mm)=1800`; asks Owner, Depth (m), Condition, Material, Width (mm) | Rectangular box culvert with a clear opening 1800 mm high. |
| `D4*` | line `Culvert Outline`; `katana stormwater` (#00A800); 0.35 mm | `drainage/pipes/culvert` | kept out of surfaces; sets `utility.type=stormwater`, `Height (mm)=2100`; asks Owner, Depth (m), Condition, Material, Width (mm) | Rectangular box culvert with a clear opening 2100 mm high. |
| `D5*` | line `Culvert Outline`; `katana stormwater` (#00A800); 0.35 mm | `drainage/pipes/culvert` | kept out of surfaces; sets `utility.type=stormwater`, `Height (mm)=2400`; asks Owner, Depth (m), Condition, Material, Width (mm) | Rectangular box culvert with a clear opening 2400 mm high. |
| `D6*` | line `Culvert Outline`; `katana stormwater` (#00A800); 0.35 mm | `drainage/pipes/culvert` | kept out of surfaces; sets `utility.type=stormwater`, `Height (mm)=2700`; asks Owner, Depth (m), Condition, Material, Width (mm) | Rectangular box culvert with a clear opening 2700 mm high. |
| `D7*` | line `Culvert Outline`; `katana stormwater` (#00A800); 0.35 mm | `drainage/pipes/culvert` | kept out of surfaces; sets `utility.type=stormwater`, `Height (mm)=3000`; asks Owner, Depth (m), Condition, Material, Width (mm) | Rectangular box culvert with a clear opening 3000 mm high. |
| `D8*` | line `Culvert Outline`; `katana stormwater` (#00A800); 0.35 mm | `drainage/pipes/culvert` | kept out of surfaces; sets `utility.type=stormwater`, `Height (mm)=3300`; asks Owner, Depth (m), Condition, Material, Width (mm) | Rectangular box culvert with a clear opening 3300 mm high. |
| `D9*` | line `Culvert Outline`; `katana stormwater` (#00A800); 0.35 mm | `drainage/pipes/culvert` | kept out of surfaces; sets `utility.type=stormwater`, `Height (mm)=3600`; asks Owner, Depth (m), Condition, Material, Width (mm) | Rectangular box culvert with a clear opening 3600 mm high. |
| `DZ*` | line `Culvert Outline`; `katana stormwater` (#00A800); 0.25 mm | `drainage/pipes/culvert` | sets `utility.type=stormwater`; asks Owner, Depth (m), Condition, Material, Width (mm), Height (mm) | Box culvert traced from imagery and not measured on the ground. |
| `PI01` | point, symbol `Invert Level`; `katana stormwater` (#00A800); 0.18 mm | `drainage/pipes/invert-small` | sets `utility.type=stormwater`, `Diameter (mm)=225`; asks Owner, Depth (m), Condition, Material | The invert of a 225 mm stormwater pipe, shot at its lowest inside point. |
| `PI02` | point, symbol `Invert Level`; `katana stormwater` (#00A800); 0.18 mm | `drainage/pipes/invert-small` | sets `utility.type=stormwater`, `Diameter (mm)=300`; asks Owner, Depth (m), Condition, Material | The invert of a 300 mm stormwater pipe, shot at its lowest inside point. |
| `PI03` | point, symbol `Invert Level`; `katana stormwater` (#00A800); 0.18 mm | `drainage/pipes/invert-small` | sets `utility.type=stormwater`, `Diameter (mm)=375`; asks Owner, Depth (m), Condition, Material | The invert of a 375 mm stormwater pipe, shot at its lowest inside point. |
| `PI04` | point, symbol `Invert Level`; `katana stormwater` (#00A800); 0.18 mm | `drainage/pipes/invert-small` | sets `utility.type=stormwater`, `Diameter (mm)=450`; asks Owner, Depth (m), Condition, Material | The invert of a 450 mm stormwater pipe, shot at its lowest inside point. |
| `PI05` | point, symbol `Invert Level`; `katana stormwater` (#00A800); 0.18 mm | `drainage/pipes/invert-small` | sets `utility.type=stormwater`, `Diameter (mm)=525`; asks Owner, Depth (m), Condition, Material | The invert of a 525 mm stormwater pipe, shot at its lowest inside point. |
| `PI06` | point, symbol `Invert Level`; `katana stormwater` (#00A800); 0.18 mm | `drainage/pipes/invert-small` | sets `utility.type=stormwater`, `Diameter (mm)=600`; asks Owner, Depth (m), Condition, Material | The invert of a 600 mm stormwater pipe, shot at its lowest inside point. |
| `PI07` | point, symbol `Invert Level`; `katana stormwater` (#00A800); 0.18 mm | `drainage/pipes/invert-large` | sets `utility.type=stormwater`, `Diameter (mm)=750`; asks Owner, Depth (m), Condition, Material | The invert of a 750 mm stormwater pipe, shot at its lowest inside point. |
| `PI09` | point, symbol `Invert Level`; `katana stormwater` (#00A800); 0.18 mm | `drainage/pipes/invert-large` | sets `utility.type=stormwater`, `Diameter (mm)=900`; asks Owner, Depth (m), Condition, Material | The invert of a 900 mm stormwater pipe, shot at its lowest inside point. |
| `PI10` | point, symbol `Invert Level`; `katana stormwater` (#00A800); 0.18 mm | `drainage/pipes/invert-large` | sets `utility.type=stormwater`, `Diameter (mm)=1050`; asks Owner, Depth (m), Condition, Material | The invert of a 1050 mm stormwater pipe, shot at its lowest inside point. |
| `PI12` | point, symbol `Invert Level`; `katana stormwater` (#00A800); 0.18 mm | `drainage/pipes/invert-large` | sets `utility.type=stormwater`, `Diameter (mm)=1200`; asks Owner, Depth (m), Condition, Material | The invert of a 1200 mm stormwater pipe, shot at its lowest inside point. |
| `PI13` | point, symbol `Invert Level`; `katana stormwater` (#00A800); 0.18 mm | `drainage/pipes/invert-large` | sets `utility.type=stormwater`, `Diameter (mm)=1350`; asks Owner, Depth (m), Condition, Material | The invert of a 1350 mm stormwater pipe, shot at its lowest inside point. |
| `PI15` | point, symbol `Invert Level`; `katana stormwater` (#00A800); 0.18 mm | `drainage/pipes/invert-large` | sets `utility.type=stormwater`, `Diameter (mm)=1500`; asks Owner, Depth (m), Condition, Material | The invert of a 1500 mm stormwater pipe, shot at its lowest inside point. |
| `PI16` | point, symbol `Invert Level`; `katana stormwater` (#00A800); 0.18 mm | `drainage/pipes/invert-large` | sets `utility.type=stormwater`, `Diameter (mm)=1650`; asks Owner, Depth (m), Condition, Material | The invert of a 1650 mm stormwater pipe, shot at its lowest inside point. |
| `PI18` | point, symbol `Invert Level`; `katana stormwater` (#00A800); 0.18 mm | `drainage/pipes/invert-large` | sets `utility.type=stormwater`, `Diameter (mm)=1800`; asks Owner, Depth (m), Condition, Material | The invert of a 1800 mm stormwater pipe, shot at its lowest inside point. |
| `PINV` | point, symbol `Invert Level`; `katana stormwater` (#00A800); 0.18 mm | `drainage/pipes/invert-small` | sets `utility.type=stormwater`; asks Owner, Depth (m), Condition, Material, Diameter (mm) | The lowest inside point of a stormwater pipe, shot where it meets a pit; the diameter is asked for. |
| `POBV` | point, symbol `Obvert Level`; `katana stormwater` (#00A800); 0.18 mm | `drainage/pipes/pipe` | sets `utility.type=stormwater`; asks Owner, Depth (m), Condition, Material, Diameter (mm) | The obvert of a stormwater pipe, the highest point on its inside, shot at the crown. |
| `POBV*` | nothing |  | kept out of surfaces | Keeps pipe obvert shots out of the ground surface, since they lie below it. |
| `U1*` | line `Stormwater Pipe Line`; `katana stormwater` (#00A800); 0.25 mm | `drainage/pipes/pipe-small` | kept out of surfaces; sets `utility.type=stormwater`, `Diameter (mm)=225`; asks Owner, Depth (m), Condition, Material | Stormwater pipe 225 mm across, shot as a line along its route. |
| `U2*` | line `Stormwater Pipe Line`; `katana stormwater` (#00A800); 0.25 mm | `drainage/pipes/pipe-small` | kept out of surfaces; sets `utility.type=stormwater`, `Diameter (mm)=300`; asks Owner, Depth (m), Condition, Material | Stormwater pipe 300 mm across, shot as a line along its route. |
| `U3*` | line `Stormwater Pipe Line`; `katana stormwater` (#00A800); 0.25 mm | `drainage/pipes/pipe-small` | kept out of surfaces; sets `utility.type=stormwater`, `Diameter (mm)=375`; asks Owner, Depth (m), Condition, Material | Stormwater pipe 375 mm across, shot as a line along its route. |
| `U4*` | line `Stormwater Pipe Line`; `katana stormwater` (#00A800); 0.25 mm | `drainage/pipes/pipe-small` | kept out of surfaces; sets `utility.type=stormwater`, `Diameter (mm)=450`; asks Owner, Depth (m), Condition, Material | Stormwater pipe 450 mm across, shot as a line along its route. |
| `U5*` | line `Stormwater Pipe Line`; `katana stormwater` (#00A800); 0.35 mm | `drainage/pipes/pipe-small` | kept out of surfaces; sets `utility.type=stormwater`, `Diameter (mm)=525`; asks Owner, Depth (m), Condition, Material | Stormwater pipe 525 mm across, shot as a line along its route. |
| `U6*` | line `Stormwater Pipe Line`; `katana stormwater` (#00A800); 0.35 mm | `drainage/pipes/pipe-small` | kept out of surfaces; sets `utility.type=stormwater`, `Diameter (mm)=600`; asks Owner, Depth (m), Condition, Material | Stormwater pipe 600 mm across, shot as a line along its route. |
| `U7*` | line `Stormwater Pipe Line`; `katana stormwater` (#00A800); 0.35 mm | `drainage/pipes/pipe-large` | kept out of surfaces; sets `utility.type=stormwater`, `Diameter (mm)=750`; asks Owner, Depth (m), Condition, Material | Stormwater pipe 750 mm across, shot as a line along its route. |
| `U9*` | line `Stormwater Pipe Line`; `katana stormwater` (#00A800); 0.35 mm | `drainage/pipes/pipe-large` | kept out of surfaces; sets `utility.type=stormwater`, `Diameter (mm)=900`; asks Owner, Depth (m), Condition, Material | Stormwater pipe 900 mm across, shot as a line along its route. |
| `UB*` | line `Culvert Outline`; `katana stormwater` (#00A800); 0.35 mm | `drainage/pipes/culvert` | kept out of surfaces; sets `utility.type=stormwater`; asks Owner, Depth (m), Condition, Material, Width (mm), Height (mm) | Rectangular box culvert whose height was not measured. |
| `UU*` | line `Stormwater Pipe Line`; `katana stormwater` (#00A800); 0.25 mm | `drainage/pipes/pipe` | kept out of surfaces; sets `utility.type=stormwater`; asks Owner, Depth (m), Condition, Material, Diameter (mm) | Stormwater pipe whose diameter was not measured. |
| `UX*` | line `Stormwater Pipe Line`; `katana stormwater` (#00A800); 0.25 mm | `drainage/pipes/pipe` | sets `utility.type=stormwater`; asks Owner, Depth (m), Condition, Material, Diameter (mm) | Stormwater pipe of any diameter, for a pipe whose size is given in its attributes. |
| `UZ*` | line `Stormwater Pipe Line`; `katana stormwater` (#00A800); 0.18 mm | `drainage/pipes/pipe` | sets `utility.type=stormwater`; asks Owner, Depth (m), Condition, Material, Diameter (mm) | Stormwater pipe traced from imagery or plans and not located on the ground. |
| `V1*` | line `Stormwater Pipe Line`; `katana stormwater` (#00A800); 0.35 mm | `drainage/pipes/pipe-large` | kept out of surfaces; sets `utility.type=stormwater`, `Diameter (mm)=1050`; asks Owner, Depth (m), Condition, Material | Stormwater pipe 1050 mm across, shot as a line along its route. |
| `V2*` | line `Stormwater Pipe Line`; `katana stormwater` (#00A800); 0.35 mm | `drainage/pipes/pipe-large` | kept out of surfaces; sets `utility.type=stormwater`, `Diameter (mm)=1200`; asks Owner, Depth (m), Condition, Material | Stormwater pipe 1200 mm across, shot as a line along its route. |
| `V3*` | line `Stormwater Pipe Line`; `katana stormwater` (#00A800); 0.35 mm | `drainage/pipes/pipe-large` | kept out of surfaces; sets `utility.type=stormwater`, `Diameter (mm)=1350`; asks Owner, Depth (m), Condition, Material | Stormwater pipe 1350 mm across, shot as a line along its route. |
| `V5*` | line `Stormwater Pipe Line`; `katana stormwater` (#00A800); 0.35 mm | `drainage/pipes/pipe-large` | kept out of surfaces; sets `utility.type=stormwater`, `Diameter (mm)=1500`; asks Owner, Depth (m), Condition, Material | Stormwater pipe 1500 mm across, shot as a line along its route. |
| `V6*` | line `Stormwater Pipe Line`; `katana stormwater` (#00A800); 0.35 mm | `drainage/pipes/pipe-large` | kept out of surfaces; sets `utility.type=stormwater`, `Diameter (mm)=1650`; asks Owner, Depth (m), Condition, Material | Stormwater pipe 1650 mm across, shot as a line along its route. |
| `V8*` | line `Stormwater Pipe Line`; `katana stormwater` (#00A800); 0.35 mm | `drainage/pipes/pipe-large` | kept out of surfaces; sets `utility.type=stormwater`, `Diameter (mm)=1800`; asks Owner, Depth (m), Condition, Material | Stormwater pipe 1800 mm across, shot as a line along its route. |

#### Channels and Waterways

Open drains, flumes, fords and watercourses. (4 codes)

| Code | Draws | Layer | Also | What it is |
|---|---|---|---|---|
| `DF*` | line `Open Drain Invert`; `katana waterway` (#6399BE); 0.25 mm | `drainage/channels/drain` |  | Galvanised iron flume that carries water down a batter, shot along its floor. |
| `DT*` | line `Open Drain Invert`; `katana waterway` (#6399BE); 0.25 mm | `drainage/channels/drain` |  | Broad shallow channel beside the road shoulder that collects runoff, shot along its floor. |
| `FO*` | line `continuous`; `katana waterway` (#6399BE); 0.25 mm | `drainage/channels/creek` |  | Place where a road or track runs through a watercourse without a bridge or culvert. |
| `WC*` | line `Creek Centreline`; `katana waterway` (#6399BE); 0.25 mm | `drainage/channels/creek` |  | Centre of a natural watercourse. |

### Class T: Rail and Transit

Track, the lineside fittings and the platforms beside it.

#### Track

Rails, centreline, formation edge, and the boxes and troughs beside the track. (8 codes)

| Code | Draws | Layer | Also | What it is |
|---|---|---|---|---|
| `PRAL` | point, symbol `Spot Level`; `katana rail` (#816FA3); 0.35 mm | `rail/track/rail` |  | A level taken on the top of the head of a running rail. |
| `PRAL*` | nothing |  | kept out of surfaces | Keeps rail head levels out of the ground surface, so the track does not distort the terrain model. |
| `PRBX` | point, symbol `Rail Control Box`; `katana rail` (#816FA3); 0.25 mm | `rail/track/lineside` |  | A box beside the track that houses railway control equipment. |
| `PRCC` | point, symbol `Rail Cable Trough`; `katana rail` (#816FA3); 0.25 mm | `rail/track/lineside` |  | A trough beside the track that carries signalling cables, taken as a single point. |
| `PRTM` | point, symbol `Fixed Survey Mark`; `katana rail` (#816FA3); 0.25 mm | `rail/track/lineside` |  | A survey monument set beside the track as a lasting reference for its alignment. |
| `RA*` | line `Running Rail`; `katana rail` (#816FA3); 0.35 mm | `rail/track/rail` |  | Top of a running rail, shot along the middle of the rail head. |
| `RF*` | line `Track Formation Edge`; `katana rail` (#816FA3); 0.25 mm | `rail/track/formation` |  | Edge of the railway formation, the shaped earth platform that carries the ballast. |
| `RT*` | line `Track Centreline`; `katana rail` (#816FA3); 0.18 mm | `rail/track/centreline` |  | Centre of a railway track, midway between its two rails. |

#### Overhead and Signals

Signals, overhead wire stanchions and signal cable troughs. (3 codes)

| Code | Draws | Layer | Also | What it is |
|---|---|---|---|---|
| `PRSG` | point, symbol `Signal Mast`; `katana rail` (#816FA3); 0.18 mm | `rail/overhead/mast` |  | A railway signal on its mast or post beside the track. |
| `PSTR` | point, symbol `Overhead Mast`; `katana rail` (#816FA3); 0.18 mm | `rail/overhead/mast` |  | A steel stanchion beside the track that carries the overhead wires or a signal. |
| `RS*` | line `Buried Duct`; `katana rail` (#816FA3); 0.18 mm | `rail/overhead/trough` |  | Trough beside the track that carries signalling cable. |

#### Platforms

Platform edges and ramps. (2 codes)

| Code | Draws | Layer | Also | What it is |
|---|---|---|---|---|
| `RP*` | line `Platform Edge Line`; `katana rail` (#816FA3); 0.35 mm | `rail/platforms/edge` |  | Top edge of a railway platform, on the side nearest the track. |
| `RR*` | line `continuous`; `katana rail` (#816FA3); 0.25 mm | `rail/platforms/edge` |  | Edge of a ramp that rises to a railway platform. |

### Class U: Utilities

Everything buried or strung that carries a service, one family per service.

#### Water

Drinking water mains, connections, valves, hydrants, meters and pits. (16 codes)

| Code | Draws | Layer | Also | What it is |
|---|---|---|---|---|
| `PWAV` | point, symbol `Air Release Valve`; `katana water` (#2F80ED); 0.25 mm | `utilities/water/fitting` | sets `utility.type=water`; asks Owner, Depth (m), Condition | Valve at a high point of a water main that lets trapped air escape. |
| `PWCP` | point, symbol `Test Point`; `katana water` (#2F80ED); 0.25 mm | `utilities/water/fitting` | sets `utility.type=water`; asks Owner, Depth (m), Condition | Fitting on a water main where pressure or electrical readings are taken. |
| `PWEP` | point, symbol `Pumping Station`; `katana water` (#2F80ED); 0.25 mm | `utilities/water/structure` | sets `utility.type=water`; asks Owner, Condition | Pump that lifts or boosts water, shot as a point. |
| `PWET` | point, symbol `Earthing Point`; `katana water` (#2F80ED); 0.25 mm | `utilities/water/fitting` | sets `utility.type=water`; asks Owner, Depth (m), Condition | Terminal that connects a water main to an electrical earth. |
| `PWHY` | point, symbol `Fire Hydrant`; `katana water` (#2F80ED); 0.25 mm | `utilities/water/fitting` | sets `utility.type=water`; asks Owner, Condition | Hydrant on a drinking water main, used for flushing and for fire fighting. |
| `PWMM` | point, symbol `Marker Post`; `katana water` (#2F80ED); 0.25 mm | `utilities/water/fitting` | sets `utility.type=water`; asks Owner, Condition | Post or plate that shows the route of a water main. |
| `PWMR` | point, symbol `Water Flow Meter`; `katana water` (#2F80ED); 0.25 mm | `utilities/water/fitting` | sets `utility.type=water`; asks Owner, Depth (m), Condition | Meter that records the water passing through a connection. |
| `PWSR` | point, symbol `Scour Valve`; `katana water` (#2F80ED); 0.25 mm | `utilities/water/fitting` | sets `utility.type=water`; asks Owner, Depth (m), Condition | Valve at a low point of a main that empties it for cleaning or repair. |
| `PWSV` | point, symbol `Water Isolation Valve`; `katana water` (#2F80ED); 0.25 mm | `utilities/water/fitting` | sets `utility.type=water`; asks Owner, Depth (m), Condition | Valve that shuts off the flow in a water main. |
| `PWTP` | point, symbol `Tap`; `katana water` (#2F80ED); 0.25 mm | `utilities/water/fitting` | sets `utility.type=water`; asks Owner, Condition | Tap on a water supply, such as a standpipe or a garden tap. |
| `RE*` | line `continuous`; `katana water` (#2F80ED); 0.25 mm | `utilities/water/structure` | sets `utility.type=water`; asks Owner, Condition | Outline of a water storage reservoir, traced round its walls. |
| `UO*` | line `Water Supply Pipe`; `katana water` (#2F80ED); 0.25 mm | `utilities/water/overhead` | kept out of surfaces; sets `utility.type=water`; asks Owner, Height (m), Condition, Material, Diameter (mm) | Water pipeline that runs above ground on supports. |
| `WI*` | line `Water Supply Pipe`; `katana water` (#2F80ED); 0.18 mm | `utilities/water/irrigation` | sets `utility.type=water`; asks Owner, Depth (m), Condition, Material, Diameter (mm) | Pipe that carries water for irrigation, shot as a line along its route. |
| `WM*` | line `Water Supply Pipe`; `katana water` (#2F80ED); 0.35 mm | `utilities/water/main` | sets `utility.type=water`; asks Owner, Depth (m), Condition, Material, Diameter (mm) | Water main, shot as a line along its route. |
| `WY*` | line `Service Connection Line`; `katana water` (#2F80ED); 0.18 mm | `utilities/water/connection` | sets `utility.type=water`; asks Owner, Depth (m), Condition | Small pipe that joins a property to the water main, shot as a line along its route. |
| `WZ*` | line `Water Supply Pipe`; `katana water` (#2F80ED); 0.18 mm | `utilities/water/main` | sets `utility.type=water`; asks Owner, Depth (m), Condition, Material, Diameter (mm) | Water main traced from imagery or plans and not located on the ground. |

#### Sewer

Gravity and pressure sewers, connections, maintenance holes, vents and pump stations. (13 codes)

| Code | Draws | Layer | Also | What it is |
|---|---|---|---|---|
| `PSLH` | point, symbol `Sewer Inspection Opening`; `katana sewer` (#F0E2B0); 0.25 mm | `utilities/sewer/structure` | sets `utility.type=sewer`; asks Owner, Depth (m), Condition | A lamphole: a small opening in a sewer for looking along the pipe with a lamp. |
| `PSMH` | point, symbol `Sewer Maintenance Hole`; `katana sewer` (#F0E2B0); 0.25 mm | `utilities/sewer/structure` | sets `utility.type=sewer`; asks Owner, Depth (m), Condition | The cover of a sewer maintenance hole, taken at the centre of the lid. |
| `PSMM` | point, symbol `Marker Post`; `katana sewer` (#F0E2B0); 0.25 mm | `utilities/sewer/fitting` | sets `utility.type=sewer`; asks Owner, Depth (m), Condition | A marker that shows where a sewer main runs. |
| `PSPS` | point, symbol `Pumping Station`; `katana sewer` (#F0E2B0); 0.25 mm | `utilities/sewer/structure` | sets `utility.type=sewer`; asks Owner, Depth (m), Condition | A pumping station on a sewer, taken at the lid of the wet well. |
| `PSSV` | point, symbol `Sewer Isolation Valve`; `katana sewer` (#F0E2B0); 0.25 mm | `utilities/sewer/fitting` | sets `utility.type=sewer`; asks Owner, Depth (m), Condition | A stop valve that shuts off flow in a sewer line, taken at its cover. |
| `PSVP` | point, symbol `Sewer Vent`; `katana sewer` (#F0E2B0); 0.25 mm | `utilities/sewer/fitting` | sets `utility.type=sewer`; asks Owner, Depth (m), Condition | A vent pipe that lets air into a sewer and gas out of it. |
| `SM*` | line `Sewer Gravity Pipe`; `katana sewer` (#F0E2B0); 0.35 mm | `utilities/sewer/main` | sets `utility.type=sewer`; asks Owner, Depth (m), Condition, Material, Diameter (mm) | Gravity sewer main, shot as a line along its route. |
| `SP*` | line `continuous`; `katana sewer` (#F0E2B0); 0.25 mm | `utilities/sewer/structure` | sets `utility.type=sewer`; asks Owner, Condition | Outline of a sewage pond or lagoon. |
| `SQ*` | line `continuous`; `katana sewer` (#F0E2B0); 0.25 mm | `utilities/sewer/structure` | sets `utility.type=sewer`; asks Owner, Condition | Outline of a station that pumps sewage or water out of a low point. |
| `SR*` | line `Sewer Pressure Pipe`; `katana sewer` (#F0E2B0); 0.35 mm | `utilities/sewer/main` | sets `utility.type=sewer`; asks Owner, Depth (m), Condition, Material, Diameter (mm) | Pressure sewer, the rising main that carries pumped flow uphill, shot as a line along its route. |
| `ST*` | line `continuous`; `katana sewer` (#F0E2B0); 0.25 mm | `utilities/sewer/structure` | sets `utility.type=sewer`; asks Owner, Condition | Outline of a septic tank. |
| `SY*` | line `Service Connection Line`; `katana sewer` (#F0E2B0); 0.18 mm | `utilities/sewer/connection` | sets `utility.type=sewer`; asks Owner, Depth (m), Condition | Small branch that joins a property to the sewer main, shot as a line along its route. |
| `SZ*` | line `Sewer Gravity Pipe`; `katana sewer` (#F0E2B0); 0.18 mm | `utilities/sewer/main` | sets `utility.type=sewer`; asks Owner, Depth (m), Condition, Material, Diameter (mm) | Gravity sewer traced from imagery or plans and not located on the ground. |

#### Gas

Gas mains, regulators, meters, valve pits, vents and markers. (15 codes)

| Code | Draws | Layer | Also | What it is |
|---|---|---|---|---|
| `DG*` | line `Service Connection Line`; `katana gas` (#FFD500); 0.18 mm | `utilities/gas/main` | sets `utility.type=gas`; asks Owner, Depth (m), Condition | Small gas pipe that runs from the street main to a property. |
| `HA*` | line `Gas Supply Pipe`; `katana gas` (#FFD500); 0.35 mm | `utilities/gas/main` | sets `utility.type=gas`; asks Owner, Depth (m), Condition, Material, Diameter (mm) | Buried pipeline that carries ethane. |
| `HG*` | line `Gas Supply Pipe`; `katana gas` (#FFD500); 0.35 mm | `utilities/gas/main` | sets `utility.type=gas`; asks Owner, Depth (m), Condition, Material, Diameter (mm) | Gas main that works at high pressure. |
| `LG*` | line `Gas Supply Pipe`; `katana gas` (#FFD500); 0.35 mm | `utilities/gas/main` | sets `utility.type=gas`; asks Owner, Depth (m), Condition, Material, Diameter (mm) | A buried gas main that works at low pressure, shot as a line along its route. |
| `NG*` | line `Gas Supply Pipe`; `katana gas` (#FFD500); 0.35 mm | `utilities/gas/main` | sets `utility.type=gas`; asks Owner, Depth (m), Condition, Material, Diameter (mm) | A buried gas main laid in nylon pipe, shot as a line along its route. |
| `PGAS` | point, symbol `Gas Pit`; `katana gas` (#FFD500); 0.25 mm | `utilities/gas/structure` | sets `utility.type=gas`; asks Owner, Depth (m), Condition | The box over a gas valve, shot at its lid. |
| `PGHL` | point, symbol `Gas Maintenance Hole`; `katana gas` (#FFD500); 0.25 mm | `utilities/gas/structure` | sets `utility.type=gas`; asks Owner, Depth (m), Condition | A manhole giving access to a gas main, shot at its cover. |
| `PGHM` | point, symbol `High Pressure Marker Post`; `katana gas` (#FFD500); 0.25 mm | `utilities/gas/fitting` | sets `utility.type=gas`; asks Owner, Condition | A marker on the surface showing the route of a high-pressure gas main. |
| `PGMR` | point, symbol `Gas Flow Meter`; `katana gas` (#FFD500); 0.25 mm | `utilities/gas/fitting` | sets `utility.type=gas`; asks Owner, Depth (m), Condition | A gas meter, shot at its centre. |
| `PGPM` | point, symbol `Marker Post`; `katana gas` (#FFD500); 0.25 mm | `utilities/gas/fitting` | sets `utility.type=gas`; asks Owner, Condition | A marker on the surface showing the route of a buried gas main. |
| `PGRB` | point, symbol `Gas Pressure Regulator`; `katana gas` (#FFD500); 0.25 mm | `utilities/gas/fitting` | sets `utility.type=gas`; asks Owner, Condition | The cabinet that houses a gas pressure regulator, shot at the middle of the box. |
| `PGTP` | point, symbol `Test Point`; `katana gas` (#FFD500); 0.25 mm | `utilities/gas/fitting` | sets `utility.type=gas`; asks Owner, Condition | A surface fitting on a gas pipeline where its protection or pressure can be tested. |
| `PGVP` | point, symbol `Gas Vent`; `katana gas` (#FFD500); 0.25 mm | `utilities/gas/fitting` | sets `utility.type=gas`; asks Owner, Condition | A vent riser on a gas main or casing that lets gas escape safely to the open air. |
| `YG*` | line `Gas Supply Pipe`; `katana gas` (#FFD500); 0.25 mm | `utilities/gas/main` | sets `utility.type=gas`; asks Owner, Depth (m), Condition, Material, Diameter (mm) | Polyethylene gas main, shot as a line along its route. |
| `ZG*` | line `Gas Supply Pipe`; `katana gas` (#FFD500); 0.18 mm | `utilities/gas/main` | sets `utility.type=gas`; asks Owner, Depth (m), Condition, Material, Diameter (mm) | Gas main traced from imagery or plans and not located on the ground. |

#### Electricity

Cables, overhead lines, ducts, poles, transformers, pillars and pits. (23 codes)

| Code | Draws | Layer | Also | What it is |
|---|---|---|---|---|
| `EA*` | line `Underground Cable`; `katana electricity` (#E07000); 0.18 mm | `utilities/electricity/main` | sets `utility.type=electricity`; asks Owner, Depth (m), Condition | Buried conductor that bonds electrical equipment to the earth. |
| `EC*` | line `continuous`; `katana electricity` (#E07000); 0.25 mm | `utilities/electricity/structure` | sets `utility.type=electricity`; asks Owner, Condition | Plan outline of the cabinet that houses a transformer. |
| `ED*` | line `Buried Duct`; `katana electricity` (#E07000); 0.25 mm | `utilities/electricity/main` | sets `utility.type=electricity`; asks Owner, Depth (m), Condition | Buried conduit that carries electrical cable. |
| `EL*` | line `continuous`; `katana electricity` (#E07000); 0.25 mm | `utilities/electricity/structure` | sets `utility.type=electricity`; asks Owner, Condition | Outline of the footing under one leg of a transmission pylon. |
| `EN*` | line `continuous`; `katana electricity` (#E07000); 0.25 mm | `utilities/electricity/structure` | sets `utility.type=electricity`; asks Owner, Depth (m), Condition | Outline of a sump that collects water or oil at an electricity installation. |
| `EU*` | line `Underground Cable`; `katana electricity` (#E07000); 0.35 mm | `utilities/electricity/main` | sets `utility.type=electricity`; asks Owner, Depth (m), Condition | Buried electricity line, located by locator or from records. |
| `EY*` | line `Service Connection Line`; `katana electricity` (#E07000); 0.18 mm | `utilities/electricity/main` | kept out of surfaces; sets `utility.type=electricity`; asks Owner, Depth (m), Condition | Small electricity cable from the street network to a property. |
| `EZ*` | line `Underground Cable`; `katana electricity` (#E07000); 0.18 mm | `utilities/electricity/main` | sets `utility.type=electricity`; asks Owner, Depth (m), Condition | Electricity line traced from imagery or plans and not located on the ground. |
| `PECM` | point, symbol `Marker Post`; `katana electricity` (#E07000); 0.25 mm | `utilities/electricity/fitting` | sets `utility.type=electricity`; asks Owner, Condition | A marker on the surface showing the route of a buried power cable. |
| `PEFP` | point, symbol `Electrical Fuse`; `katana electricity` (#E07000); 0.25 mm | `utilities/electricity/fitting` | sets `utility.type=electricity`; asks Owner, Condition | A point where a fuse protects a distribution cable, on a pole or in a cabinet. |
| `PEJB` | point, symbol `Electrical Pit`; `katana electricity` (#E07000); 0.25 mm | `utilities/electricity/structure` | sets `utility.type=electricity`; asks Owner, Depth (m), Condition | A box in which electricity cables are joined, shot at its lid. |
| `PEMH` | point, symbol `Electrical Maintenance Hole`; `katana electricity` (#E07000); 0.25 mm | `utilities/electricity/structure` | sets `utility.type=electricity`; asks Owner, Depth (m), Condition | A manhole giving access to underground electricity cables, shot at its cover. |
| `PETC` | point, symbol `Transformer`; `katana electricity` (#E07000); 0.25 mm | `utilities/electricity/structure` | sets `utility.type=electricity`; asks Owner, Condition | The centre of a ground-mounted cabinet that houses an electricity transformer. |
| `PEUP` | point, symbol `Electrical Pillar`; `katana electricity` (#E07000); 0.25 mm | `utilities/electricity/structure` | sets `utility.type=electricity`; asks Owner, Condition | A pillar above ground where a customer is supplied from the underground power cable. |
| `PHTT` | point, symbol `Transmission Pylon`; `katana electricity` (#E07000); 0.35 mm | `utilities/electricity/structure` | sets `utility.type=electricity`; asks Owner, Condition | A steel lattice tower that carries high-voltage lines, shot at the centre of its footprint. |
| `PPLP` | point, symbol `Power and Light Pole`; `katana electricity` (#E07000); 0.25 mm | `utilities/electricity/structure` | sets `utility.type=electricity`; asks Owner, Depth (m), Condition | A pole that carries power lines and also a street light. |
| `PPPL` | point, symbol `Power Pole`; `katana electricity` (#E07000); 0.25 mm | `utilities/electricity/structure` | sets `utility.type=electricity`; asks Owner, Depth (m), Condition | A pole that carries power lines and nothing else. |
| `PPTR` | point, symbol `Transformer`; `katana electricity` (#E07000); 0.25 mm | `utilities/electricity/structure` | sets `utility.type=electricity`; asks Owner, Depth (m), Condition | A pole that carries power lines and has a transformer mounted on it. |
| `PSAP` | point, symbol `Guy Anchor`; `katana electricity` (#E07000); 0.25 mm | `utilities/electricity/fitting` | sets `utility.type=electricity`; asks Owner, Depth (m), Condition | The ground anchor of the stay wire that braces a pole. |
| `PSPL` | point, symbol `Stay Pole`; `katana electricity` (#E07000); 0.25 mm | `utilities/electricity/structure` | sets `utility.type=electricity`; asks Owner, Depth (m), Condition | A short pole that carries the stay wire bracing another pole. |
| `SC*` | line `Overhead Line`; `katana electricity` (#E07000); 0.18 mm | `utilities/electricity/overhead` | kept out of surfaces; sets `utility.type=electricity`; asks Owner, Condition | Stay wire that braces a pole or mast, drawn from the pole to its ground anchor. |
| `UE*` | line `Overhead Line`; `katana electricity` (#E07000); 0.35 mm | `utilities/electricity/overhead` | kept out of surfaces; sets `utility.type=electricity`; asks Owner, Height (m), Condition | Main overhead power line that carries bulk supply between substations. |
| `UL*` | line `Overhead Line`; `katana electricity` (#E07000); 0.25 mm | `utilities/electricity/overhead` | kept out of surfaces; sets `utility.type=electricity`; asks Owner, Height (m), Condition | Overhead power line of the local network, below the major transmission lines. |

#### Communications

Telephone, data and fibre cables, ducts, pits, pillars, poles and masts. (23 codes)

| Code | Draws | Layer | Also | What it is |
|---|---|---|---|---|
| `OA*` | line `Overhead Line`; `katana telecom` (#FFFFFF); 0.18 mm | `utilities/telecommunications/fibre` | kept out of surfaces; sets `utility.type=telecommunications`; asks Owner, Height (m), Condition | Optical fibre cable strung above ground on poles, shot as a line along its route. |
| `OD*` | line `Buried Duct`; `katana telecom` (#FFFFFF); 0.25 mm | `utilities/telecommunications/fibre` | sets `utility.type=telecommunications`; asks Owner, Depth (m), Condition, Material, Diameter (mm) | A conduit that carries optical fibre cable, shot as a line along its route. |
| `OU*` | line `Communications Cable`; `katana telecom` (#FFFFFF); 0.35 mm | `utilities/telecommunications/fibre` | sets `utility.type=telecommunications`; asks Owner, Depth (m), Condition | Optical fibre cable buried in the ground, shot as a line along its route. |
| `OZ*` | line `Communications Cable`; `katana telecom` (#FFFFFF); 0.25 mm | `utilities/telecommunications/fibre` | sets `utility.type=telecommunications`; asks Owner, Depth (m), Condition | An optical fibre route digitised from records and not located on site. |
| `PMPT` | point, symbol `Mast`; `katana telecom` (#FFFFFF); 0.25 mm | `utilities/telecommunications/structure` | sets `utility.type=telecommunications`; asks Owner, Condition | A transmitter mast or tower of the mobile telephone network, shot at its base. |
| `POFJ` | point, symbol `Communications Pillar`; `katana telecom` (#FFFFFF); 0.25 mm | `utilities/telecommunications/structure` | sets `utility.type=telecommunications`; asks Owner, Condition | A junction box on an optical fibre route where cables are joined or split. |
| `POFM` | point, symbol `Marker Post`; `katana telecom` (#FFFFFF); 0.25 mm | `utilities/telecommunications/structure` | sets `utility.type=telecommunications`; asks Owner, Condition | A marker that shows where an optical fibre cable runs underground. |
| `POFP` | point, symbol `Communications Pit`; `katana telecom` (#FFFFFF); 0.25 mm | `utilities/telecommunications/structure` | sets `utility.type=telecommunications`; asks Owner, Depth (m), Condition | A pit on an optical fibre route, shot at the middle of its lid. |
| `PT3P` | point, symbol `Communications Triple Pit`; `katana telecom` (#FFFFFF); 0.25 mm | `utilities/telecommunications/structure` | sets `utility.type=telecommunications`; asks Owner, Depth (m), Condition | Concrete telephone pit with three lids in a row, where cable is joined and drawn in. |
| `PTBX` | point, symbol `Telephone Box`; `katana telecom` (#FFFFFF); 0.25 mm | `utilities/telecommunications/structure` | sets `utility.type=telecommunications`; asks Owner, Condition | Public telephone box or booth standing on the footpath. |
| `PTCM` | point, symbol `Marker Post`; `katana telecom` (#FFFFFF); 0.25 mm | `utilities/telecommunications/fitting` | sets `utility.type=telecommunications`; asks Owner, Condition | Marker that shows the route of a buried telephone cable. |
| `PTDP` | point, symbol `Communications Pillar`; `katana telecom` (#FFFFFF); 0.25 mm | `utilities/telecommunications/structure` | sets `utility.type=telecommunications`; asks Owner, Condition | Street cabinet from which telephone cable is distributed to nearby customers. |
| `PTJP` | point, symbol `Marker Post`; `katana telecom` (#FFFFFF); 0.25 mm | `utilities/telecommunications/fitting` | sets `utility.type=telecommunications`; asks Owner, Condition | Post standing above ground at a joint in a telephone cable. |
| `PTMP` | point, symbol `Communications Main Pit`; `katana telecom` (#FFFFFF); 0.25 mm | `utilities/telecommunications/structure` | sets `utility.type=telecommunications`; asks Owner, Depth (m), Condition | Large main pit, about 1.1 m square, at a major telephone cable junction. |
| `PTPL` | point, symbol `Power Pole`; `katana telecom` (#FFFFFF); 0.25 mm | `utilities/telecommunications/overhead` | sets `utility.type=telecommunications`; asks Owner, Condition | Pole that carries telephone cable overhead. |
| `PTSP` | point, symbol `Communications Pit`; `katana telecom` (#FFFFFF); 0.25 mm | `utilities/telecommunications/structure` | sets `utility.type=telecommunications`; asks Owner, Depth (m), Condition | Concrete telephone pit with a single lid. |
| `PTTP` | point, symbol `Communications Twin Pit`; `katana telecom` (#FFFFFF); 0.25 mm | `utilities/telecommunications/structure` | sets `utility.type=telecommunications`; asks Owner, Depth (m), Condition | Concrete telephone pit with two lids side by side. |
| `TD*` | line `Buried Duct`; `katana telecom` (#FFFFFF); 0.25 mm | `utilities/telecommunications/duct` | sets `utility.type=telecommunications`; asks Owner, Depth (m), Condition | Conduit that carries telephone cable, shot as a line along its route. |
| `TN*` | line `Communications Cable`; `katana telecom` (#FFFFFF); 0.25 mm | `utilities/telecommunications/copper` | sets `utility.type=telecommunications`; asks Owner, Depth (m), Condition | Buried telephone line, shot as a line along its route. |
| `TS*` | line `continuous`; `katana telecom` (#FFFFFF); 0.25 mm | `utilities/telecommunications/structure` | sets `Check Zone=3` | Outline of a telephone sump, checked in the third model zone (road side). |
| `TX*` | line `continuous`; `katana telecom` (#FFFFFF); 0.25 mm | `utilities/telecommunications/structure` | sets `utility.type=telecommunications`; asks Owner, Condition | Plan outline of a public telephone box. |
| `TY*` | line `Service Connection Line`; `katana telecom` (#FFFFFF); 0.18 mm | `utilities/telecommunications/connection` | kept out of surfaces; sets `utility.type=telecommunications`; asks Owner, Depth (m), Condition | Small cable that joins a property to the telephone network, shot as a line along its route. |
| `TZ*` | line `Communications Cable`; `katana telecom` (#FFFFFF); 0.18 mm | `utilities/telecommunications/copper` | sets `utility.type=telecommunications`; asks Owner, Depth (m), Condition | Telephone line traced from imagery or plans and not located on the ground. |

#### Recycled Water

Recycled water mains, valves, meters, hydrants and markers. (8 codes)

| Code | Draws | Layer | Also | What it is |
|---|---|---|---|---|
| `PRAV` | point, symbol `Air Release Valve`; `katana recycled` (#9B59D0); 0.25 mm | `utilities/recycled-water/fitting` | sets `utility.type=recycled-water`; asks Owner, Depth (m), Condition | An air valve on a recycled water main, which lets trapped air out at a high point. |
| `PRET` | point, symbol `Earthing Point`; `katana recycled` (#9B59D0); 0.25 mm | `utilities/recycled-water/fitting` | sets `utility.type=recycled-water`; asks Owner, Depth (m), Condition | An earth terminal on a recycled water main, the point where the main is bonded to earth. |
| `PRHY` | point, symbol `Fire Hydrant`; `katana recycled` (#9B59D0); 0.25 mm | `utilities/recycled-water/fitting` | sets `utility.type=recycled-water`; asks Owner, Depth (m), Condition | A hydrant on a recycled water main. |
| `PRMM` | point, symbol `Marker Post`; `katana recycled` (#9B59D0); 0.25 mm | `utilities/recycled-water/fitting` | sets `utility.type=recycled-water`; asks Owner, Depth (m), Condition | A marker that shows where a recycled water main runs. |
| `PRMR` | point, symbol `Recycled Flow Meter`; `katana recycled` (#9B59D0); 0.25 mm | `utilities/recycled-water/fitting` | sets `utility.type=recycled-water`; asks Owner, Depth (m), Condition | A meter on a recycled water supply. |
| `PRSV` | point, symbol `Recycled Isolation Valve`; `katana recycled` (#9B59D0); 0.25 mm | `utilities/recycled-water/fitting` | sets `utility.type=recycled-water`; asks Owner, Depth (m), Condition | A stop valve that shuts off a length of recycled water main. |
| `PRTP` | point, symbol `Tap`; `katana recycled` (#9B59D0); 0.25 mm | `utilities/recycled-water/fitting` | sets `utility.type=recycled-water`; asks Owner, Depth (m), Condition | A tap on a recycled water supply, such as an outlet or a standpipe. |
| `RM*` | line `Recycled Water Pipe`; `katana recycled` (#9B59D0); 0.35 mm | `utilities/recycled-water/main` | sets `utility.type=recycled-water`; asks Owner, Depth (m), Condition, Material, Diameter (mm) | Main that carries recycled water, shot along its centre. |

#### Fire Service

Fire hydrants. (1 code)

| Code | Draws | Layer | Also | What it is |
|---|---|---|---|---|
| `PWFB` | point, symbol `Fire Hydrant`; `katana fire` (#E53935); 0.25 mm | `utilities/fire-service/fitting` | sets `utility.type=fire-service`; asks Owner, Condition | Hydrant on a fire service main, for the fire brigade's hoses. |

#### Fuel

Fuel pipelines, markers, dispensers and tanks. (7 codes)

| Code | Draws | Layer | Also | What it is |
|---|---|---|---|---|
| `FU*` | line `Fuel Supply Pipe`; `katana fuel` (#A36215); 0.35 mm | `utilities/fuel/main` | sets `utility.type=fuel`; asks Owner, Depth (m), Condition, Material, Diameter (mm) | Buried pipeline for a fuel that has no code of its own. |
| `FZ*` | line `Fuel Supply Pipe`; `katana fuel` (#A36215); 0.18 mm | `utilities/fuel/main` | sets `utility.type=fuel`; asks Owner, Depth (m), Condition, Material, Diameter (mm) | Fuel pipeline of an unlisted kind, traced from imagery and not located on the ground. |
| `HO*` | line `Fuel Supply Pipe`; `katana fuel` (#A36215); 0.35 mm | `utilities/fuel/main` | sets `utility.type=fuel`; asks Owner, Depth (m), Condition, Material, Diameter (mm) | Pipeline that carries oil at high pressure. |
| `HZ*` | line `Fuel Supply Pipe`; `katana fuel` (#A36215); 0.18 mm | `utilities/fuel/main` | sets `utility.type=fuel`; asks Owner, Depth (m), Condition, Material, Diameter (mm) | High pressure oil pipeline traced from imagery and not located on the ground. |
| `PFPM` | point, symbol `Marker Post`; `katana fuel` (#A36215); 0.25 mm | `utilities/fuel/fitting` | sets `utility.type=fuel`; asks Owner, Condition | A marker on the surface showing the route of a buried fuel pipeline. |
| `POHM` | point, symbol `Marker Post`; `katana fuel` (#A36215); 0.25 mm | `utilities/fuel/fitting` | sets `utility.type=fuel`; asks Owner, Condition | A sign or post that marks the route of a high-pressure oil pipeline. |
| `PPET` | point, symbol `Petrol Pump`; `katana fuel` (#A36215); 0.25 mm | `utilities/fuel/structure` | sets `utility.type=fuel`; asks Owner, Condition | A petrol pump at a service station, shot at the middle of its base. |

#### Traffic Systems

Cables, detectors and junction boxes for signals and cameras. (4 codes)

| Code | Draws | Layer | Also | What it is |
|---|---|---|---|---|
| `IT*` | line `Traffic Systems Cable`; `katana its` (#0B9BA8); 0.25 mm | `utilities/its/main` | sets `utility.type=its`; asks Owner, Depth (m), Condition | A cable of a road traffic management system, such as signals or cameras, shot as a line along its route. |
| `PSDR` | point, symbol `Loop Detector`; `katana its` (#0B9BA8); 0.25 mm | `utilities/its/fitting` | sets `utility.type=its`; asks Owner, Depth (m), Condition | A detector for a set of traffic signals, such as a vehicle loop in the road. |
| `PSJX` | point, symbol `Traffic Systems Pit`; `katana its` (#0B9BA8); 0.25 mm | `utilities/its/structure` | sets `utility.type=its`; asks Owner, Depth (m), Condition | A junction box in the ground where traffic signal cables are joined. |
| `SD*` | line `Traffic Systems Cable`; `katana its` (#0B9BA8); 0.18 mm | `utilities/its/loop` | sets `utility.type=its`; asks Owner, Depth (m), Condition | Loop cut into the road surface that detects vehicles for a traffic signal, drawn along the loop. |

#### Unknown Services

What is there and has not been named: lines, pits, covers and poles. (8 codes)

| Code | Draws | Layer | Also | What it is |
|---|---|---|---|---|
| `CB*` | line `Hidden Outline`; `katana unknown` (#F2559B); 0.13 mm | `utilities/unknown/structure` | sets `utility.type=unknown`; asks Owner, Depth (m), Condition | The bottom outline of an underground utility chamber, drawn as a hidden edge. |
| `CT*` | line `continuous`; `katana unknown` (#F2559B); 0.25 mm | `utilities/unknown/structure` | sets `utility.type=unknown`; asks Owner, Depth (m), Condition | The top outline of an underground utility chamber. |
| `PGAT` | point, symbol `Unknown Pit`; `katana unknown` (#F2559B); 0.25 mm | `utilities/unknown/structure` | sets `utility.type=unknown`; asks Owner, Depth (m), Condition | The lid of a recessed access cover that serves a service nobody has identified. |
| `PPOL` | point, symbol `Query Point`; `katana unknown` (#F2559B); 0.25 mm | `utilities/unknown/structure` | sets `utility.type=unknown`; asks Owner, Depth (m), Condition | A pole that has not been identified as power, telephone or lighting. |
| `PUSR` | point, symbol `Query Point`; `katana unknown` (#F2559B); 0.25 mm | `utilities/unknown/structure` | sets `utility.type=unknown`; asks Owner, Depth (m), Condition | Surface sign of a service that has not been identified, such as an unmarked cover or stub. |
| `PUTP` | point, symbol `Storage Tank`; `katana unknown` (#F2559B); 0.25 mm | `utilities/unknown/structure` | sets `utility.type=unknown`; asks Owner, Depth (m), Condition | Buried tank whose contents are not known, shot at its lid or filler. |
| `UP*` | line `Unknown Service Line`; `katana unknown` (#F2559B); 0.25 mm | `utilities/unknown/line` | sets `utility.type=unknown`; asks Owner, Depth (m), Condition, Material, Diameter (mm) | Pipeline whose service has not been identified. |
| `US*` | line `continuous`; `katana unknown` (#F2559B); 0.25 mm | `utilities/unknown/structure` | sets `utility.type=unknown`; asks Owner, Condition | Plan outline of a box where service lines are joined, whose service is not known. |

### Class V: Vegetation and Landscape

Trees and the edges of planted and natural ground cover.

#### Trees

Trunks by diameter, canopies by spread, and outlines of foliage. (51 codes)

| Code | Draws | Layer | Also | What it is |
|---|---|---|---|---|
| `PF01` | point, symbol `Tiny Tree Canopy`; `katana tree` (#5C8627); 0.18 mm | `vegetation/trees/canopy` |  | A tree whose crown spreads about one metre across, drawn as a canopy ring of that size. |
| `PF01*` | nothing |  | kept out of surfaces | Keeps the one-metre canopy marks out of the ground surface when the code carries a string number. |
| `PF02` | point, symbol `Tiny Tree Canopy`; `katana tree` (#5C8627); 0.18 mm | `vegetation/trees/canopy` |  | A tree whose crown spreads about two metres across, drawn as a canopy ring of that size. |
| `PF02*` | nothing |  | kept out of surfaces | Keeps the two-metre canopy marks out of the ground surface when the code carries a string number. |
| `PF03` | point, symbol `Small Tree Canopy`; `katana tree` (#5C8627); 0.18 mm | `vegetation/trees/canopy` |  | A tree whose crown spreads about three metres across, drawn as a canopy ring of that size. |
| `PF03*` | nothing |  | kept out of surfaces | Keeps the three-metre canopy marks out of the ground surface when the code carries a string number. |
| `PF04` | point, symbol `Small Tree Canopy`; `katana tree` (#5C8627); 0.18 mm | `vegetation/trees/canopy` |  | A tree whose crown spreads about four metres across, drawn as a canopy ring of that size. |
| `PF04*` | nothing |  | kept out of surfaces | Keeps the four-metre canopy marks out of the ground surface when the code carries a string number. |
| `PF05` | point, symbol `Medium Tree Canopy`; `katana tree` (#5C8627); 0.18 mm | `vegetation/trees/canopy` |  | A tree whose crown spreads about five metres across, drawn as a canopy ring of that size. |
| `PF05*` | nothing |  | kept out of surfaces | Keeps the five-metre canopy marks out of the ground surface when the code carries a string number. |
| `PF06` | point, symbol `Medium Tree Canopy`; `katana tree` (#5C8627); 0.18 mm | `vegetation/trees/canopy` |  | A tree whose crown spreads about six metres across, drawn as a canopy ring of that size. |
| `PF06*` | nothing |  | kept out of surfaces | Keeps the six-metre canopy marks out of the ground surface when the code carries a string number. |
| `PF07` | point, symbol `Large Tree Canopy`; `katana tree` (#5C8627); 0.18 mm | `vegetation/trees/canopy` |  | A tree whose crown spreads about seven metres across, drawn as a canopy ring of that size. |
| `PF07*` | nothing |  | kept out of surfaces | Keeps the seven-metre canopy marks out of the ground surface when the code carries a string number. |
| `PF08` | point, symbol `Large Tree Canopy`; `katana tree` (#5C8627); 0.18 mm | `vegetation/trees/canopy` |  | A tree whose crown spreads about eight metres across, drawn as a canopy ring of that size. |
| `PF08*` | nothing |  | kept out of surfaces | Keeps the eight-metre canopy marks out of the ground surface when the code carries a string number. |
| `PF09` | point, symbol `Broad Tree Canopy`; `katana tree` (#5C8627); 0.18 mm | `vegetation/trees/canopy` |  | A tree whose crown spreads about nine metres across, drawn as a canopy ring of that size. |
| `PF09*` | nothing |  | kept out of surfaces | Keeps the nine-metre canopy marks out of the ground surface when the code carries a string number. |
| `PF10` | point, symbol `Broad Tree Canopy`; `katana tree` (#5C8627); 0.18 mm | `vegetation/trees/canopy` |  | A tree whose crown spreads about ten metres across, drawn as a canopy ring of that size. |
| `PF10*` | nothing |  | kept out of surfaces | Keeps the ten-metre canopy marks out of the ground surface when the code carries a string number. |
| `PF12` | point, symbol `Giant Tree Canopy`; `katana tree` (#5C8627); 0.18 mm | `vegetation/trees/canopy` |  | A tree whose crown spreads about twelve metres across, drawn as a canopy ring of that size. |
| `PF12*` | nothing |  | kept out of surfaces | Keeps the twelve-metre canopy marks out of the ground surface when the code carries a string number. |
| `PF15` | point, symbol `Giant Tree Canopy`; `katana tree` (#5C8627); 0.18 mm | `vegetation/trees/canopy` |  | A tree whose crown spreads about fifteen metres across, drawn as a canopy ring of that size. |
| `PF15*` | nothing |  | kept out of surfaces | Keeps the fifteen-metre canopy marks out of the ground surface when the code carries a string number. |
| `PT01` | point, symbol `Hundred Millimetre Tree Trunk`; `katana tree` (#5C8627); 0.18 mm | `vegetation/trees/trunk` |  | A tree whose trunk is about 100 mm across, taken at the trunk. |
| `PT01*` | nothing |  | kept out of surfaces | Keeps 100 mm tree trunks out of the ground surface, since a trunk is not the ground. |
| `PT02` | point, symbol `Two Hundred Millimetre Tree Trunk`; `katana tree` (#5C8627); 0.18 mm | `vegetation/trees/trunk` |  | A tree whose trunk is about 200 mm across, taken at the trunk. |
| `PT02*` | nothing |  | kept out of surfaces | Keeps 200 mm tree trunks out of the ground surface, since a trunk is not the ground. |
| `PT03` | point, symbol `Three Hundred Millimetre Tree Trunk`; `katana tree` (#5C8627); 0.18 mm | `vegetation/trees/trunk` |  | A tree whose trunk is about 300 mm across, taken at the trunk. |
| `PT03*` | nothing |  | kept out of surfaces | Keeps 300 mm tree trunks out of the ground surface, since a trunk is not the ground. |
| `PT04` | point, symbol `Narrow Tree Trunk`; `katana tree` (#5C8627); 0.18 mm | `vegetation/trees/trunk` |  | Tree trunk about 400 mm across, shot at the middle of the trunk. |
| `PT04*` | nothing |  | kept out of surfaces | Keeps trunk points of 400 mm trees out of the ground surface. |
| `PT05` | point, symbol `Narrow Tree Trunk`; `katana tree` (#5C8627); 0.18 mm | `vegetation/trees/trunk` |  | Tree trunk about 500 mm across, shot at the middle of the trunk. |
| `PT05*` | nothing |  | kept out of surfaces | Keeps trunk points of 500 mm trees out of the ground surface. |
| `PT06` | point, symbol `Narrow Tree Trunk`; `katana tree` (#5C8627); 0.18 mm | `vegetation/trees/trunk` |  | Tree trunk about 600 mm across, shot at the middle of the trunk. |
| `PT06*` | nothing |  | kept out of surfaces | Keeps trunk points of 600 mm trees out of the ground surface. |
| `PT07` | point, symbol `Medium Tree Trunk`; `katana tree` (#5C8627); 0.18 mm | `vegetation/trees/trunk` |  | Tree trunk about 700 mm across, shot at the middle of the trunk. |
| `PT07*` | nothing |  | kept out of surfaces | Keeps trunk points of 700 mm trees out of the ground surface. |
| `PT08` | point, symbol `Medium Tree Trunk`; `katana tree` (#5C8627); 0.18 mm | `vegetation/trees/trunk` |  | Tree trunk about 800 mm across, shot at the middle of the trunk. |
| `PT08*` | nothing |  | kept out of surfaces | Keeps trunk points of 800 mm trees out of the ground surface. |
| `PT09` | point, symbol `Medium Tree Trunk`; `katana tree` (#5C8627); 0.18 mm | `vegetation/trees/trunk` |  | Tree trunk about 900 mm across, shot at the middle of the trunk. |
| `PT09*` | nothing |  | kept out of surfaces | Keeps trunk points of 900 mm trees out of the ground surface. |
| `PT10` | point, symbol `Wide Tree Trunk`; `katana tree` (#5C8627); 0.18 mm | `vegetation/trees/trunk` |  | Tree trunk about 1000 mm across, shot at the middle of the trunk. |
| `PT10*` | nothing |  | kept out of surfaces | Keeps trunk points of 1000 mm trees out of the ground surface. |
| `PT12` | point, symbol `Wide Tree Trunk`; `katana tree` (#5C8627); 0.18 mm | `vegetation/trees/trunk` |  | Tree trunk about 1200 mm across, shot at the middle of the trunk. |
| `PT12*` | nothing |  | kept out of surfaces | Keeps trunk points of 1200 mm trees out of the ground surface. |
| `PT15` | point, symbol `Wide Tree Trunk`; `katana tree` (#5C8627); 0.18 mm | `vegetation/trees/trunk` |  | Tree trunk about 1500 mm across, shot at the middle of the trunk. |
| `PT15*` | nothing |  | kept out of surfaces | Keeps trunk points of 1500 mm trees out of the ground surface. |
| `PTRE` | point, symbol `Unmeasured Tree Trunk`; `katana tree` (#5C8627); 0.18 mm | `vegetation/trees/trunk` |  | Tree trunk whose diameter was not measured, shot at the middle of the trunk. |
| `PTRE*` | nothing |  | kept out of surfaces | Keeps trunk points of unmeasured trees out of the ground surface. |
| `TR*` | line `continuous`; `katana tree` (#5C8627); 0.13 mm | `vegetation/trees/foliage` |  | Outline of the foliage of a tree or a stand of trees, drawn along its drip line. |

#### Ground Cover

The edges of timber, scrub, grass, crops, orchards and garden beds. (7 codes)

| Code | Draws | Layer | Also | What it is |
|---|---|---|---|---|
| `EG*` | line `continuous`; `katana planting` (#779624); 0.18 mm | `vegetation/ground/edge` |  | Edge of a garden bed where planting meets lawn or paving. |
| `JC*` | line `continuous`; `katana planting` (#779624); 0.18 mm | `vegetation/ground/edge` |  | Edge of cultivated land or pasture, shot as a line round the paddock. |
| `JD*` | line `Bush Edge Line`; `katana planting` (#779624); 0.18 mm | `vegetation/ground/edge` |  | Edge of dense timber or scrub, shot along the line of the outermost trunks. |
| `JG*` | line `Grass Edge`; `katana planting` (#779624); 0.18 mm | `vegetation/ground/edge` |  | Edge of grassland, shot as a line round the grassed area. |
| `JM*` | line `Medium Bush Edge`; `katana planting` (#779624); 0.18 mm | `vegetation/ground/edge` |  | Edge of medium-density timber or scrub, where the trees stand more widely spaced than in dense bush. |
| `JO*` | line `continuous`; `katana planting` (#779624); 0.18 mm | `vegetation/ground/edge` |  | Outline of an orchard, shot round the outer rows of trees. |
| `JS*` | line `Scattered Bush Edge`; `katana planting` (#779624); 0.18 mm | `vegetation/ground/edge` |  | Edge of scattered timber or scrub, where single trees stand well apart. |

### Class B: Buildings and Structures

What stands on the ground: buildings, bridges, masts, tanks and steps.

#### Buildings

Walls, eaves, awnings, doors, slabs and floor levels. (14 codes)

| Code | Draws | Layer | Also | What it is |
|---|---|---|---|---|
| `AW*` | line `Eave Line`; `katana building` (#627B8C); 0.18 mm | `buildings/outline/roof` | kept out of surfaces | The outer edge of an awning that projects over a footpath or a frontage. |
| `BU*` | line `continuous`; `katana building` (#627B8C); 0.35 mm | `buildings/outline/wall` |  | The outer face of a building wall. |
| `BV*` | line `Eave Line`; `katana building` (#627B8C); 0.18 mm | `buildings/outline/roof` | kept out of surfaces | The edge of a roof where it overhangs the wall. |
| `BW*` | line `Hidden Outline`; `katana building` (#627B8C); 0.13 mm | `buildings/outline/wall` |  | The foot of a wall where it meets the ground, drawn as a hidden edge. |
| `DO*` | line `continuous`; `katana building` (#627B8C); 0.25 mm | `buildings/outline/detail` |  | Opening of a door in a building wall, drawn as a short line across the wall. |
| `LB*` | line `Platform Edge Line`; `katana building` (#627B8C); 0.25 mm | `buildings/outline/detail` |  | Edge of a loading bay or loading dock, shot as a line along the raised platform. |
| `OR*` | line `Hidden Outline`; `katana building` (#627B8C); 0.13 mm | `buildings/outline/hidden` |  | Outline of a ruin, the remains of a building that is no longer whole. |
| `PAWN` | point, symbol `Awning Point`; `katana building` (#627B8C); 0.25 mm | `buildings/outline/roof` |  | A survey point on the edge of an awning. |
| `PAWN*` | nothing |  | kept out of surfaces | Awning points that carry a string number, which are kept out of the surface model. |
| `PCBU` | point, symbol `Building Corner`; `katana building` (#627B8C); 0.35 mm | `buildings/outline/detail` |  | A building corner shot at natural surface level, where the wall meets the ground. |
| `PFLR` | point, symbol `Spot Level`; `katana building` (#627B8C); 0.18 mm | `buildings/outline/detail` |  | A spot level taken on the finished floor of a building. |
| `PFLR*` | nothing |  | kept out of surfaces | Keeps floor levels out of the ground surface when the code carries a string number. |
| `TW*` | line `continuous`; `katana building` (#627B8C); 0.25 mm | `buildings/outline/wall` |  | Line along the top of a wall. |
| `VE*` | line `continuous`; `katana building` (#627B8C); 0.25 mm | `buildings/outline/slab` |  | Edge of a concrete slab laid flat on the ground. |

#### Structures

Bridge parts, piers, tanks, silos, pools and other built outlines. (20 codes)

| Code | Draws | Layer | Also | What it is |
|---|---|---|---|---|
| `AB*` | line `Abutment Foot Line`; `katana building` (#627B8C); 0.35 mm | `buildings/structures/bridge` |  | Foot of a bridge abutment, where its face meets the ground. |
| `AT*` | line `continuous`; `katana building` (#627B8C); 0.35 mm | `buildings/structures/bridge` |  | The upper edge of a bridge abutment, shot along its crest. |
| `BP*` | line `continuous`; `katana building` (#627B8C); 0.35 mm | `buildings/structures/bridge` |  | The plan outline of a bridge pier or column. |
| `BR*` | line `continuous`; `katana building` (#627B8C); 0.50 mm | `buildings/structures/bridge` |  | The edge of a bridge deck. |
| `HS*` | line `continuous`; `katana building` (#627B8C); 0.35 mm | `buildings/structures/bridge` |  | Beam across the top of a bridge pier or pile that carries the girders. |
| `OM*` | line `continuous`; `katana building` (#627B8C); 0.25 mm | `buildings/structures/structure` |  | Outline of a structure that no other code names. |
| `OS*` | line `continuous`; `katana building` (#627B8C); 0.25 mm | `buildings/structures/structure` |  | Outline of a silo or tank, seen from above. |
| `OT*` | line `continuous`; `katana waterway` (#6399BE); 0.25 mm | `buildings/structures/structure` |  | Outline of a swimming pool. |
| `OW*` | line `continuous`; `katana building` (#627B8C); 0.25 mm | `buildings/structures/structure` |  | Outline of a windmill, shot round its base. |
| `PABB` | point, symbol `Bottom Abutment Point`; `katana building` (#627B8C); 0.25 mm | `buildings/structures/bridge` |  | Survey point on the bottom of a bridge abutment. |
| `PABT` | point, symbol `Top Abutment Point`; `katana building` (#627B8C); 0.25 mm | `buildings/structures/bridge` |  | Survey point on the top of a bridge abutment. |
| `PBCP` | point, symbol `Pile Cap`; `katana building` (#627B8C); 0.25 mm | `buildings/structures/bridge` |  | Point where a bridge pile meets its cap. |
| `PBPI` | point, symbol `Bridge Column`; `katana building` (#627B8C); 0.35 mm | `buildings/structures/bridge` |  | The centre of a bridge pier or column, shot where it meets the ground. |
| `PFLG` | point, symbol `Flagpole`; `katana building` (#627B8C); 0.35 mm | `buildings/structures/structure` |  | A flagpole, shot at its base. |
| `PWML` | point, symbol `Windmill`; `katana building` (#627B8C); 0.25 mm | `buildings/structures/structure` |  | Windmill, usually one that pumps water, shot at the middle of its tower. |
| `UC*` | line `Hidden Outline`; `katana building` (#627B8C); 0.13 mm | `buildings/structures/bridge` | kept out of surfaces | Line on the ground under a bridge that marks the limit of its clearance overhead. |
| `UT*` | line `Hidden Outline`; `katana building` (#627B8C); 0.25 mm | `buildings/structures/tank` |  | Plan outline of a tank buried in the ground. |
| `WW*` | line `continuous`; `katana building` (#627B8C); 0.35 mm | `buildings/structures/bridge` |  | Wall that flares out from the end of a bridge abutment to hold back the embankment. |
| `XJ*` | line `continuous`; `katana building` (#627B8C); 0.13 mm | `buildings/structures/joint` |  | Line of a joint that lets a bridge deck expand. |
| `XP*` | line `continuous`; `katana building` (#627B8C); 0.25 mm | `buildings/structures/joint` |  | Edge of a steel plate that covers an expansion joint. |

#### Steps and Access

Outside stairs. (1 code)

| Code | Draws | Layer | Also | What it is |
|---|---|---|---|---|
| `SO*` | line `Steps Edge`; `katana building` (#627B8C); 0.25 mm | `buildings/access/steps` | kept out of surfaces | Outside stairs of a building, shot along the nosing of the treads. |

### Class F: Fences and Walls

What divides land: fences, walls, and the posts and gates between.

#### Fences

Fences of every kind, general, security and safety. (5 codes)

| Code | Draws | Layer | Also | What it is |
|---|---|---|---|---|
| `FC*` | line `continuous`; `katana fence` (#A18C65); 0.25 mm | `fences/fence/general` |  | Fence of a construction that no other fence code covers. |
| `FE*` | line `Post and Wire Fence`; `katana fence` (#A18C65); 0.25 mm | `fences/fence/wire` | sets `Check Zone=2` | Ordinary property or paddock fence of wire on posts. |
| `FL*` | line `continuous`; `katana fence` (#A18C65); 0.18 mm | `fences/fence/general` |  | Fence of unrecorded construction, shot as a plain line. |
| `FM*` | line `Metal Palisade Fence`; `katana fence` (#A18C65); 0.25 mm | `fences/fence/palisade` |  | Fence built to keep people out, too high or too close in its mesh to climb. |
| `SF*` | line `Metal Palisade Fence`; `katana fence` (#A18C65); 0.25 mm | `fences/fence/general` |  | Fence put up to keep people away from a drop, a pool or another danger. |

#### Walls

Retaining walls. (1 code)

| Code | Draws | Layer | Also | What it is |
|---|---|---|---|---|
| `RW*` | line `Earth Retaining Wall`; `katana wall` (#BA5556); 0.35 mm | `fences/wall/retaining` |  | Wall built to hold back earth, shot along its base. |

#### Gates and Posts

Gate leaves and fence or guide markers. (2 codes)

| Code | Draws | Layer | Also | What it is |
|---|---|---|---|---|
| `AG*` | line `Gate Leaf Outline`; `katana fence` (#A18C65); 0.25 mm | `fences/gate/gate` |  | A gate leaf between two posts, shot as a line from its hinge to its latch. |
| `POST` | point, symbol `Fence Post`; `katana fence` (#A18C65); 0.25 mm | `fences/gate/post` |  | A single fence post or roadside guide post, shot at its centre. |

### Class G: Terrain and Breaklines

The shape of the ground: shots, breaklines, contours, water edges and rock.

#### Ground Points

Natural ground shots and spot heights. (2 codes)

| Code | Draws | Layer | Also | What it is |
|---|---|---|---|---|
| `PNSS` | point, symbol `Ground Shot`; `katana ground` (#688F68); 0.18 mm | `terrain/points/ground` |  | A shot on the natural ground surface, used as a point of the terrain model. |
| `PSHT` | point, symbol `Spot Level`; `katana ground` (#688F68); 0.18 mm | `terrain/points/spot` |  | A spot height: a single level taken at a point and kept visible on the plan. |

#### Breaklines

Banks, cuttings, ridges, scarps and the soft lines that shape a surface. (7 codes)

| Code | Draws | Layer | Also | What it is |
|---|---|---|---|---|
| `BB*` | line `Bank Toe Edge`; `katana breakline` (#BE7D83); 0.25 mm | `terrain/breaklines/bank` |  | The bottom edge of a bank, where the slope meets the flat ground below. |
| `CU*` | line `Bank Top Edge`; `katana breakline` (#BE7D83); 0.25 mm | `terrain/breaklines/bank` |  | The top edge of a cutting, where the ground begins to fall into the cut. |
| `NB*` | line `Ridge Line`; `katana breakline` (#BE7D83); 0.18 mm | `terrain/breaklines/ridge-gully` |  | A breakline or ridge, shot as a line that the surface model must follow. |
| `NS*` | line `Soft Breakline`; `katana ground` (#688F68); 0.18 mm | `terrain/breaklines/general` | sets `Check Zone=2` | A line of ground shots along the natural surface where there is no sharper break, checked in model zone 2. |
| `PBRK` | point, symbol `Breakline Point`; `katana breakline` (#BE7D83); 0.18 mm | `terrain/breaklines/general` |  | A ground point lying on a breakline, shot where the grade changes so that the surface model can join it to its neighbours. |
| `TC*` | line `Bank Top Edge`; `katana breakline` (#BE7D83); 0.25 mm | `terrain/breaklines/bank` |  | Top edge of a bank or batter, with the ticks pointing downhill. |
| `VL*` | line `Bank Top Edge`; `katana breakline` (#BE7D83); 0.25 mm | `terrain/breaklines/erosion` |  | Scarp left by a landslide or by water eating into a bank. |

#### Contours

Index, regular and extra contours, and their depression forms. (7 codes)

| Code | Draws | Layer | Also | What it is |
|---|---|---|---|---|
| `X0*` | line `continuous`; `katana contour` (#B9704F); 0.18 mm | `terrain/contours/intermediate` |  | Contour at the regular interval between index contours. |
| `XQ*` | line `Depression Contour`; `katana contour` (#B9704F); 0.18 mm | `terrain/contours/depression` |  | Contour round a hollow at the regular interval, with the ticks pointing downhill. |
| `Y0*` | line `continuous`; `katana contour` (#B9704F); 0.13 mm | `terrain/contours/auxiliary` |  | Extra contour drawn at each whole metre between the regular ones. |
| `YQ*` | line `continuous`; `katana contour` (#B9704F); 0.13 mm | `terrain/contours/auxiliary` |  | Extra contour drawn at each tenth of a metre, for flat ground. |
| `Z*` | line `Index Contour`; `katana contour` (#B9704F); 0.35 mm | `terrain/contours/index` |  | Index contour, the heavier line drawn at every fifth contour. |
| `Z0*` | line `Index Contour`; `katana contour` (#B9704F); 0.35 mm | `terrain/contours/index` |  | Index contour at the regular index interval. |
| `ZQ*` | line `Depression Contour`; `katana contour` (#B9704F); 0.35 mm | `terrain/contours/depression` |  | Index contour round a hollow, with the ticks pointing downhill. |

#### Water Edges

Shorelines, bank edges, swamps and water levels. (7 codes)

| Code | Draws | Layer | Also | What it is |
|---|---|---|---|---|
| `JW*` | line `Marsh Edge`; `katana waterway` (#6399BE); 0.18 mm | `terrain/water/edge` |  | Outline of a swamp or marsh, shot along the edge of the wet ground. |
| `PFHT` | point, symbol `Water Level Mark`; `katana waterway` (#6399BE); 0.25 mm | `terrain/water/edge` |  | The level that a flood is known to have reached, shot at the mark it left. |
| `PSWA` | point, symbol `Swamp`; `katana waterway` (#6399BE); 0.18 mm | `terrain/water/wetland` |  | A swamp or marsh: a patch of wet, reedy ground marked by a point. |
| `PWLP` | point, symbol `Water Level Mark`; `katana waterway` (#6399BE); 0.18 mm | `terrain/water/level` |  | Point where the level of a water surface was read. |
| `WE*` | line `Water Edge`; `katana waterway` (#6399BE); 0.25 mm | `terrain/water/edge` |  | Edge of water, with a level taken on it. |
| `WL*` | line `Water Edge`; `katana waterway` (#6399BE); 0.25 mm | `terrain/water/lake` |  | Shoreline of a lake. |
| `WR*` | line `Water Edge`; `katana waterway` (#6399BE); 0.25 mm | `terrain/water/river` |  | Bank edge of a river or creek. |

#### Rock and Cliff

Rock outcrops and escarpment crests. (2 codes)

| Code | Draws | Layer | Also | What it is |
|---|---|---|---|---|
| `JR*` | line `Rock Outcrop Edge`; `katana contour` (#B9704F); 0.18 mm | `terrain/rock/edge` |  | Outline of an area of exposed rock, shot along its edge. |
| `VC*` | line `Cliff Edge`; `katana contour` (#B9704F); 0.25 mm | `terrain/rock/edge` |  | Top edge of a cliff or escarpment, with the ticks pointing over the drop. |

### Class M: Survey Control and Annotation

The survey's own marks, its investigations and the words on the plan.

#### Control Marks

Bench marks, survey marks, stations, pegs, bolts and the marks a job sets and uses. (28 codes)

| Code | Draws | Layer | Also | What it is |
|---|---|---|---|---|
| `PAPN` | point, symbol `Alignment Pin`; `katana control` (#0999C9); 0.25 mm | `control/marks/control` |  | An alignment pin: a pin set in the ground to mark a design alignment for setting out. |
| `PAPN*` | nothing |  | kept out of surfaces | Alignment pins that carry a string number, which are kept out of the surface model. |
| `PBLT` | point, symbol `Survey Spike`; `katana control` (#0999C9); 0.25 mm | `control/marks/control` |  | A bolt set in concrete or rock as a survey mark, shot at the centre of its head. |
| `PBLT*` | nothing |  | kept out of surfaces | Survey bolts that carry a string number, which are kept out of the surface model. |
| `PBMK` | point, symbol `Level Benchmark`; `katana control` (#0999C9); 0.25 mm | `control/marks/level` |  | A bench mark: a mark of known height that levelling is run from. |
| `PBMZ` | point, symbol `Level Benchmark` (hidden); `katana control` (#0999C9); 0.25 mm | `control/marks/level` |  | A bench mark kept under its own separate code so it can be listed and checked on its own. |
| `PCMK` | point, symbol `Control Mark`; `katana control` (#0999C9); 0.25 mm | `control/marks/control` |  | A permanent control mark of the road authority, used to tie road work into the survey network. |
| `PCMK*` | nothing |  | kept out of surfaces | Keeps the road authority's control marks out of the ground surface when the code carries a string number. |
| `PDPY` | point, symbol `Temporary Control`; `katana control` (#0999C9); 0.25 mm | `control/marks/temporary` |  | A temporary peg driven into the ground to carry a height for a dumpy level. |
| `PDPY*` | nothing |  | kept out of surfaces | Keeps levelling pegs out of the ground surface when the code carries a string number. |
| `PDSM` | point, symbol `Fixed Survey Mark`; `katana control` (#0999C9); 0.25 mm | `control/marks/control` |  | A permanent survey mark placed by the water authority and used as a height or position reference. |
| `PDSM*` | nothing |  | kept out of surfaces | Keeps the water authority's survey marks out of the ground surface when the code carries a string number. |
| `PHCP` | point, symbol `Control Mark`; `katana control` (#0999C9); 0.25 mm | `control/marks/control` |  | A control point held in plan only, with no level, used to control photogrammetry. |
| `PMCP` | point, symbol `Control Mark`; `katana control` (#0999C9); 0.25 mm | `control/marks/control` |  | A minor control point set to support the main control network. |
| `PPCN` | point, symbol `Photo Centre`; `katana control` (#0999C9); 0.25 mm | `control/marks/control` |  | The ground point at the centre of an aerial photograph, used to position and orient the image. |
| `PPSM` | point, symbol `Fixed Survey Mark` (hidden); `katana control` (#0999C9); 0.25 mm | `control/marks/control` |  | A permanent survey mark with coordinates, which the survey can be tied to. |
| `PSS*` | nothing |  | kept out of surfaces | Keeps every station mark out of the ground surface: control marks fix positions and are not terrain. |
| `PSSA` | point, symbol `Control Mark`; `katana control` (#0999C9); 0.25 mm | `control/marks/control` |  | A survey mark of unspecified type, used when the kind of mark is not given. |
| `PSSB` | point, symbol `Level Benchmark`; `katana control` (#0999C9); 0.25 mm | `control/marks/level` |  | The broad arrow cut in stone or a kerb to mark a bench level. |
| `PSSC` | point, symbol `Temporary Control`; `katana control` (#0999C9); 0.25 mm | `control/marks/temporary` |  | A peg driven in as a station for the job. |
| `PSSD` | point, symbol `Fixed Survey Mark`; `katana control` (#0999C9); 0.25 mm | `control/marks/control` |  | A station marked by a plug, a drill hole, an iron pipe or a reference block. |
| `PSSE` | point, symbol `Survey Spike`; `katana control` (#0999C9); 0.25 mm | `control/marks/temporary` |  | A station marked by a bolt, a nail, a spike or a similar small metal fixing. |
| `PSSF` | point, symbol `Trigonometric Station`; `katana control` (#0999C9); 0.25 mm | `control/marks/control` |  | A trigonometric station, a primary control mark of the geodetic network. |
| `PSSG` | point, symbol `Control Mark`; `katana control` (#0999C9); 0.25 mm | `control/marks/control` |  | A mark of the permanent, state or project control networks, used as a survey station. |
| `PSSM` | point, symbol `Control Mark`; `katana control` (#0999C9); 0.25 mm | `control/marks/control` |  | A mark of the state geodetic network, with published coordinates and heights. |
| `PTSS` | point, symbol `Trigonometric Station`; `katana control` (#0999C9); 0.25 mm | `control/marks/control` |  | Trigonometric station built as a concrete pillar. |
| `PTSS*` | nothing |  | kept out of surfaces | Keeps concrete trig pillar points out of the ground surface. |
| `PVCP` | point, symbol `Level Benchmark`; `katana control` (#0999C9); 0.25 mm | `control/marks/level` |  | Point whose height alone is held as control, such as a target for aerial photo mapping. |

#### Investigation Points

Boreholes, test pits, potholes, seismic shots and trenches. (9 codes)

| Code | Draws | Layer | Also | What it is |
|---|---|---|---|---|
| `PBHX` | point, symbol `Borehole`; `katana control` (#0999C9); 0.25 mm | `control/investigation/ground` |  | A borehole, shot at the centre of its collar. |
| `PGEO` | point, symbol `Test Pit`; `katana control` (#0999C9); 0.25 mm | `control/investigation/ground` |  | A pit excavated to inspect the ground or to expose a buried service, shot at its centre. |
| `PHNL` | point, symbol `Pothole`; `katana control` (#0999C9); 0.25 mm | `control/investigation/pothole` |  | A pothole dug to expose a service, recorded without a level on the service. |
| `PHNL*` | nothing |  | kept out of surfaces | Keeps pothole points without a level out of the ground surface. |
| `PHRL` | point, symbol `Pothole`; `katana control` (#0999C9); 0.25 mm | `control/investigation/pothole` |  | A pothole dug to expose a service, with the level of the exposed service recorded. |
| `PSTS` | point, symbol `Seismic Shot Point`; `katana control` (#0999C9); 0.25 mm | `control/investigation/ground` |  | A seismic test shot: the point where a charge was fired for a seismic survey. |
| `SL*` | line `Hidden Outline`; `katana control` (#0999C9); 0.18 mm | `control/investigation/geophysics` |  | Line followed by a seismic survey, along which shots and receivers are set. |
| `XB*` | line `Hidden Outline`; `katana control` (#0999C9); 0.18 mm | `control/investigation/trench` |  | Bottom edge of a trench dug to expose buried services. |
| `XT*` | line `continuous`; `katana control` (#0999C9); 0.25 mm | `control/investigation/trench` |  | Top edge of a trench dug to expose buried services. |

#### Text and Notes

Seven text sizes and the layer that holds them. (14 codes)

| Code | Draws | Layer | Also | What it is |
|---|---|---|---|---|
| `1` | text 1.5 mm; `katana note` (#9A8EA4) |  |  | Plan text 1.5 mm high on paper, the finest size, for crowded detail. |
| `1*` | the layer of the text; `katana note` (#9A8EA4); 0.13 mm | `control/annotation/text` |  | Layer and pen for 1.5 mm plan text, so it can be switched off with the other notes. |
| `2` | text 2.5 mm; `katana note` (#9A8EA4) |  |  | Plan text 2.5 mm high on paper, the everyday size for notes and labels. |
| `2*` | the layer of the text; `katana note` (#9A8EA4); 0.13 mm | `control/annotation/text` |  | Layer and pen for 2.5 mm plan text, so it can be switched off with the other notes. |
| `3` | text 3.5 mm; `katana note` (#9A8EA4) |  |  | Plan text 3.5 mm high on paper, for prominent notes and sub-headings. |
| `3*` | the layer of the text; `katana note` (#9A8EA4); 0.13 mm | `control/annotation/text` |  | Layer and pen for 3.5 mm plan text, so it can be switched off with the other notes. |
| `4` | text 5 mm; `katana note` (#9A8EA4) |  |  | Plan text 5 mm high on paper, large enough for headings. |
| `4*` | the layer of the text; `katana note` (#9A8EA4); 0.13 mm | `control/annotation/text` |  | Layer and pen for 5.0 mm plan text, so it can be switched off with the other notes. |
| `5` | text 7 mm; `katana note` (#9A8EA4) |  |  | Plan text 7 mm high on paper, for titles and major headings. |
| `5*` | the layer of the text; `katana note` (#9A8EA4); 0.13 mm | `control/annotation/text` |  | Layer and pen for 7.0 mm plan text, so it can be switched off with the other notes. |
| `7` | text 10 mm; `katana note` (#9A8EA4) |  |  | Plan text 10 mm high on paper, the largest size, for sheet titles. |
| `7*` | the layer of the text; `katana note` (#9A8EA4); 0.13 mm | `control/annotation/text` |  | Layer and pen for 10.0 mm plan text, so it can be switched off with the other notes. |
| `8` | text 2.5 mm; `katana note` (#9A8EA4) |  |  | Parcel label text 2.5 mm high on paper: the lot number and the deposited plan number. |
| `8*` | the layer of the text; `katana note` (#9A8EA4); 0.13 mm | `control/annotation/text` |  | Layer and pen for parcel label text, so lot and plan numbers can be switched off together. |

#### Plan Marks

The limits of a survey and of the detail taken from photographs. (3 codes)

| Code | Draws | Layer | Also | What it is |
|---|---|---|---|---|
| `DE*` | line `Photo Detail Boundary`; `katana note` (#9A8EA4); 0.35 mm | `control/plan/limit` |  | Outer limit of the area digitised from aerial photography, where detail comes from the photo and not the field. |
| `FS*` | line `Limit of Survey Line`; `katana note` (#9A8EA4); 0.50 mm | `control/plan/limit` |  | Outer limit of the ground that was surveyed in the field, drawn over a photo base. |
| `NF*` | line `Limit of Survey Line`; `katana note` (#9A8EA4); 0.25 mm | `control/plan/limit` |  | Limit of an area where photo-based mapping was completed or checked by survey in the field. |

### Class X: Miscellaneous

Hazards, heritage, land use and the catch-alls.

#### Hazards and Heritage

Hazards, heritage items, habitat and threatened communities. (12 codes)

| Code | Draws | Layer | Also | What it is |
|---|---|---|---|---|
| `PHAZ` | point, symbol `Hazard Point`; `katana hazard` (#FF5A37); 0.25 mm | `miscellaneous/hazards/hazard` |  | A site that endangers people or later works, such as contaminated or unstable ground. |
| `PHAZ*` | nothing |  | kept out of surfaces | Keeps hazardous-site points out of the ground surface. |
| `PHEC` | point, symbol `Habitat Marker`; `katana tree` (#5C8627); 0.25 mm | `miscellaneous/hazards/habitat` |  | A point inside a plant or animal community recorded as endangered, to be avoided by works. |
| `PHEC*` | nothing |  | kept out of surfaces | Keeps endangered-community points out of the ground surface. |
| `PHEE` | point, symbol `Habitat Marker`; `katana tree` (#5C8627); 0.25 mm | `miscellaneous/hazards/habitat` |  | A point inside a vegetation community listed by law as an endangered ecological community. |
| `PHEE*` | nothing |  | kept out of surfaces | Keeps listed ecological community points out of the ground surface. |
| `PHIA` | point, symbol `Heritage Item`; `katana note` (#9A8EA4); 0.25 mm | `miscellaneous/hazards/heritage` |  | A place of Indigenous cultural heritage, marked so that works can keep clear of it. |
| `PHIA*` | nothing |  | kept out of surfaces | Keeps Indigenous heritage points out of the ground surface. |
| `PHIS` | point, symbol `Heritage Item`; `katana note` (#9A8EA4); 0.25 mm | `miscellaneous/hazards/heritage` |  | A point of historic interest, such as a plaque, a marker stone or an old relic. |
| `PHNI` | point, symbol `Heritage Item`; `katana note` (#9A8EA4); 0.25 mm | `miscellaneous/hazards/heritage` |  | A heritage site without Indigenous significance, such as a historic ruin, relic or building remnant. |
| `PHNI*` | nothing |  | kept out of surfaces | Keeps non-Indigenous heritage points out of the ground surface. |
| `PHST` | point, symbol `Headstone`; `katana note` (#9A8EA4); 0.25 mm | `miscellaneous/hazards/heritage` |  | A grave headstone in a cemetery, shot at the middle of its base. |

#### General

A point to attend to, a crossing of two lines, and the survey check line. (3 codes)

| Code | Draws | Layer | Also | What it is |
|---|---|---|---|---|
| `HP*` | line `continuous`; `katana note` (#9A8EA4); 0.25 mm | `miscellaneous/general/generic` | kept out of surfaces | Outline of an area singled out for attention; the code does not say why. |
| `PVID` | point, symbol `Generic Point`; `katana note` (#9A8EA4); 0.25 mm | `miscellaneous/general/generic` |  | Point where two surveyed lines meet, usually worked out by calculation. |
| `QQ*` | line `continuous`; `katana control` (#0999C9); 0.13 mm | `miscellaneous/general/check` | kept out of surfaces | Line of check shots used to test the survey or the model, kept out of the ground surface. |

#### Land Use Areas

The outlines of land by its use: built-up areas, cemeteries, golf courses, parks, sporting arenas and stockpiles. (6 codes)

| Code | Draws | Layer | Also | What it is |
|---|---|---|---|---|
| `OB*` | line `continuous`; `katana building` (#627B8C); 0.13 mm | `miscellaneous/land-use/area` |  | General outline of a built-up area, drawn as one extent and not building by building. |
| `OC*` | line `continuous`; `katana note` (#9A8EA4); 0.18 mm | `miscellaneous/land-use/area` |  | Outline of a cemetery or burial ground. |
| `OG*` | line `Grass Edge`; `katana planting` (#779624); 0.18 mm | `miscellaneous/land-use/area` |  | Outline of a golf course. |
| `OH*` | line `Bank Toe Edge`; `katana breakline` (#BE7D83); 0.18 mm | `miscellaneous/land-use/area` |  | Outline of a stockpile of loose material, shot round its toe. |
| `OO*` | line `continuous`; `katana building` (#627B8C); 0.18 mm | `miscellaneous/land-use/area` |  | Outline of a sporting arena, such as a stadium, an oval or a court. |
| `OP*` | line `Grass Edge`; `katana planting` (#779624); 0.18 mm | `miscellaneous/land-use/area` |  | Outline of a park or other open space. |

### Class D: Design and Setting Out

Strings that carry a design, not a survey: set out from them, do not read the ground from them.

#### Road Design Strings

Design strings of a road that the file does not name for any purpose. (27 codes)

| Code | Draws | Layer | Also | What it is |
|---|---|---|---|---|
| `BD*` | line `continuous`; `katana pavement` (#789295); 0.18 mm | `design/road-design-strings/unclassified` |  | A road design string with no named role, left free for a design package to give its own meaning. |
| `C9*` | line `continuous`; `katana pavement` (#789295); 0.18 mm | `design/road-design-strings/unclassified` |  | A road design string with no named role, left free for a design package to give its own meaning. |
| `CL*` | line `continuous`; `katana pavement` (#789295); 0.18 mm | `design/road-design-strings/unclassified` |  | A road design string with no named role, left free for a design package to give its own meaning. |
| `CM*` | line `continuous`; `katana pavement` (#789295); 0.18 mm | `design/road-design-strings/unclassified` |  | A road design string with no named role, left free for a design package to give its own meaning. |
| `CR*` | line `continuous`; `katana pavement` (#789295); 0.18 mm | `design/road-design-strings/unclassified` |  | A road design string with no named role, left free for a design package to give its own meaning. |
| `CS*` | line `continuous`; `katana pavement` (#789295); 0.18 mm | `design/road-design-strings/unclassified` |  | A road design string with no named role, left free for a design package to give its own meaning. |
| `E0*` | line `continuous`; `katana pavement` (#789295); 0.18 mm | `design/road-design-strings/unclassified-e` |  | Road design string of a kind that has no recorded purpose, kept so that a design file still plots. |
| `E3*` | line `continuous`; `katana pavement` (#789295); 0.18 mm | `design/road-design-strings/unclassified-e` |  | Road design string of a kind that has no recorded purpose, kept so that a design file still plots. |
| `E4*` | line `continuous`; `katana pavement` (#789295); 0.18 mm | `design/road-design-strings/unclassified-e` |  | Road design string of a kind that has no recorded purpose, kept so that a design file still plots. |
| `E5*` | line `continuous`; `katana pavement` (#789295); 0.18 mm | `design/road-design-strings/unclassified-e` |  | Road design string of a kind that has no recorded purpose, kept so that a design file still plots. |
| `E6*` | line `continuous`; `katana pavement` (#789295); 0.18 mm | `design/road-design-strings/unclassified-e` |  | Road design string of a kind that has no recorded purpose, kept so that a design file still plots. |
| `E7*` | line `continuous`; `katana pavement` (#789295); 0.18 mm | `design/road-design-strings/unclassified-e` |  | Road design string of a kind that has no recorded purpose, kept so that a design file still plots. |
| `EB*` | line `continuous`; `katana pavement` (#789295); 0.18 mm | `design/road-design-strings/unclassified-e` |  | Road design string of a kind that has no recorded purpose, kept so that a design file still plots. |
| `EF*` | line `continuous`; `katana pavement` (#789295); 0.18 mm | `design/road-design-strings/unclassified-e` |  | Road design string of a kind that has no recorded purpose, kept so that a design file still plots. |
| `ER*` | line `continuous`; `katana pavement` (#789295); 0.18 mm | `design/road-design-strings/unclassified-e` |  | Road design string of a kind that has no recorded purpose, kept so that a design file still plots. |
| `ET*` | line `continuous`; `katana pavement` (#789295); 0.18 mm | `design/road-design-strings/unclassified-e` |  | Road design string of a kind that has no recorded purpose, kept so that a design file still plots. |
| `I0*` | line `continuous`; `katana pavement` (#789295); 0.18 mm | `design/road-design-strings/unclassified-i` |  | Road design string of a kind that has no recorded purpose, kept so that a design file still plots. |
| `I1*` | line `continuous`; `katana pavement` (#789295); 0.18 mm | `design/road-design-strings/unclassified-i` |  | Road design string of a kind that has no recorded purpose, kept so that a design file still plots. |
| `I2*` | line `continuous`; `katana pavement` (#789295); 0.18 mm | `design/road-design-strings/unclassified-i` |  | Road design string of a kind that has no recorded purpose, kept so that a design file still plots. |
| `I3*` | line `continuous`; `katana pavement` (#789295); 0.18 mm | `design/road-design-strings/unclassified-i` |  | Road design string of a kind that has no recorded purpose, kept so that a design file still plots. |
| `I4*` | line `continuous`; `katana pavement` (#789295); 0.18 mm | `design/road-design-strings/unclassified-i` |  | Road design string of a kind that has no recorded purpose, kept so that a design file still plots. |
| `I5*` | line `continuous`; `katana pavement` (#789295); 0.18 mm | `design/road-design-strings/unclassified-i` |  | Road design string of a kind that has no recorded purpose, kept so that a design file still plots. |
| `I6*` | line `continuous`; `katana pavement` (#789295); 0.18 mm | `design/road-design-strings/unclassified-i` |  | Road design string of a kind that has no recorded purpose, kept so that a design file still plots. |
| `I7*` | line `continuous`; `katana pavement` (#789295); 0.18 mm | `design/road-design-strings/unclassified-i` |  | Road design string of a kind that has no recorded purpose, kept so that a design file still plots. |
| `I8*` | line `continuous`; `katana pavement` (#789295); 0.18 mm | `design/road-design-strings/unclassified` |  | A road design string with no role assigned yet; the first of two spare classes held for the designer's own use. |
| `I9*` | line `continuous`; `katana pavement` (#789295); 0.18 mm | `design/road-design-strings/unclassified` |  | A road design string with no role assigned yet; the second of two spare classes held for the designer's own use. |
| `R*` | line `continuous`; `katana pavement` (#789295); 0.25 mm | `design/road-design-strings/unclassified` |  | Road design string starting with R that no more specific code claims. |

#### Lane Marking Design

The designed lines of lanes, edges and junctions, to be painted. (25 codes)

| Code | Draws | Layer | Also | What it is |
|---|---|---|---|---|
| `L0*` | line `Bicycle Lane Marking`; `katana pavement` (#789295); 0.18 mm | `design/lane-marking-design/dividing` |  | Separation line along a bicycle path, design type S4. |
| `L2*` | line `Dividing Line Marking`; `katana pavement` (#789295); 0.25 mm | `design/lane-marking-design/dividing` |  | Dividing line between opposing lanes of a multi-lane road, design type S2. |
| `L3*` | line `Dashed Lane Marking`; `katana pavement` (#789295); 0.18 mm | `design/lane-marking-design/lane-lines` |  | Lane line between lanes travelling the same way on a multi-lane road, design type L1. |
| `L4*` | line `Dashed Lane Marking`; `katana pavement` (#789295); 0.18 mm | `design/lane-marking-design/lane-lines` |  | Lane line between the lanes of a multi-lane roundabout, design type L4. |
| `L5*` | line `continuous`; `katana pavement` (#789295); 0.18 mm | `design/lane-marking-design/dividing` |  | Separation line along a bicycle path, design type S5. |
| `L6*` | line `Dashed Lane Marking`; `katana pavement` (#789295); 0.18 mm | `design/lane-marking-design/lane-lines` |  | Lane line beside a bus lane, design type L6. |
| `L7*` | line `Bicycle Lane Marking`; `katana pavement` (#789295); 0.18 mm | `design/lane-marking-design/lane-lines` |  | Lane line beside a bicycle lane, design type L7. |
| `L8*` | line `Dividing Line Marking`; `katana pavement` (#789295); 0.25 mm | `design/lane-marking-design/dividing` |  | Dividing line between opposing lanes of a two-lane road, design type S1. |
| `L9*` | line `Dividing Line Marking`; `katana pavement` (#789295); 0.25 mm | `design/lane-marking-design/dividing` |  | Dividing line between opposing lanes of a two-lane road, design type S3. |
| `LA*` | line `Lane Continuity Marking`; `katana pavement` (#789295); 0.18 mm | `design/lane-marking-design/intersection` |  | Continuity line, design type C1, that carries a lane edge across a junction. |
| `LD*` | line `Double Barrier Marking`; `katana pavement` (#789295); 0.25 mm | `design/lane-marking-design/dividing` |  | Double barrier line, design type BB: two unbroken lines that forbid crossing in either direction. |
| `LE*` | line `continuous`; `katana pavement` (#789295); 0.18 mm | `design/lane-marking-design/edge-lines` |  | Edge line along the side of the carriageway, in its general form. |
| `LF*` | line `Lane Continuity Marking`; `katana pavement` (#789295); 0.18 mm | `design/lane-marking-design/intersection` |  | Continuity line, design type C4, that carries a bicycle lane across a junction. |
| `LH*` | line `Left Broken Barrier Marking`; `katana pavement` (#789295); 0.25 mm | `design/lane-marking-design/dividing` |  | Combined barrier and separation line, design type BS1: an unbroken line beside a broken one. |
| `LJ*` | line `Lane Continuity Marking`; `katana pavement` (#789295); 0.18 mm | `design/lane-marking-design/edge-lines` |  | Kerbside line marking a clearway along the kerb, design type C2. |
| `LK*` | line `continuous`; `katana pavement` (#789295); 0.25 mm | `design/lane-marking-design/edge-lines` |  | Kerbside line marking a no-stopping length of kerb, design type C3. |
| `LM*` | line `continuous`; `katana pavement` (#789295); 0.18 mm | `design/lane-marking-design/edge-lines` |  | Edge line round a painted median, design type E5. |
| `LN*` | line `Dashed Lane Marking`; `katana pavement` (#789295); 0.18 mm | `design/lane-marking-design/lane-lines` |  | Lane line between lanes travelling the same way on a multi-lane road, design type L3. |
| `LR*` | line `Right Broken Barrier Marking`; `katana pavement` (#789295); 0.25 mm | `design/lane-marking-design/dividing` |  | Combined barrier and separation line, design type BS2, the mirror form of BS1. |
| `LS*` | line `continuous`; `katana pavement` (#789295); 0.35 mm | `design/lane-marking-design/intersection` |  | Stop line at traffic signals, design type TF. |
| `LT*` | line `Lane Continuity Marking`; `katana pavement` (#789295); 0.18 mm | `design/lane-marking-design/intersection` |  | Turn line, design type T1, that guides turning traffic through a junction. |
| `LU*` | line `continuous`; `katana pavement` (#789295); 0.18 mm | `design/lane-marking-design/edge-lines` |  | A plain unbroken painted line that fits none of the named types. |
| `LW*` | line `Lane Continuity Marking`; `katana pavement` (#789295); 0.18 mm | `design/lane-marking-design/intersection` |  | Give way line, design type TB, painted across the approach to a junction. |
| `LY*` | line `Bicycle Crossing Marking`; `katana pavement` (#789295); 0.18 mm | `design/lane-marking-design/intersection` |  | A bicycle crossing painted across the carriageway. |
| `LZ*` | line `Zebra Crossing Marking`; `katana pavement` (#789295); 0.18 mm | `design/lane-marking-design/intersection` |  | A zebra pedestrian crossing painted across the carriageway. |

#### Earthworks Design

Batters, formation, interfaces and the design edges of cut and fill. (14 codes)

| Code | Draws | Layer | Also | What it is |
|---|---|---|---|---|
| `DJ*` | line `continuous`; `katana waterway` (#6399BE); 0.25 mm | `design/earthworks-design/table-drain` |  | Design edge of a table drain on the side furthest from the road. |
| `DK*` | line `continuous`; `katana waterway` (#6399BE); 0.25 mm | `design/earthworks-design/table-drain` |  | Design edge of a table drain on the side nearest the road. |
| `DM*` | line `continuous`; `katana breakline` (#BE7D83); 0.25 mm | `design/earthworks-design/formation` |  | Line along a median designed to sit lower than the carriageways beside it. |
| `E1*` | line `continuous`; `katana breakline` (#BE7D83); 0.25 mm | `design/earthworks-design/batter` |  | Outer, downslope edge of a bench cut into a slope in the earthworks design. |
| `E2*` | line `continuous`; `katana breakline` (#BE7D83); 0.25 mm | `design/earthworks-design/batter` |  | Inner edge of a bench, where it meets the rising slope behind. |
| `EH*` | line `continuous`; `katana breakline` (#BE7D83); 0.18 mm | `design/earthworks-design/batter` |  | Line where the formation turns into the batter slope, the hinge of the cross-section. |
| `EV*` | line `continuous`; `katana breakline` (#BE7D83); 0.25 mm | `design/earthworks-design/formation` |  | Outer edge of the verge as designed, on the side furthest from the road. |
| `EW*` | line `continuous`; `katana pavement` (#789295); 0.25 mm | `design/earthworks-design/formation` |  | Edge of a designed footpath on the side nearest the road. |
| `EX*` | line `continuous`; `katana pavement` (#789295); 0.25 mm | `design/earthworks-design/formation` |  | Edge of a designed footway on the side furthest from the road. |
| `IA*` | line `Bank Toe Edge`; `katana breakline` (#BE7D83); 0.18 mm | `design/earthworks-design/interface` |  | Design line where an earthworks batter meets the existing ground, the catch point of a cut or a fill. |
| `IB*` | line `Bank Top Edge`; `katana breakline` (#BE7D83); 0.18 mm | `design/earthworks-design/interface` |  | Design line along the front edge of a bench cut into an earthworks batter. |
| `IC*` | line `continuous`; `katana breakline` (#BE7D83); 0.18 mm | `design/earthworks-design/interface` |  | A second design string for the front edge of an earthworks bench, drawn as a plain line. |
| `ID*` | line `Dish Drain Edge`; `katana breakline` (#BE7D83); 0.18 mm | `design/earthworks-design/interface` |  | Design line along the front edge of a table drain, the side nearest the road, where it meets the earthworks. |
| `IE*` | line `continuous`; `katana breakline` (#BE7D83); 0.18 mm | `design/earthworks-design/interface` |  | Design line along the back edge of a table drain, on the side away from the road. |

#### Kerb and Carriageway Design

The designed lines of kerbs, gutters, hinges and carriageway edges. (15 codes)

| Code | Draws | Layer | Also | What it is |
|---|---|---|---|---|
| `C0*` | line `continuous`; `katana pavement` (#789295); 0.25 mm | `design/kerb-and-carriageway-design/carriageway` |  | Design line for the edge of a median. |
| `C1*` | line `continuous`; `katana kerb` (#907368); 0.25 mm | `design/kerb-and-carriageway-design/kerb` |  | Design line for the first base line of a kerb, at its foot. |
| `C2*` | line `continuous`; `katana kerb` (#907368); 0.25 mm | `design/kerb-and-carriageway-design/kerb` |  | Design line for the second base line of a kerb, at its foot. |
| `C3*` | line `continuous`; `katana kerb` (#907368); 0.18 mm | `design/kerb-and-carriageway-design/kerb` |  | Design line for the first hinge of a kerb, a break in its cross-section. |
| `C4*` | line `continuous`; `katana kerb` (#907368); 0.18 mm | `design/kerb-and-carriageway-design/kerb` |  | Design line for the second hinge of a kerb, a break in its cross-section. |
| `C5*` | line `continuous`; `katana kerb` (#907368); 0.18 mm | `design/kerb-and-carriageway-design/kerb` |  | Design line for the third hinge of a kerb, a break in its cross-section. |
| `C6*` | line `continuous`; `katana kerb` (#907368); 0.18 mm | `design/kerb-and-carriageway-design/kerb` |  | Design line for the fourth hinge of a kerb, a break in its cross-section. |
| `C7*` | line `continuous`; `katana kerb` (#907368); 0.18 mm | `design/kerb-and-carriageway-design/kerb` |  | Design line for the fifth hinge of a kerb, a break in its cross-section. |
| `C8*` | line `continuous`; `katana kerb` (#907368); 0.18 mm | `design/kerb-and-carriageway-design/kerb` |  | Design line for the sixth hinge of a kerb, a break in its cross-section. |
| `CE*` | line `continuous`; `katana pavement` (#789295); 0.25 mm | `design/kerb-and-carriageway-design/carriageway` |  | Design line for the edge of the sealed carriageway surface. |
| `CF*` | line `continuous`; `katana kerb` (#907368); 0.18 mm | `design/kerb-and-carriageway-design/carriageway` |  | Design line along the flow line of a gutter. |
| `CH*` | line `continuous`; `katana kerb` (#907368); 0.18 mm | `design/kerb-and-carriageway-design/carriageway` |  | Design line along the lip of the gutter at the edge of the through lane. |
| `ES*` | line `continuous`; `katana pavement` (#789295); 0.25 mm | `design/kerb-and-carriageway-design/carriageway` |  | Outer edge of the road shoulder as designed, on the side away from the traffic lanes. |
| `NJ*` | line `continuous`; `katana pavement` (#789295); 0.13 mm | `design/kerb-and-carriageway-design/carriageway` |  | Design line along the joint between two pavement areas, such as new work meeting existing. |
| `NL*` | line `Road Centreline`; `katana pavement` (#789295); 0.25 mm | `design/kerb-and-carriageway-design/carriageway` |  | Design line for the crown of a carriageway that is offset from the road's master alignment. |

#### Bridge Design

The designed lines of abutments and piers. (6 codes)

| Code | Draws | Layer | Also | What it is |
|---|---|---|---|---|
| `AU*` | line `continuous`; `katana building` (#627B8C); 0.25 mm | `design/bridge-design/substructure` |  | Design line for the first hinge of a bridge abutment, a break in its cross-section. |
| `AV*` | line `continuous`; `katana building` (#627B8C); 0.25 mm | `design/bridge-design/substructure` |  | Design line for the second hinge of a bridge abutment, a break in its cross-section. |
| `AY*` | line `continuous`; `katana building` (#627B8C); 0.25 mm | `design/bridge-design/substructure` |  | Design line for the third hinge of a bridge abutment, a break in its cross-section. |
| `AZ*` | line `continuous`; `katana building` (#627B8C); 0.25 mm | `design/bridge-design/substructure` |  | Design line for the fourth hinge of a bridge abutment, a break in its cross-section. |
| `BA*` | line `continuous`; `katana building` (#627B8C); 0.35 mm | `design/bridge-design/substructure` |  | Design line along the face of a bridge abutment. |
| `BQ*` | line `continuous`; `katana building` (#627B8C); 0.35 mm | `design/bridge-design/substructure` |  | Design line for the outline of a bridge pier. |

#### Drainage Design

Designed pipes, culverts, channels, drains and structures. (6 codes)

| Code | Draws | Layer | Also | What it is |
|---|---|---|---|---|
| `DH*` | line `continuous`; `katana stormwater` (#00A800); 0.25 mm | `design/drainage-design/structure` |  | Design line for the wall that surrounds the end of a drain or culvert. |
| `DI*` | line `Stormwater Pipe Line`; `katana stormwater` (#00A800); 0.35 mm | `design/drainage-design/pipe` |  | Design line for a stormwater drainage pipe. |
| `DL*` | line `Open Drain Invert`; `katana waterway` (#6399BE); 0.35 mm | `design/drainage-design/channel` |  | Design line for a concrete or otherwise lined drain that catches runoff above a cutting. |
| `DU*` | line `Open Drain Invert`; `katana waterway` (#6399BE); 0.18 mm | `design/drainage-design/channel` |  | Design line for an earth drain with no lining that catches runoff above a cutting. |
| `DV*` | line `Culvert Outline`; `katana stormwater` (#00A800); 0.35 mm | `design/drainage-design/pipe` |  | Design line for a culvert that carries water under a road or embankment. |
| `SH*` | line `Subsoil Drain Line`; `katana stormwater` (#00A800); 0.18 mm | `design/drainage-design/subsoil` |  | Design line along the edge of a subsoil drain. |

#### Barrier and Fence Design

Designed guard rails, fences and bollards. (5 codes)

| Code | Draws | Layer | Also | What it is |
|---|---|---|---|---|
| `AD*` | point, symbol `Bollard`; `katana street` (#A2759D); 0.25 mm | `design/barrier-and-fence-design/barrier` |  | Intended position of a bollard on a design, set out as a point and drawn with the bollard symbol. |
| `BF*` | line `Guard Rail Line`; `katana street` (#A2759D); 0.35 mm | `design/barrier-and-fence-design/barrier` |  | Design line for a W-beam or thrie-beam guard rail on the left-hand side of the alignment. |
| `BG*` | line `Guard Rail Line`; `katana street` (#A2759D); 0.35 mm | `design/barrier-and-fence-design/barrier` |  | Design line for a W-beam or thrie-beam guard rail on the right-hand side of the alignment. |
| `FN*` | line `Chain Mesh Fence`; `katana fence` (#A18C65); 0.25 mm | `design/barrier-and-fence-design/fence` |  | Design line for a fence along a property boundary. |
| `SA*` | line `Post and Rail Fence`; `katana fence` (#A18C65); 0.25 mm | `design/barrier-and-fence-design/fence` |  | Design line for the rail of a pedestrian fence along a footpath or a bridge. |

#### Master Strings

The master lines of a design that the others are built from. (10 codes)

| Code | Draws | Layer | Also | What it is |
|---|---|---|---|---|
| `MA*` | line `continuous`; `katana pavement` (#789295); 0.25 mm | `design/master-strings/carriageway` |  | Master string for the edge of a median, one of the strings that control the road model. |
| `MC*` | line `continuous`; `katana pavement` (#789295); 0.35 mm | `design/master-strings/carriageway` |  | Master control string, the main alignment from which the other road model strings are built. |
| `MD*` | line `continuous`; `katana pavement` (#789295); 0.25 mm | `design/master-strings/carriageway` |  | Master string for the edge of a driveway. |
| `ME*` | line `continuous`; `katana breakline` (#BE7D83); 0.25 mm | `design/master-strings/works` |  | Master string for the earthworks of the road model, such as batters and benching. |
| `MK*` | line `continuous`; `katana pavement` (#789295); 0.25 mm | `design/master-strings/carriageway` |  | Master string for the edge of the sealed bitumen surface. |
| `MP*` | line `continuous`; `katana pavement` (#789295); 0.25 mm | `design/master-strings/carriageway` |  | Master string for the edge of an off-road path. |
| `MS*` | line `Guard Rail Line`; `katana street` (#A2759D); 0.25 mm | `design/master-strings/works` |  | Master string for a safety barrier along the road. |
| `MT*` | line `continuous`; `katana pavement` (#789295); 0.25 mm | `design/master-strings/carriageway` |  | Master string for the edge of a traffic island or a turning head. |
| `MW*` | line `Earth Retaining Wall`; `katana wall` (#BA5556); 0.25 mm | `design/master-strings/works` |  | Master string for a retaining wall beside the road. |
| `MX*` | line `Temporary Works Line`; `katana hazard` (#FF5A37); 0.25 mm | `design/master-strings/works` |  | Master string for temporary works that the finished road does not keep. |

#### Design General

Designed paths, lights, walls and hazards that fit no other design group. (7 codes)

| Code | Draws | Layer | Also | What it is |
|---|---|---|---|---|
| `DY*` | line `continuous`; `katana pavement` (#789295); 0.25 mm | `design/design-general/path` |  | Design edge of a driveway. |
| `E8*` | line `continuous`; `katana pavement` (#789295); 0.25 mm | `design/design-general/path` |  | Edge of a designed path on the side nearest the road. |
| `E9*` | line `continuous`; `katana pavement` (#789295); 0.25 mm | `design/design-general/path` |  | Edge of a designed path on the side furthest from the road. |
| `HE*` | line `Left Wall Face Hatch Line`; `katana wall` (#BA5556); 0.13 mm | `design/design-general/wall-hatch` |  | Hatch ticks on the left of the line, showing which side of a designed wall is its face. |
| `HL*` | line `Hazard Boundary`; `katana hazard` (#FF5A37); 0.25 mm | `design/design-general/hazard` |  | Design line that marks the edge of a hazard area. |
| `HR*` | line `Right Wall Face Hatch Line`; `katana wall` (#BA5556); 0.13 mm | `design/design-general/wall-hatch` |  | Hatch ticks on the right of the line, showing which side of a designed wall is its face. |
| `PLIG*` | point; `katana street` (#A2759D); 0.25 mm | `design/design-general/lighting` |  | The designed position of a road lighting pole, for setting out. |

### The codes in alphabetical order

Every code once, for looking one up: the key, where it is catalogued and what it is.

| Code | Class and group | Meaning |
|---|---|---|
| `*` | (no class) | Any string that carries a Diameter attribute is also drawn as a round pipe section, measured down from its top (the obvert). |
| `1` | Survey Control and Annotation/Text and Notes | Plan text 1.5 mm high on paper, the finest size, for crowded detail. |
| `1*` | Survey Control and Annotation/Text and Notes | Layer and pen for 1.5 mm plan text, so it can be switched off with the other notes. |
| `2` | Survey Control and Annotation/Text and Notes | Plan text 2.5 mm high on paper, the everyday size for notes and labels. |
| `2*` | Survey Control and Annotation/Text and Notes | Layer and pen for 2.5 mm plan text, so it can be switched off with the other notes. |
| `3` | Survey Control and Annotation/Text and Notes | Plan text 3.5 mm high on paper, for prominent notes and sub-headings. |
| `3*` | Survey Control and Annotation/Text and Notes | Layer and pen for 3.5 mm plan text, so it can be switched off with the other notes. |
| `4` | Survey Control and Annotation/Text and Notes | Plan text 5 mm high on paper, large enough for headings. |
| `4*` | Survey Control and Annotation/Text and Notes | Layer and pen for 5.0 mm plan text, so it can be switched off with the other notes. |
| `5` | Survey Control and Annotation/Text and Notes | Plan text 7 mm high on paper, for titles and major headings. |
| `5*` | Survey Control and Annotation/Text and Notes | Layer and pen for 7.0 mm plan text, so it can be switched off with the other notes. |
| `7` | Survey Control and Annotation/Text and Notes | Plan text 10 mm high on paper, the largest size, for sheet titles. |
| `7*` | Survey Control and Annotation/Text and Notes | Layer and pen for 10.0 mm plan text, so it can be switched off with the other notes. |
| `8` | Survey Control and Annotation/Text and Notes | Parcel label text 2.5 mm high on paper: the lot number and the deposited plan number. |
| `8*` | Survey Control and Annotation/Text and Notes | Layer and pen for parcel label text, so lot and plan numbers can be switched off together. |
| `AB*` | Buildings and Structures/Structures | Foot of a bridge abutment, where its face meets the ground. |
| `AC*` | Street Furniture and Signs/Posts and Barriers | A fixed bollard, shot as a single point and drawn with the bollard symbol. |
| `AD*` | Design and Setting Out/Barrier and Fence Design | Intended position of a bollard on a design, set out as a point and drawn with the bollard symbol. |
| `AE*` | Roads and Pavements/Markings | A painted pavement arrow for a lane that goes straight on or turns left, shot along its axis. |
| `AG*` | Fences and Walls/Gates and Posts | A gate leaf between two posts, shot as a line from its hinge to its latch. |
| `AI*` | Roads and Pavements/Markings | A painted pavement arrow for a lane that goes straight on or turns right, shot along its axis. |
| `AL*` | Roads and Pavements/Markings | A painted pavement arrow for traffic that turns left, shot along its axis. |
| `AR*` | Roads and Pavements/Markings | A painted pavement arrow for traffic that turns right, shot along its axis. |
| `AS*` | Roads and Pavements/Markings | A painted pavement arrow for traffic that goes straight on, shot along its axis. |
| `AT*` | Buildings and Structures/Structures | The upper edge of a bridge abutment, shot along its crest. |
| `AU*` | Design and Setting Out/Bridge Design | Design line for the first hinge of a bridge abutment, a break in its cross-section. |
| `AV*` | Design and Setting Out/Bridge Design | Design line for the second hinge of a bridge abutment, a break in its cross-section. |
| `AW*` | Buildings and Structures/Buildings | The outer edge of an awning that projects over a footpath or a frontage. |
| `AY*` | Design and Setting Out/Bridge Design | Design line for the third hinge of a bridge abutment, a break in its cross-section. |
| `AZ*` | Design and Setting Out/Bridge Design | Design line for the fourth hinge of a bridge abutment, a break in its cross-section. |
| `B0*` | Kerbs and Drainage/Pipes and Culverts | A box culvert 150 mm high, drawn as its outline in plan. |
| `B1*` | Kerbs and Drainage/Pipes and Culverts | A box culvert 225 mm high, drawn as its outline in plan. |
| `B2*` | Kerbs and Drainage/Pipes and Culverts | A box culvert 300 mm high, drawn as its outline in plan. |
| `B3*` | Kerbs and Drainage/Pipes and Culverts | A box culvert 375 mm high, drawn as its outline in plan. |
| `B4*` | Kerbs and Drainage/Pipes and Culverts | A box culvert 450 mm high, drawn as its outline in plan. |
| `B6*` | Kerbs and Drainage/Pipes and Culverts | A box culvert 600 mm high, drawn as its outline in plan. |
| `B7*` | Kerbs and Drainage/Pipes and Culverts | A box culvert 750 mm high, drawn as its outline in plan. |
| `B9*` | Kerbs and Drainage/Pipes and Culverts | A box culvert 900 mm high, drawn as its outline in plan. |
| `BA*` | Design and Setting Out/Bridge Design | Design line along the face of a bridge abutment. |
| `BB*` | Terrain and Breaklines/Breaklines | The bottom edge of a bank, where the slope meets the flat ground below. |
| `BC*` | Boundaries and Cadastre/Lot Boundaries | A cadastral boundary compiled from plans and positioned to about one metre. |
| `BD*` | Design and Setting Out/Road Design Strings | A road design string with no named role, left free for a design package to give its own meaning. |
| `BE*` | Boundaries and Cadastre/Easements and Reserves | The limit of an easement, land that carries another party's right of use. |
| `BF*` | Design and Setting Out/Barrier and Fence Design | Design line for a W-beam or thrie-beam guard rail on the left-hand side of the alignment. |
| `BG*` | Design and Setting Out/Barrier and Fence Design | Design line for a W-beam or thrie-beam guard rail on the right-hand side of the alignment. |
| `BH*` | Street Furniture and Signs/Amenities | The roof outline of a bus shelter, shot round its edge. |
| `BI*` | Street Furniture and Signs/Amenities | A large waste bin such as a skip, shot round its outline and marked with the bin symbol. |
| `BK*` | Kerbs and Drainage/Kerbs | The back edge of a kerb, where it meets the verge or the footpath. |
| `BM*` | Boundaries and Cadastre/Lot Boundaries | A boundary taken from the digital cadastral map, whose position can be anywhere from one to twenty metres out. |
| `BN*` | Boundaries and Cadastre/Lot Boundaries | A cadastral boundary whose position is good to about half a metre. |
| `BO*` | Boundaries and Cadastre/Lot Boundaries | A cadastral boundary whose position is good to about a tenth of a metre. |
| `BP*` | Buildings and Structures/Structures | The plan outline of a bridge pier or column. |
| `BQ*` | Design and Setting Out/Bridge Design | Design line for the outline of a bridge pier. |
| `BR*` | Buildings and Structures/Structures | The edge of a bridge deck. |
| `BS*` | Roads and Pavements/Markings | A painted double line with an unbroken barrier line on one side and a broken separation line on the other. |
| `BT*` | Boundaries and Cadastre/Lot Boundaries | A title boundary measured on the ground and good to about two centimetres. |
| `BU*` | Buildings and Structures/Buildings | The outer face of a building wall. |
| `BV*` | Buildings and Structures/Buildings | The edge of a roof where it overhangs the wall. |
| `BW*` | Buildings and Structures/Buildings | The foot of a wall where it meets the ground, drawn as a hidden edge. |
| `BX*` | Kerbs and Drainage/Pipes and Culverts | A box culvert of any height, drawn as its outline in plan. |
| `BY*` | Boundaries and Cadastre/Lot Boundaries | A proposed boundary that marks land to be acquired. |
| `BZ*` | Boundaries and Cadastre/Lot Boundaries | The limit of a stratum parcel, which is fixed by a height or depth and not only by a line on the ground. |
| `C0*` | Design and Setting Out/Kerb and Carriageway Design | Design line for the edge of a median. |
| `C1*` | Design and Setting Out/Kerb and Carriageway Design | Design line for the first base line of a kerb, at its foot. |
| `C2*` | Design and Setting Out/Kerb and Carriageway Design | Design line for the second base line of a kerb, at its foot. |
| `C3*` | Design and Setting Out/Kerb and Carriageway Design | Design line for the first hinge of a kerb, a break in its cross-section. |
| `C4*` | Design and Setting Out/Kerb and Carriageway Design | Design line for the second hinge of a kerb, a break in its cross-section. |
| `C5*` | Design and Setting Out/Kerb and Carriageway Design | Design line for the third hinge of a kerb, a break in its cross-section. |
| `C6*` | Design and Setting Out/Kerb and Carriageway Design | Design line for the fourth hinge of a kerb, a break in its cross-section. |
| `C7*` | Design and Setting Out/Kerb and Carriageway Design | Design line for the fifth hinge of a kerb, a break in its cross-section. |
| `C8*` | Design and Setting Out/Kerb and Carriageway Design | Design line for the sixth hinge of a kerb, a break in its cross-section. |
| `C9*` | Design and Setting Out/Road Design Strings | A road design string with no named role, left free for a design package to give its own meaning. |
| `CA*` | Roads and Pavements/Markings | The zigzag line painted along the approach to a pedestrian crossing. |
| `CB*` | Utilities/Unknown Services | The bottom outline of an underground utility chamber, drawn as a hidden edge. |
| `CC*` | Roads and Pavements/Markings | A short-dash line that carries a lane through a junction, checked in check zone 1 (roads, bridges and drainage). |
| `CD*` | Roads and Pavements/Markings | An unbroken painted line on the pavement. |
| `CE*` | Design and Setting Out/Kerb and Carriageway Design | Design line for the edge of the sealed carriageway surface. |
| `CF*` | Design and Setting Out/Kerb and Carriageway Design | Design line along the flow line of a gutter. |
| `CG*` | Roads and Pavements/Markings | The give-way line painted across a turning lane. |
| `CH*` | Design and Setting Out/Kerb and Carriageway Design | Design line along the lip of the gutter at the edge of the through lane. |
| `CL*` | Design and Setting Out/Road Design Strings | A road design string with no named role, left free for a design package to give its own meaning. |
| `CM*` | Design and Setting Out/Road Design Strings | A road design string with no named role, left free for a design package to give its own meaning. |
| `CN*` | Roads and Pavements/Markings | A lane separation line with dashes and gaps of equal length, checked in check zone 1 (roads, bridges and drainage). |
| `CR*` | Design and Setting Out/Road Design Strings | A road design string with no named role, left free for a design package to give its own meaning. |
| `CS*` | Design and Setting Out/Road Design Strings | A road design string with no named role, left free for a design package to give its own meaning. |
| `CT*` | Utilities/Unknown Services | The top outline of an underground utility chamber. |
| `CU*` | Terrain and Breaklines/Breaklines | The top edge of a cutting, where the ground begins to fall into the cut. |
| `CV*` | Roads and Pavements/Markings | A row of painted chevrons that point to the left of travel, used to fill a painted island. |
| `CW*` | Roads and Pavements/Markings | The painted edge of a clearway, drawn as dashes and gaps of equal length. |
| `CX*` | Roads and Pavements/Markings | The painted lines that bound a pedestrian crossing controlled by traffic signals. |
| `CY*` | Roads and Pavements/Markings | A row of painted chevrons that point to the right of travel, used to fill a painted island. |
| `CZ*` | Roads and Pavements/Markings | Painted zebra stripes where pedestrians have right of way across the carriageway. |
| `D0*` | Kerbs and Drainage/Pipes and Culverts | Rectangular box culvert with a clear opening 1050 mm high. |
| `D1*` | Kerbs and Drainage/Pipes and Culverts | Rectangular box culvert with a clear opening 1200 mm high. |
| `D2*` | Kerbs and Drainage/Pipes and Culverts | Rectangular box culvert with a clear opening 1500 mm high. |
| `D3*` | Kerbs and Drainage/Pipes and Culverts | Rectangular box culvert with a clear opening 1800 mm high. |
| `D4*` | Kerbs and Drainage/Pipes and Culverts | Rectangular box culvert with a clear opening 2100 mm high. |
| `D5*` | Kerbs and Drainage/Pipes and Culverts | Rectangular box culvert with a clear opening 2400 mm high. |
| `D6*` | Kerbs and Drainage/Pipes and Culverts | Rectangular box culvert with a clear opening 2700 mm high. |
| `D7*` | Kerbs and Drainage/Pipes and Culverts | Rectangular box culvert with a clear opening 3000 mm high. |
| `D8*` | Kerbs and Drainage/Pipes and Culverts | Rectangular box culvert with a clear opening 3300 mm high. |
| `D9*` | Kerbs and Drainage/Pipes and Culverts | Rectangular box culvert with a clear opening 3600 mm high. |
| `DA*` | Roads and Pavements/Markings | Row of painted dragon's teeth with the points standing to the left of the way the line is shot. |
| `DB*` | Roads and Pavements/Markings | Pair of unbroken painted lines down a road that forbids crossing in either direction. |
| `DC*` | Roads and Pavements/Markings | Row of painted dragon's teeth with the points standing to the right of the way the line is shot. |
| `DD*` | Kerbs and Drainage/Kerbs | Shallow V-shaped concrete drain beside a road, shot along its lowest line. |
| `DE*` | Survey Control and Annotation/Plan Marks | Outer limit of the area digitised from aerial photography, where detail comes from the photo and not the field. |
| `DF*` | Kerbs and Drainage/Channels and Waterways | Galvanised iron flume that carries water down a batter, shot along its floor. |
| `DG*` | Utilities/Gas | Small gas pipe that runs from the street main to a property. |
| `DH*` | Design and Setting Out/Drainage Design | Design line for the wall that surrounds the end of a drain or culvert. |
| `DI*` | Design and Setting Out/Drainage Design | Design line for a stormwater drainage pipe. |
| `DJ*` | Design and Setting Out/Earthworks Design | Design edge of a table drain on the side furthest from the road. |
| `DK*` | Design and Setting Out/Earthworks Design | Design edge of a table drain on the side nearest the road. |
| `DL*` | Design and Setting Out/Drainage Design | Design line for a concrete or otherwise lined drain that catches runoff above a cutting. |
| `DM*` | Design and Setting Out/Earthworks Design | Line along a median designed to sit lower than the carriageways beside it. |
| `DO*` | Buildings and Structures/Buildings | Opening of a door in a building wall, drawn as a short line across the wall. |
| `DP*` | Kerbs and Drainage/Pits and Structures | Outline of a stormwater pit, drawn at its true size on the plan. |
| `DT*` | Kerbs and Drainage/Channels and Waterways | Broad shallow channel beside the road shoulder that collects runoff, shot along its floor. |
| `DU*` | Design and Setting Out/Drainage Design | Design line for an earth drain with no lining that catches runoff above a cutting. |
| `DV*` | Design and Setting Out/Drainage Design | Design line for a culvert that carries water under a road or embankment. |
| `DW*` | Roads and Pavements/Paths and Access | Edge of a driveway that gives a lot access across the footpath to the road. |
| `DX*` | Kerbs and Drainage/Pits and Structures | Outline of a rectangular box in the drainage system, such as a junction or inlet box. |
| `DY*` | Design and Setting Out/Design General | Design edge of a driveway. |
| `DZ*` | Kerbs and Drainage/Pipes and Culverts | Box culvert traced from imagery and not measured on the ground. |
| `E0*` | Design and Setting Out/Road Design Strings | Road design string of a kind that has no recorded purpose, kept so that a design file still plots. |
| `E1*` | Design and Setting Out/Earthworks Design | Outer, downslope edge of a bench cut into a slope in the earthworks design. |
| `E2*` | Design and Setting Out/Earthworks Design | Inner edge of a bench, where it meets the rising slope behind. |
| `E3*` | Design and Setting Out/Road Design Strings | Road design string of a kind that has no recorded purpose, kept so that a design file still plots. |
| `E4*` | Design and Setting Out/Road Design Strings | Road design string of a kind that has no recorded purpose, kept so that a design file still plots. |
| `E5*` | Design and Setting Out/Road Design Strings | Road design string of a kind that has no recorded purpose, kept so that a design file still plots. |
| `E6*` | Design and Setting Out/Road Design Strings | Road design string of a kind that has no recorded purpose, kept so that a design file still plots. |
| `E7*` | Design and Setting Out/Road Design Strings | Road design string of a kind that has no recorded purpose, kept so that a design file still plots. |
| `E8*` | Design and Setting Out/Design General | Edge of a designed path on the side nearest the road. |
| `E9*` | Design and Setting Out/Design General | Edge of a designed path on the side furthest from the road. |
| `EA*` | Utilities/Electricity | Buried conductor that bonds electrical equipment to the earth. |
| `EB*` | Design and Setting Out/Road Design Strings | Road design string of a kind that has no recorded purpose, kept so that a design file still plots. |
| `EC*` | Utilities/Electricity | Plan outline of the cabinet that houses a transformer. |
| `ED*` | Utilities/Electricity | Buried conduit that carries electrical cable. |
| `EF*` | Design and Setting Out/Road Design Strings | Road design string of a kind that has no recorded purpose, kept so that a design file still plots. |
| `EG*` | Vegetation and Landscape/Ground Cover | Edge of a garden bed where planting meets lawn or paving. |
| `EH*` | Design and Setting Out/Earthworks Design | Line where the formation turns into the batter slope, the hinge of the cross-section. |
| `EJ*` | Roads and Pavements/Carriageway | Outer edge of the graded formation that carries a road or track, before any pavement is laid. |
| `EK*` | Roads and Pavements/Carriageway | Edge of a vehicle track, worn or cut into the ground and not made up as a road. |
| `EL*` | Utilities/Electricity | Outline of the footing under one leg of a transmission pylon. |
| `EM*` | Roads and Pavements/Carriageway | Edge of the central median that separates opposing lanes of traffic. |
| `EN*` | Utilities/Electricity | Outline of a sump that collects water or oil at an electricity installation. |
| `EP*` | Roads and Pavements/Carriageway | Outer edge of the sealed road surface, shot where the seal meets the shoulder or kerb. |
| `ER*` | Design and Setting Out/Road Design Strings | Road design string of a kind that has no recorded purpose, kept so that a design file still plots. |
| `ES*` | Design and Setting Out/Kerb and Carriageway Design | Outer edge of the road shoulder as designed, on the side away from the traffic lanes. |
| `ET*` | Design and Setting Out/Road Design Strings | Road design string of a kind that has no recorded purpose, kept so that a design file still plots. |
| `EU*` | Utilities/Electricity | Buried electricity line, located by locator or from records. |
| `EV*` | Design and Setting Out/Earthworks Design | Outer edge of the verge as designed, on the side furthest from the road. |
| `EW*` | Design and Setting Out/Earthworks Design | Edge of a designed footpath on the side nearest the road. |
| `EX*` | Design and Setting Out/Earthworks Design | Edge of a designed footway on the side furthest from the road. |
| `EY*` | Utilities/Electricity | Small electricity cable from the street network to a property. |
| `EZ*` | Utilities/Electricity | Electricity line traced from imagery or plans and not located on the ground. |
| `FC*` | Fences and Walls/Fences | Fence of a construction that no other fence code covers. |
| `FE*` | Fences and Walls/Fences | Ordinary property or paddock fence of wire on posts. |
| `FG*` | Street Furniture and Signs/Posts and Barriers | Steel beam barrier on posts that guards the edge of a road. |
| `FI*` | Kerbs and Drainage/Kerbs | Lowest line of the gutter beside a kerb, along which stormwater runs. |
| `FL*` | Fences and Walls/Fences | Fence of unrecorded construction, shot as a plain line. |
| `FM*` | Fences and Walls/Fences | Fence built to keep people out, too high or too close in its mesh to climb. |
| `FN*` | Design and Setting Out/Barrier and Fence Design | Design line for a fence along a property boundary. |
| `FO*` | Kerbs and Drainage/Channels and Waterways | Place where a road or track runs through a watercourse without a bridge or culvert. |
| `FP*` | Roads and Pavements/Paths and Access | Edge of a footpath for pedestrians beside the road. |
| `FS*` | Survey Control and Annotation/Plan Marks | Outer limit of the ground that was surveyed in the field, drawn over a photo base. |
| `FU*` | Utilities/Fuel | Buried pipeline for a fuel that has no code of its own. |
| `FW*` | Street Furniture and Signs/Posts and Barriers | Safety barrier of tensioned steel cables on posts beside or between traffic lanes. |
| `FZ*` | Utilities/Fuel | Fuel pipeline of an unlisted kind, traced from imagery and not located on the ground. |
| `HA*` | Utilities/Gas | Buried pipeline that carries ethane. |
| `HB*` | Kerbs and Drainage/Pits and Structures | Line along the foot of a headwall where it meets the ground or the pipe invert. |
| `HE*` | Design and Setting Out/Design General | Hatch ticks on the left of the line, showing which side of a designed wall is its face. |
| `HG*` | Utilities/Gas | Gas main that works at high pressure. |
| `HL*` | Design and Setting Out/Design General | Design line that marks the edge of a hazard area. |
| `HO*` | Utilities/Fuel | Pipeline that carries oil at high pressure. |
| `HP*` | Miscellaneous/General | Outline of an area singled out for attention; the code does not say why. |
| `HR*` | Design and Setting Out/Design General | Hatch ticks on the right of the line, showing which side of a designed wall is its face. |
| `HS*` | Buildings and Structures/Structures | Beam across the top of a bridge pier or pile that carries the girders. |
| `HW*` | Kerbs and Drainage/Pits and Structures | Line along the top of a headwall that surrounds the end of a pipe or culvert. |
| `HZ*` | Utilities/Fuel | High pressure oil pipeline traced from imagery and not located on the ground. |
| `I0*` | Design and Setting Out/Road Design Strings | Road design string of a kind that has no recorded purpose, kept so that a design file still plots. |
| `I1*` | Design and Setting Out/Road Design Strings | Road design string of a kind that has no recorded purpose, kept so that a design file still plots. |
| `I2*` | Design and Setting Out/Road Design Strings | Road design string of a kind that has no recorded purpose, kept so that a design file still plots. |
| `I3*` | Design and Setting Out/Road Design Strings | Road design string of a kind that has no recorded purpose, kept so that a design file still plots. |
| `I4*` | Design and Setting Out/Road Design Strings | Road design string of a kind that has no recorded purpose, kept so that a design file still plots. |
| `I5*` | Design and Setting Out/Road Design Strings | Road design string of a kind that has no recorded purpose, kept so that a design file still plots. |
| `I6*` | Design and Setting Out/Road Design Strings | Road design string of a kind that has no recorded purpose, kept so that a design file still plots. |
| `I7*` | Design and Setting Out/Road Design Strings | Road design string of a kind that has no recorded purpose, kept so that a design file still plots. |
| `I8*` | Design and Setting Out/Road Design Strings | A road design string with no role assigned yet; the first of two spare classes held for the designer's own use. |
| `I9*` | Design and Setting Out/Road Design Strings | A road design string with no role assigned yet; the second of two spare classes held for the designer's own use. |
| `IA*` | Design and Setting Out/Earthworks Design | Design line where an earthworks batter meets the existing ground, the catch point of a cut or a fill. |
| `IB*` | Design and Setting Out/Earthworks Design | Design line along the front edge of a bench cut into an earthworks batter. |
| `IC*` | Design and Setting Out/Earthworks Design | A second design string for the front edge of an earthworks bench, drawn as a plain line. |
| `ID*` | Design and Setting Out/Earthworks Design | Design line along the front edge of a table drain, the side nearest the road, where it meets the earthworks. |
| `IE*` | Design and Setting Out/Earthworks Design | Design line along the back edge of a table drain, on the side away from the road. |
| `IP*` | Kerbs and Drainage/Pits and Structures | Outline of a stormwater gully pit shot as a closed line instead of a single point. |
| `IT*` | Utilities/Traffic Systems | A cable of a road traffic management system, such as signals or cameras, shot as a line along its route. |
| `JC*` | Vegetation and Landscape/Ground Cover | Edge of cultivated land or pasture, shot as a line round the paddock. |
| `JD*` | Vegetation and Landscape/Ground Cover | Edge of dense timber or scrub, shot along the line of the outermost trunks. |
| `JG*` | Vegetation and Landscape/Ground Cover | Edge of grassland, shot as a line round the grassed area. |
| `JL*` | Boundaries and Cadastre/Lot Boundaries | A join line between two boundary points, giving a bearing and distance; it is not itself a boundary. |
| `JM*` | Vegetation and Landscape/Ground Cover | Edge of medium-density timber or scrub, where the trees stand more widely spaced than in dense bush. |
| `JO*` | Vegetation and Landscape/Ground Cover | Outline of an orchard, shot round the outer rows of trees. |
| `JR*` | Terrain and Breaklines/Rock and Cliff | Outline of an area of exposed rock, shot along its edge. |
| `JS*` | Vegetation and Landscape/Ground Cover | Edge of scattered timber or scrub, where single trees stand well apart. |
| `JW*` | Terrain and Breaklines/Water Edges | Outline of a swamp or marsh, shot along the edge of the wet ground. |
| `KB*` | Kerbs and Drainage/Kerbs | Top edge of a kerb, shot on the road side and checked in model zone 3. |
| `KI*` | Kerbs and Drainage/Pits and Structures | A kerb inlet shot as a line along the kerb: the opening where gutter water enters the stormwater system. |
| `KJ*` | Street Furniture and Signs/Posts and Barriers | A concrete safety barrier of the F-shaped profile, shot as a line along its base. |
| `KR*` | Roads and Pavements/Paths and Access | A pedestrian kerb ramp, or pram ramp, shot as a line along its edge. |
| `L0*` | Design and Setting Out/Lane Marking Design | Separation line along a bicycle path, design type S4. |
| `L2*` | Design and Setting Out/Lane Marking Design | Dividing line between opposing lanes of a multi-lane road, design type S2. |
| `L3*` | Design and Setting Out/Lane Marking Design | Lane line between lanes travelling the same way on a multi-lane road, design type L1. |
| `L4*` | Design and Setting Out/Lane Marking Design | Lane line between the lanes of a multi-lane roundabout, design type L4. |
| `L5*` | Design and Setting Out/Lane Marking Design | Separation line along a bicycle path, design type S5. |
| `L6*` | Design and Setting Out/Lane Marking Design | Lane line beside a bus lane, design type L6. |
| `L7*` | Design and Setting Out/Lane Marking Design | Lane line beside a bicycle lane, design type L7. |
| `L8*` | Design and Setting Out/Lane Marking Design | Dividing line between opposing lanes of a two-lane road, design type S1. |
| `L9*` | Design and Setting Out/Lane Marking Design | Dividing line between opposing lanes of a two-lane road, design type S3. |
| `LA*` | Design and Setting Out/Lane Marking Design | Continuity line, design type C1, that carries a lane edge across a junction. |
| `LB*` | Buildings and Structures/Buildings | Edge of a loading bay or loading dock, shot as a line along the raised platform. |
| `LD*` | Design and Setting Out/Lane Marking Design | Double barrier line, design type BB: two unbroken lines that forbid crossing in either direction. |
| `LE*` | Design and Setting Out/Lane Marking Design | Edge line along the side of the carriageway, in its general form. |
| `LF*` | Design and Setting Out/Lane Marking Design | Continuity line, design type C4, that carries a bicycle lane across a junction. |
| `LG*` | Utilities/Gas | A buried gas main that works at low pressure, shot as a line along its route. |
| `LH*` | Design and Setting Out/Lane Marking Design | Combined barrier and separation line, design type BS1: an unbroken line beside a broken one. |
| `LI*` | Street Furniture and Signs/Lighting | Outreach arm of a street light, shot from the pole to the lantern; it is kept out of the ground surface. |
| `LJ*` | Design and Setting Out/Lane Marking Design | Kerbside line marking a clearway along the kerb, design type C2. |
| `LK*` | Design and Setting Out/Lane Marking Design | Kerbside line marking a no-stopping length of kerb, design type C3. |
| `LL*` | Roads and Pavements/Markings | Lane line painted in 3 metre dashes with 9 metre gaps, shot as built. |
| `LM*` | Design and Setting Out/Lane Marking Design | Edge line round a painted median, design type E5. |
| `LN*` | Design and Setting Out/Lane Marking Design | Lane line between lanes travelling the same way on a multi-lane road, design type L3. |
| `LP*` | Kerbs and Drainage/Kerbs | Lip of the gutter beside a kerb, shot along its line and checked in model zone 1. |
| `LR*` | Design and Setting Out/Lane Marking Design | Combined barrier and separation line, design type BS2, the mirror form of BS1. |
| `LS*` | Design and Setting Out/Lane Marking Design | Stop line at traffic signals, design type TF. |
| `LT*` | Design and Setting Out/Lane Marking Design | Turn line, design type T1, that guides turning traffic through a junction. |
| `LU*` | Design and Setting Out/Lane Marking Design | A plain unbroken painted line that fits none of the named types. |
| `LW*` | Design and Setting Out/Lane Marking Design | Give way line, design type TB, painted across the approach to a junction. |
| `LY*` | Design and Setting Out/Lane Marking Design | A bicycle crossing painted across the carriageway. |
| `LZ*` | Design and Setting Out/Lane Marking Design | A zebra pedestrian crossing painted across the carriageway. |
| `MA*` | Design and Setting Out/Master Strings | Master string for the edge of a median, one of the strings that control the road model. |
| `MC*` | Design and Setting Out/Master Strings | Master control string, the main alignment from which the other road model strings are built. |
| `MD*` | Design and Setting Out/Master Strings | Master string for the edge of a driveway. |
| `ME*` | Design and Setting Out/Master Strings | Master string for the earthworks of the road model, such as batters and benching. |
| `MK*` | Design and Setting Out/Master Strings | Master string for the edge of the sealed bitumen surface. |
| `MP*` | Design and Setting Out/Master Strings | Master string for the edge of an off-road path. |
| `MS*` | Design and Setting Out/Master Strings | Master string for a safety barrier along the road. |
| `MT*` | Design and Setting Out/Master Strings | Master string for the edge of a traffic island or a turning head. |
| `MW*` | Design and Setting Out/Master Strings | Master string for a retaining wall beside the road. |
| `MX*` | Design and Setting Out/Master Strings | Master string for temporary works that the finished road does not keep. |
| `NB*` | Terrain and Breaklines/Breaklines | A breakline or ridge, shot as a line that the surface model must follow. |
| `NF*` | Survey Control and Annotation/Plan Marks | Limit of an area where photo-based mapping was completed or checked by survey in the field. |
| `NG*` | Utilities/Gas | A buried gas main laid in nylon pipe, shot as a line along its route. |
| `NJ*` | Design and Setting Out/Kerb and Carriageway Design | Design line along the joint between two pavement areas, such as new work meeting existing. |
| `NL*` | Design and Setting Out/Kerb and Carriageway Design | Design line for the crown of a carriageway that is offset from the road's master alignment. |
| `NS*` | Terrain and Breaklines/Breaklines | A line of ground shots along the natural surface where there is no sharper break, checked in model zone 2. |
| `OA*` | Utilities/Communications | Optical fibre cable strung above ground on poles, shot as a line along its route. |
| `OB*` | Miscellaneous/Land Use Areas | General outline of a built-up area, drawn as one extent and not building by building. |
| `OC*` | Miscellaneous/Land Use Areas | Outline of a cemetery or burial ground. |
| `OD*` | Utilities/Communications | A conduit that carries optical fibre cable, shot as a line along its route. |
| `OF*` | Roads and Pavements/Carriageway | Crown of a carriageway shot where it is offset from the road centreline, checked in model zone 1. |
| `OG*` | Miscellaneous/Land Use Areas | Outline of a golf course. |
| `OH*` | Miscellaneous/Land Use Areas | Outline of a stockpile of loose material, shot round its toe. |
| `OM*` | Buildings and Structures/Structures | Outline of a structure that no other code names. |
| `OO*` | Miscellaneous/Land Use Areas | Outline of a sporting arena, such as a stadium, an oval or a court. |
| `OP*` | Miscellaneous/Land Use Areas | Outline of a park or other open space. |
| `OR*` | Buildings and Structures/Buildings | Outline of a ruin, the remains of a building that is no longer whole. |
| `OS*` | Buildings and Structures/Structures | Outline of a silo or tank, seen from above. |
| `OT*` | Buildings and Structures/Structures | Outline of a swimming pool. |
| `OU*` | Utilities/Communications | Optical fibre cable buried in the ground, shot as a line along its route. |
| `OW*` | Buildings and Structures/Structures | Outline of a windmill, shot round its base. |
| `OZ*` | Utilities/Communications | An optical fibre route digitised from records and not located on site. |
| `PABB` | Buildings and Structures/Structures | Survey point on the bottom of a bridge abutment. |
| `PABT` | Buildings and Structures/Structures | Survey point on the top of a bridge abutment. |
| `PAPN` | Survey Control and Annotation/Control Marks | An alignment pin: a pin set in the ground to mark a design alignment for setting out. |
| `PAPN*` | Survey Control and Annotation/Control Marks | Alignment pins that carry a string number, which are kept out of the surface model. |
| `PAWN` | Buildings and Structures/Buildings | A survey point on the edge of an awning. |
| `PAWN*` | Buildings and Structures/Buildings | Awning points that carry a string number, which are kept out of the surface model. |
| `PBCP` | Buildings and Structures/Structures | Point where a bridge pile meets its cap. |
| `PBDY` | Boundaries and Cadastre/Boundary Marks | A boundary mark of any kind, shot as a point. |
| `PBHX` | Survey Control and Annotation/Investigation Points | A borehole, shot at the centre of its collar. |
| `PBIN` | Street Furniture and Signs/Amenities | A rubbish bin, shot at its centre. |
| `PBLT` | Survey Control and Annotation/Control Marks | A bolt set in concrete or rock as a survey mark, shot at the centre of its head. |
| `PBLT*` | Survey Control and Annotation/Control Marks | Survey bolts that carry a string number, which are kept out of the surface model. |
| `PBMK` | Survey Control and Annotation/Control Marks | A bench mark: a mark of known height that levelling is run from. |
| `PBMZ` | Survey Control and Annotation/Control Marks | A bench mark kept under its own separate code so it can be listed and checked on its own. |
| `PBPG` | Boundaries and Cadastre/Boundary Marks | A peg set at a boundary corner, shot at its top. |
| `PBPG*` | Boundaries and Cadastre/Boundary Marks | Keeps boundary pegs out of the ground surface when the code carries a string number. |
| `PBPI` | Buildings and Structures/Structures | The centre of a bridge pier or column, shot where it meets the ground. |
| `PBRK` | Terrain and Breaklines/Breaklines | A ground point lying on a breakline, shot where the grade changes so that the surface model can join it to its neighbours. |
| `PBUS` | Street Furniture and Signs/Amenities | The position of a bus stop, shot at the foot of its sign post. |
| `PCAM` | Street Furniture and Signs/Traffic Control | A speed, red-light or traffic enforcement camera, shot at its pole. |
| `PCBU` | Buildings and Structures/Buildings | A building corner shot at natural surface level, where the wall meets the ground. |
| `PCFU` | Street Furniture and Signs/Traffic Control | The flash lamp that lights a vehicle for an enforcement camera, shot at its pole. |
| `PCMK` | Survey Control and Annotation/Control Marks | A permanent control mark of the road authority, used to tie road work into the survey network. |
| `PCMK*` | Survey Control and Annotation/Control Marks | Keeps the road authority's control marks out of the ground surface when the code carries a string number. |
| `PCRM` | Boundaries and Cadastre/Boundary Marks | A mark set to reference cadastral corners, so that the corners can be re-established if their own marks are lost. |
| `PCTV` | Street Furniture and Signs/Traffic Control | A closed-circuit television camera watching the road, shot at the pole that carries it. |
| `PDHL` | Boundaries and Cadastre/Boundary Marks | A hole drilled in rock or concrete, with a chiselled wing cut beside it to make it easy to find. |
| `PDHL*` | Boundaries and Cadastre/Boundary Marks | Keeps winged drill holes out of the ground surface when the code carries a string number. |
| `PDJM` | Kerbs and Drainage/Pits and Structures | A stormwater maintenance hole at the point where two or more drainage lines meet. |
| `PDPY` | Survey Control and Annotation/Control Marks | A temporary peg driven into the ground to carry a height for a dumpy level. |
| `PDPY*` | Survey Control and Annotation/Control Marks | Keeps levelling pegs out of the ground surface when the code carries a string number. |
| `PDSM` | Survey Control and Annotation/Control Marks | A permanent survey mark placed by the water authority and used as a height or position reference. |
| `PDSM*` | Survey Control and Annotation/Control Marks | Keeps the water authority's survey marks out of the ground surface when the code carries a string number. |
| `PECM` | Utilities/Electricity | A marker on the surface showing the route of a buried power cable. |
| `PEFP` | Utilities/Electricity | A point where a fuse protects a distribution cable, on a pole or in a cabinet. |
| `PEJB` | Utilities/Electricity | A box in which electricity cables are joined, shot at its lid. |
| `PEMH` | Utilities/Electricity | A manhole giving access to underground electricity cables, shot at its cover. |
| `PETC` | Utilities/Electricity | The centre of a ground-mounted cabinet that houses an electricity transformer. |
| `PEUP` | Utilities/Electricity | A pillar above ground where a customer is supplied from the underground power cable. |
| `PEWW` | Kerbs and Drainage/Pits and Structures | The free end of a wing wall that flanks a culvert or a drain outlet. |
| `PF01` | Vegetation and Landscape/Trees | A tree whose crown spreads about one metre across, drawn as a canopy ring of that size. |
| `PF01*` | Vegetation and Landscape/Trees | Keeps the one-metre canopy marks out of the ground surface when the code carries a string number. |
| `PF02` | Vegetation and Landscape/Trees | A tree whose crown spreads about two metres across, drawn as a canopy ring of that size. |
| `PF02*` | Vegetation and Landscape/Trees | Keeps the two-metre canopy marks out of the ground surface when the code carries a string number. |
| `PF03` | Vegetation and Landscape/Trees | A tree whose crown spreads about three metres across, drawn as a canopy ring of that size. |
| `PF03*` | Vegetation and Landscape/Trees | Keeps the three-metre canopy marks out of the ground surface when the code carries a string number. |
| `PF04` | Vegetation and Landscape/Trees | A tree whose crown spreads about four metres across, drawn as a canopy ring of that size. |
| `PF04*` | Vegetation and Landscape/Trees | Keeps the four-metre canopy marks out of the ground surface when the code carries a string number. |
| `PF05` | Vegetation and Landscape/Trees | A tree whose crown spreads about five metres across, drawn as a canopy ring of that size. |
| `PF05*` | Vegetation and Landscape/Trees | Keeps the five-metre canopy marks out of the ground surface when the code carries a string number. |
| `PF06` | Vegetation and Landscape/Trees | A tree whose crown spreads about six metres across, drawn as a canopy ring of that size. |
| `PF06*` | Vegetation and Landscape/Trees | Keeps the six-metre canopy marks out of the ground surface when the code carries a string number. |
| `PF07` | Vegetation and Landscape/Trees | A tree whose crown spreads about seven metres across, drawn as a canopy ring of that size. |
| `PF07*` | Vegetation and Landscape/Trees | Keeps the seven-metre canopy marks out of the ground surface when the code carries a string number. |
| `PF08` | Vegetation and Landscape/Trees | A tree whose crown spreads about eight metres across, drawn as a canopy ring of that size. |
| `PF08*` | Vegetation and Landscape/Trees | Keeps the eight-metre canopy marks out of the ground surface when the code carries a string number. |
| `PF09` | Vegetation and Landscape/Trees | A tree whose crown spreads about nine metres across, drawn as a canopy ring of that size. |
| `PF09*` | Vegetation and Landscape/Trees | Keeps the nine-metre canopy marks out of the ground surface when the code carries a string number. |
| `PF10` | Vegetation and Landscape/Trees | A tree whose crown spreads about ten metres across, drawn as a canopy ring of that size. |
| `PF10*` | Vegetation and Landscape/Trees | Keeps the ten-metre canopy marks out of the ground surface when the code carries a string number. |
| `PF12` | Vegetation and Landscape/Trees | A tree whose crown spreads about twelve metres across, drawn as a canopy ring of that size. |
| `PF12*` | Vegetation and Landscape/Trees | Keeps the twelve-metre canopy marks out of the ground surface when the code carries a string number. |
| `PF15` | Vegetation and Landscape/Trees | A tree whose crown spreads about fifteen metres across, drawn as a canopy ring of that size. |
| `PF15*` | Vegetation and Landscape/Trees | Keeps the fifteen-metre canopy marks out of the ground surface when the code carries a string number. |
| `PFHT` | Terrain and Breaklines/Water Edges | The level that a flood is known to have reached, shot at the mark it left. |
| `PFLG` | Buildings and Structures/Structures | A flagpole, shot at its base. |
| `PFLR` | Buildings and Structures/Buildings | A spot level taken on the finished floor of a building. |
| `PFLR*` | Buildings and Structures/Buildings | Keeps floor levels out of the ground surface when the code carries a string number. |
| `PFPL` | Street Furniture and Signs/Amenities | A built fireplace, such as a barbecue hearth, in a park, camp or picnic area. |
| `PFPM` | Utilities/Fuel | A marker on the surface showing the route of a buried fuel pipeline. |
| `PGAS` | Utilities/Gas | The box over a gas valve, shot at its lid. |
| `PGAT` | Utilities/Unknown Services | The lid of a recessed access cover that serves a service nobody has identified. |
| `PGEO` | Survey Control and Annotation/Investigation Points | A pit excavated to inspect the ground or to expose a buried service, shot at its centre. |
| `PGHL` | Utilities/Gas | A manhole giving access to a gas main, shot at its cover. |
| `PGHM` | Utilities/Gas | A marker on the surface showing the route of a high-pressure gas main. |
| `PGIN` | Boundaries and Cadastre/Boundary Marks | A galvanised iron nail driven into a hard surface as a survey mark. |
| `PGIN*` | Boundaries and Cadastre/Boundary Marks | Keeps galvanised nails out of the ground surface when the code carries a string number. |
| `PGMR` | Utilities/Gas | A gas meter, shot at its centre. |
| `PGPI` | Boundaries and Cadastre/Boundary Marks | A galvanised iron pipe driven in as a survey mark. |
| `PGPI*` | Boundaries and Cadastre/Boundary Marks | Keeps galvanised pipes out of the ground surface when the code carries a string number. |
| `PGPM` | Utilities/Gas | A marker on the surface showing the route of a buried gas main. |
| `PGRB` | Utilities/Gas | The cabinet that houses a gas pressure regulator, shot at the middle of the box. |
| `PGTP` | Utilities/Gas | A surface fitting on a gas pipeline where its protection or pressure can be tested. |
| `PGUL` | Kerbs and Drainage/Pits and Structures | A stormwater gully pit that takes surface runoff, shot at the middle of its grate. |
| `PGVP` | Utilities/Gas | A vent riser on a gas main or casing that lets gas escape safely to the open air. |
| `PHAZ` | Miscellaneous/Hazards and Heritage | A site that endangers people or later works, such as contaminated or unstable ground. |
| `PHAZ*` | Miscellaneous/Hazards and Heritage | Keeps hazardous-site points out of the ground surface. |
| `PHCP` | Survey Control and Annotation/Control Marks | A control point held in plan only, with no level, used to control photogrammetry. |
| `PHEC` | Miscellaneous/Hazards and Heritage | A point inside a plant or animal community recorded as endangered, to be avoided by works. |
| `PHEC*` | Miscellaneous/Hazards and Heritage | Keeps endangered-community points out of the ground surface. |
| `PHEE` | Miscellaneous/Hazards and Heritage | A point inside a vegetation community listed by law as an endangered ecological community. |
| `PHEE*` | Miscellaneous/Hazards and Heritage | Keeps listed ecological community points out of the ground surface. |
| `PHIA` | Miscellaneous/Hazards and Heritage | A place of Indigenous cultural heritage, marked so that works can keep clear of it. |
| `PHIA*` | Miscellaneous/Hazards and Heritage | Keeps Indigenous heritage points out of the ground surface. |
| `PHIS` | Miscellaneous/Hazards and Heritage | A point of historic interest, such as a plaque, a marker stone or an old relic. |
| `PHNI` | Miscellaneous/Hazards and Heritage | A heritage site without Indigenous significance, such as a historic ruin, relic or building remnant. |
| `PHNI*` | Miscellaneous/Hazards and Heritage | Keeps non-Indigenous heritage points out of the ground surface. |
| `PHNL` | Survey Control and Annotation/Investigation Points | A pothole dug to expose a service, recorded without a level on the service. |
| `PHNL*` | Survey Control and Annotation/Investigation Points | Keeps pothole points without a level out of the ground surface. |
| `PHRL` | Survey Control and Annotation/Investigation Points | A pothole dug to expose a service, with the level of the exposed service recorded. |
| `PHST` | Miscellaneous/Hazards and Heritage | A grave headstone in a cemetery, shot at the middle of its base. |
| `PHTT` | Utilities/Electricity | A steel lattice tower that carries high-voltage lines, shot at the centre of its footprint. |
| `PHWB` | Kerbs and Drainage/Pits and Structures | The foot of a pipe headwall, shot where the wall meets the pipe or the ground. |
| `PHWT` | Kerbs and Drainage/Pits and Structures | The top edge of a pipe headwall, shot on its coping. |
| `PI01` | Kerbs and Drainage/Pipes and Culverts | The invert of a 225 mm stormwater pipe, shot at its lowest inside point. |
| `PI02` | Kerbs and Drainage/Pipes and Culverts | The invert of a 300 mm stormwater pipe, shot at its lowest inside point. |
| `PI03` | Kerbs and Drainage/Pipes and Culverts | The invert of a 375 mm stormwater pipe, shot at its lowest inside point. |
| `PI04` | Kerbs and Drainage/Pipes and Culverts | The invert of a 450 mm stormwater pipe, shot at its lowest inside point. |
| `PI05` | Kerbs and Drainage/Pipes and Culverts | The invert of a 525 mm stormwater pipe, shot at its lowest inside point. |
| `PI06` | Kerbs and Drainage/Pipes and Culverts | The invert of a 600 mm stormwater pipe, shot at its lowest inside point. |
| `PI07` | Kerbs and Drainage/Pipes and Culverts | The invert of a 750 mm stormwater pipe, shot at its lowest inside point. |
| `PI09` | Kerbs and Drainage/Pipes and Culverts | The invert of a 900 mm stormwater pipe, shot at its lowest inside point. |
| `PI10` | Kerbs and Drainage/Pipes and Culverts | The invert of a 1050 mm stormwater pipe, shot at its lowest inside point. |
| `PI12` | Kerbs and Drainage/Pipes and Culverts | The invert of a 1200 mm stormwater pipe, shot at its lowest inside point. |
| `PI13` | Kerbs and Drainage/Pipes and Culverts | The invert of a 1350 mm stormwater pipe, shot at its lowest inside point. |
| `PI15` | Kerbs and Drainage/Pipes and Culverts | The invert of a 1500 mm stormwater pipe, shot at its lowest inside point. |
| `PI16` | Kerbs and Drainage/Pipes and Culverts | The invert of a 1650 mm stormwater pipe, shot at its lowest inside point. |
| `PI18` | Kerbs and Drainage/Pipes and Culverts | The invert of a 1800 mm stormwater pipe, shot at its lowest inside point. |
| `PILT` | Kerbs and Drainage/Pits and Structures | The point where drainage flows into a sump, shot at the mouth of the inlet. |
| `PINV` | Kerbs and Drainage/Pipes and Culverts | The lowest inside point of a stormwater pipe, shot where it meets a pit; the diameter is asked for. |
| `PJBX` | Kerbs and Drainage/Pits and Structures | The top of a concrete junction box on a stormwater line, where pipes meet. |
| `PKME` | Street Furniture and Signs/Amenities | A kerbside meter that collects the fee for a parking bay. |
| `PLGN` | Street Furniture and Signs/Lighting | A low light standing in a garden bed or on a lawn, shot at its base. |
| `PLIG*` | Design and Setting Out/Design General | The designed position of a road lighting pole, for setting out. |
| `PLPL` | Street Furniture and Signs/Lighting | A pole that carries a light, shot at the centre of the pole at ground level. |
| `PLSU` | Street Furniture and Signs/Lighting | A light hung from an overhead wire or bracket, shot on the ground directly below it. |
| `PMBX` | Street Furniture and Signs/Amenities | A letterbox at the property frontage, shot at the middle of its base. |
| `PMCP` | Survey Control and Annotation/Control Marks | A minor control point set to support the main control network. |
| `PMPT` | Utilities/Communications | A transmitter mast or tower of the mobile telephone network, shot at its base. |
| `PMSM` | Boundaries and Cadastre/Boundary Marks | A survey mark of a kind no other code covers, shot at its centre. |
| `PMSM*` | Boundaries and Cadastre/Boundary Marks | Keeps miscellaneous survey marks out of the ground surface. |
| `PNAL` | Boundaries and Cadastre/Boundary Marks | A nail driven into a hard surface as a survey mark; each shot also records its code. |
| `PNSS` | Terrain and Breaklines/Ground Points | A shot on the natural ground surface, used as a point of the terrain model. |
| `POBV` | Kerbs and Drainage/Pipes and Culverts | The obvert of a stormwater pipe, the highest point on its inside, shot at the crown. |
| `POBV*` | Kerbs and Drainage/Pipes and Culverts | Keeps pipe obvert shots out of the ground surface, since they lie below it. |
| `POCP` | Boundaries and Cadastre/Boundary Marks | A post standing at a boundary corner, taken as the corner mark. |
| `POFJ` | Utilities/Communications | A junction box on an optical fibre route where cables are joined or split. |
| `POFM` | Utilities/Communications | A marker that shows where an optical fibre cable runs underground. |
| `POFP` | Utilities/Communications | A pit on an optical fibre route, shot at the middle of its lid. |
| `POHM` | Utilities/Fuel | A sign or post that marks the route of a high-pressure oil pipeline. |
| `POST` | Fences and Walls/Gates and Posts | A single fence post or roadside guide post, shot at its centre. |
| `PPCN` | Survey Control and Annotation/Control Marks | The ground point at the centre of an aerial photograph, used to position and orient the image. |
| `PPET` | Utilities/Fuel | A petrol pump at a service station, shot at the middle of its base. |
| `PPLP` | Utilities/Electricity | A pole that carries power lines and also a street light. |
| `PPMK` | Boundaries and Cadastre/Boundary Marks | A permanent mark set in the ground to last, which later surveys can rely on to find a position. |
| `PPOL` | Utilities/Unknown Services | A pole that has not been identified as power, telephone or lighting. |
| `PPPL` | Utilities/Electricity | A pole that carries power lines and nothing else. |
| `PPSM` | Survey Control and Annotation/Control Marks | A permanent survey mark with coordinates, which the survey can be tied to. |
| `PPTR` | Utilities/Electricity | A pole that carries power lines and has a transformer mounted on it. |
| `PRAL` | Rail and Transit/Track | A level taken on the top of the head of a running rail. |
| `PRAL*` | Rail and Transit/Track | Keeps rail head levels out of the ground surface, so the track does not distort the terrain model. |
| `PRAV` | Utilities/Recycled Water | An air valve on a recycled water main, which lets trapped air out at a high point. |
| `PRBX` | Rail and Transit/Track | A box beside the track that houses railway control equipment. |
| `PRCC` | Rail and Transit/Track | A trough beside the track that carries signalling cables, taken as a single point. |
| `PRET` | Utilities/Recycled Water | An earth terminal on a recycled water main, the point where the main is bonded to earth. |
| `PRHY` | Utilities/Recycled Water | A hydrant on a recycled water main. |
| `PRMB` | Boundaries and Cadastre/Boundary Marks | A reference mark set in a concrete block, used to re-establish a survey mark if it is lost. |
| `PRMB*` | Boundaries and Cadastre/Boundary Marks | Keeps concrete reference blocks out of the ground surface, since they fix a position and not the ground. |
| `PRMM` | Utilities/Recycled Water | A marker that shows where a recycled water main runs. |
| `PRMR` | Utilities/Recycled Water | A meter on a recycled water supply. |
| `PRSG` | Rail and Transit/Overhead and Signals | A railway signal on its mast or post beside the track. |
| `PRSV` | Utilities/Recycled Water | A stop valve that shuts off a length of recycled water main. |
| `PRTM` | Rail and Transit/Track | A survey monument set beside the track as a lasting reference for its alignment. |
| `PRTP` | Utilities/Recycled Water | A tap on a recycled water supply, such as an outlet or a standpipe. |
| `PRTR` | Boundaries and Cadastre/Boundary Marks | A tree used as a witness to a survey mark or a boundary corner, shown as a plain point. |
| `PSAP` | Utilities/Electricity | The ground anchor of the stay wire that braces a pole. |
| `PSCL` | Street Furniture and Signs/Traffic Control | The cabinet that houses the controller of a set of traffic signals. |
| `PSDO` | Kerbs and Drainage/Pits and Structures | The invert level where a subsoil drain discharges at its outlet. |
| `PSDO*` | Kerbs and Drainage/Pits and Structures | Keeps subsoil drain outlet levels out of the ground surface, since an invert is not the ground. |
| `PSDR` | Utilities/Traffic Systems | A detector for a set of traffic signals, such as a vehicle loop in the road. |
| `PSDS` | Street Furniture and Signs/Signs | A post that carries a sign face on each of its two sides. |
| `PSFP` | Kerbs and Drainage/Pits and Structures | The flushing point of a subsoil drain, a riser through which the pipe can be cleared. |
| `PSFP*` | Kerbs and Drainage/Pits and Structures | Keeps subsoil flushing points out of the ground surface. |
| `PSGL` | Street Furniture and Signs/Traffic Control | A traffic signal post with its lanterns, at an intersection or a crossing. |
| `PSHT` | Terrain and Breaklines/Ground Points | A spot height: a single level taken at a point and kept visible on the plan. |
| `PSIN` | Street Furniture and Signs/Signs | A post that carries a single sign face. |
| `PSJX` | Utilities/Traffic Systems | A junction box in the ground where traffic signal cables are joined. |
| `PSKI` | Boundaries and Cadastre/Boundary Marks | A spike or nail driven into a hard surface and used as a survey mark. |
| `PSLH` | Utilities/Sewer | A lamphole: a small opening in a sewer for looking along the pipe with a lamp. |
| `PSMH` | Utilities/Sewer | The cover of a sewer maintenance hole, taken at the centre of the lid. |
| `PSMM` | Utilities/Sewer | A marker that shows where a sewer main runs. |
| `PSPL` | Utilities/Electricity | A short pole that carries the stay wire bracing another pole. |
| `PSPS` | Utilities/Sewer | A pumping station on a sewer, taken at the lid of the wet well. |
| `PSS*` | Survey Control and Annotation/Control Marks | Keeps every station mark out of the ground surface: control marks fix positions and are not terrain. |
| `PSSA` | Survey Control and Annotation/Control Marks | A survey mark of unspecified type, used when the kind of mark is not given. |
| `PSSB` | Survey Control and Annotation/Control Marks | The broad arrow cut in stone or a kerb to mark a bench level. |
| `PSSC` | Survey Control and Annotation/Control Marks | A peg driven in as a station for the job. |
| `PSSD` | Survey Control and Annotation/Control Marks | A station marked by a plug, a drill hole, an iron pipe or a reference block. |
| `PSSE` | Survey Control and Annotation/Control Marks | A station marked by a bolt, a nail, a spike or a similar small metal fixing. |
| `PSSF` | Survey Control and Annotation/Control Marks | A trigonometric station, a primary control mark of the geodetic network. |
| `PSSG` | Survey Control and Annotation/Control Marks | A mark of the permanent, state or project control networks, used as a survey station. |
| `PSSM` | Survey Control and Annotation/Control Marks | A mark of the state geodetic network, with published coordinates and heights. |
| `PSSV` | Utilities/Sewer | A stop valve that shuts off flow in a sewer line, taken at its cover. |
| `PSTA` | Boundaries and Cadastre/Boundary Marks | A star picket, a steel picket with a star-shaped section, driven in as a survey mark. |
| `PSTA*` | Boundaries and Cadastre/Boundary Marks | Keeps star pickets out of the ground surface, since they mark a position and not the ground. |
| `PSTR` | Rail and Transit/Overhead and Signals | A steel stanchion beside the track that carries the overhead wires or a signal. |
| `PSTS` | Survey Control and Annotation/Investigation Points | A seismic test shot: the point where a charge was fired for a seismic survey. |
| `PSVP` | Utilities/Sewer | A vent pipe that lets air into a sewer and gas out of it. |
| `PSWA` | Terrain and Breaklines/Water Edges | A swamp or marsh: a patch of wet, reedy ground marked by a point. |
| `PT01` | Vegetation and Landscape/Trees | A tree whose trunk is about 100 mm across, taken at the trunk. |
| `PT01*` | Vegetation and Landscape/Trees | Keeps 100 mm tree trunks out of the ground surface, since a trunk is not the ground. |
| `PT02` | Vegetation and Landscape/Trees | A tree whose trunk is about 200 mm across, taken at the trunk. |
| `PT02*` | Vegetation and Landscape/Trees | Keeps 200 mm tree trunks out of the ground surface, since a trunk is not the ground. |
| `PT03` | Vegetation and Landscape/Trees | A tree whose trunk is about 300 mm across, taken at the trunk. |
| `PT03*` | Vegetation and Landscape/Trees | Keeps 300 mm tree trunks out of the ground surface, since a trunk is not the ground. |
| `PT04` | Vegetation and Landscape/Trees | Tree trunk about 400 mm across, shot at the middle of the trunk. |
| `PT04*` | Vegetation and Landscape/Trees | Keeps trunk points of 400 mm trees out of the ground surface. |
| `PT05` | Vegetation and Landscape/Trees | Tree trunk about 500 mm across, shot at the middle of the trunk. |
| `PT05*` | Vegetation and Landscape/Trees | Keeps trunk points of 500 mm trees out of the ground surface. |
| `PT06` | Vegetation and Landscape/Trees | Tree trunk about 600 mm across, shot at the middle of the trunk. |
| `PT06*` | Vegetation and Landscape/Trees | Keeps trunk points of 600 mm trees out of the ground surface. |
| `PT07` | Vegetation and Landscape/Trees | Tree trunk about 700 mm across, shot at the middle of the trunk. |
| `PT07*` | Vegetation and Landscape/Trees | Keeps trunk points of 700 mm trees out of the ground surface. |
| `PT08` | Vegetation and Landscape/Trees | Tree trunk about 800 mm across, shot at the middle of the trunk. |
| `PT08*` | Vegetation and Landscape/Trees | Keeps trunk points of 800 mm trees out of the ground surface. |
| `PT09` | Vegetation and Landscape/Trees | Tree trunk about 900 mm across, shot at the middle of the trunk. |
| `PT09*` | Vegetation and Landscape/Trees | Keeps trunk points of 900 mm trees out of the ground surface. |
| `PT10` | Vegetation and Landscape/Trees | Tree trunk about 1000 mm across, shot at the middle of the trunk. |
| `PT10*` | Vegetation and Landscape/Trees | Keeps trunk points of 1000 mm trees out of the ground surface. |
| `PT12` | Vegetation and Landscape/Trees | Tree trunk about 1200 mm across, shot at the middle of the trunk. |
| `PT12*` | Vegetation and Landscape/Trees | Keeps trunk points of 1200 mm trees out of the ground surface. |
| `PT15` | Vegetation and Landscape/Trees | Tree trunk about 1500 mm across, shot at the middle of the trunk. |
| `PT15*` | Vegetation and Landscape/Trees | Keeps trunk points of 1500 mm trees out of the ground surface. |
| `PT3P` | Utilities/Communications | Concrete telephone pit with three lids in a row, where cable is joined and drawn in. |
| `PTBX` | Utilities/Communications | Public telephone box or booth standing on the footpath. |
| `PTCM` | Utilities/Communications | Marker that shows the route of a buried telephone cable. |
| `PTDP` | Utilities/Communications | Street cabinet from which telephone cable is distributed to nearby customers. |
| `PTJP` | Utilities/Communications | Post standing above ground at a joint in a telephone cable. |
| `PTMP` | Utilities/Communications | Large main pit, about 1.1 m square, at a major telephone cable junction. |
| `PTMX` | Roads and Pavements/Markings | Raised domed marker fixed to the road, called a silent cop, that guides traffic. |
| `PTMX*` | Roads and Pavements/Markings | Keeps silent-cop road markers out of the ground surface. |
| `PTPL` | Utilities/Communications | Pole that carries telephone cable overhead. |
| `PTRE` | Vegetation and Landscape/Trees | Tree trunk whose diameter was not measured, shot at the middle of the trunk. |
| `PTRE*` | Vegetation and Landscape/Trees | Keeps trunk points of unmeasured trees out of the ground surface. |
| `PTSP` | Utilities/Communications | Concrete telephone pit with a single lid. |
| `PTSS` | Survey Control and Annotation/Control Marks | Trigonometric station built as a concrete pillar. |
| `PTSS*` | Survey Control and Annotation/Control Marks | Keeps concrete trig pillar points out of the ground surface. |
| `PTTP` | Utilities/Communications | Concrete telephone pit with two lids side by side. |
| `PUSR` | Utilities/Unknown Services | Surface sign of a service that has not been identified, such as an unmarked cover or stub. |
| `PUTP` | Utilities/Unknown Services | Buried tank whose contents are not known, shot at its lid or filler. |
| `PVCP` | Survey Control and Annotation/Control Marks | Point whose height alone is held as control, such as a target for aerial photo mapping. |
| `PVID` | Miscellaneous/General | Point where two surveyed lines meet, usually worked out by calculation. |
| `PWAV` | Utilities/Water | Valve at a high point of a water main that lets trapped air escape. |
| `PWCP` | Utilities/Water | Fitting on a water main where pressure or electrical readings are taken. |
| `PWEP` | Utilities/Water | Pump that lifts or boosts water, shot as a point. |
| `PWET` | Utilities/Water | Terminal that connects a water main to an electrical earth. |
| `PWFB` | Utilities/Fire Service | Hydrant on a fire service main, for the fire brigade's hoses. |
| `PWHY` | Utilities/Water | Hydrant on a drinking water main, used for flushing and for fire fighting. |
| `PWLP` | Terrain and Breaklines/Water Edges | Point where the level of a water surface was read. |
| `PWML` | Buildings and Structures/Structures | Windmill, usually one that pumps water, shot at the middle of its tower. |
| `PWMM` | Utilities/Water | Post or plate that shows the route of a water main. |
| `PWMR` | Utilities/Water | Meter that records the water passing through a connection. |
| `PWSR` | Utilities/Water | Valve at a low point of a main that empties it for cleaning or repair. |
| `PWSV` | Utilities/Water | Valve that shuts off the flow in a water main. |
| `PWTP` | Utilities/Water | Tap on a water supply, such as a standpipe or a garden tap. |
| `QQ*` | Miscellaneous/General | Line of check shots used to test the survey or the model, kept out of the ground surface. |
| `R*` | Design and Setting Out/Road Design Strings | Road design string starting with R that no more specific code claims. |
| `RA*` | Rail and Transit/Track | Top of a running rail, shot along the middle of the rail head. |
| `RC*` | Roads and Pavements/Carriageway | Centre of the road, shot along the middle of the carriageway. |
| `RE*` | Utilities/Water | Outline of a water storage reservoir, traced round its walls. |
| `RF*` | Rail and Transit/Track | Edge of the railway formation, the shaped earth platform that carries the ballast. |
| `RM*` | Utilities/Recycled Water | Main that carries recycled water, shot along its centre. |
| `RP*` | Rail and Transit/Platforms | Top edge of a railway platform, on the side nearest the track. |
| `RR*` | Rail and Transit/Platforms | Edge of a ramp that rises to a railway platform. |
| `RS*` | Rail and Transit/Overhead and Signals | Trough beside the track that carries signalling cable. |
| `RT*` | Rail and Transit/Track | Centre of a railway track, midway between its two rails. |
| `RW*` | Fences and Walls/Walls | Wall built to hold back earth, shot along its base. |
| `S1*` | Street Furniture and Signs/Signs | Stretch of road signed with a speed limit of 100 km/h; the string runs along the zone. |
| `S2*` | Street Furniture and Signs/Signs | Stretch of road signed with a speed limit of 110 km/h; the string runs along the zone. |
| `S4*` | Street Furniture and Signs/Signs | Stretch of road signed with a speed limit of 40 km/h; the string runs along the zone. |
| `S5*` | Street Furniture and Signs/Signs | Stretch of road signed with a speed limit of 50 km/h; the string runs along the zone. |
| `S6*` | Street Furniture and Signs/Signs | Stretch of road signed with a speed limit of 60 km/h; the string runs along the zone. |
| `S7*` | Street Furniture and Signs/Signs | Stretch of road signed with a speed limit of 70 km/h; the string runs along the zone. |
| `S8*` | Street Furniture and Signs/Signs | Stretch of road signed with a speed limit of 80 km/h; the string runs along the zone. |
| `S9*` | Street Furniture and Signs/Signs | Stretch of road signed with a speed limit of 90 km/h; the string runs along the zone. |
| `SA*` | Design and Setting Out/Barrier and Fence Design | Design line for the rail of a pedestrian fence along a footpath or a bridge. |
| `SB*` | Roads and Pavements/Markings | A painted double line with a broken separation line on one side and an unbroken barrier line on the other, the barrier on the right. |
| `SC*` | Utilities/Electricity | Stay wire that braces a pole or mast, drawn from the pole to its ground anchor. |
| `SD*` | Utilities/Traffic Systems | Loop cut into the road surface that detects vehicles for a traffic signal, drawn along the loop. |
| `SE*` | Street Furniture and Signs/Amenities | Plan outline of a fixed seat or bench. |
| `SF*` | Fences and Walls/Fences | Fence put up to keep people away from a drop, a pool or another danger. |
| `SG*` | Roads and Pavements/Carriageway | Grid of bars across a road or gateway that stops stock but lets vehicles through. |
| `SH*` | Design and Setting Out/Drainage Design | Design line along the edge of a subsoil drain. |
| `SI*` | Street Furniture and Signs/Signs | Plan outline of a large sign face, such as a gantry or hoarding. |
| `SL*` | Survey Control and Annotation/Investigation Points | Line followed by a seismic survey, along which shots and receivers are set. |
| `SM*` | Utilities/Sewer | Gravity sewer main, shot as a line along its route. |
| `SO*` | Buildings and Structures/Steps and Access | Outside stairs of a building, shot along the nosing of the treads. |
| `SP*` | Utilities/Sewer | Outline of a sewage pond or lagoon. |
| `SQ*` | Utilities/Sewer | Outline of a station that pumps sewage or water out of a low point. |
| `SR*` | Utilities/Sewer | Pressure sewer, the rising main that carries pumped flow uphill, shot as a line along its route. |
| `ST*` | Utilities/Sewer | Outline of a septic tank. |
| `SX*` | Street Furniture and Signs/Signs | Plan outline of a sign carried on an arm that reaches out over the road. |
| `SY*` | Utilities/Sewer | Small branch that joins a property to the sewer main, shot as a line along its route. |
| `SZ*` | Utilities/Sewer | Gravity sewer traced from imagery or plans and not located on the ground. |
| `TA*` | Street Furniture and Signs/Amenities | Plan outline of a picnic table. |
| `TC*` | Terrain and Breaklines/Breaklines | Top edge of a bank or batter, with the ticks pointing downhill. |
| `TD*` | Utilities/Communications | Conduit that carries telephone cable, shot as a line along its route. |
| `TG*` | Street Furniture and Signs/Posts and Barriers | Line along the top of a guard rail. |
| `TL*` | Roads and Pavements/Markings | Raised bars painted across the edge of a lane that rattle a tyre and warn a driver who drifts. |
| `TM*` | Kerbs and Drainage/Kerbs | Top edge of a raised median or traffic island. |
| `TN*` | Utilities/Communications | Buried telephone line, shot as a line along its route. |
| `TO*` | Street Furniture and Signs/Traffic Control | Plan outline of a signal pole together with the arm that reaches out over the road. |
| `TR*` | Vegetation and Landscape/Trees | Outline of the foliage of a tree or a stand of trees, drawn along its drip line. |
| `TS*` | Utilities/Communications | Outline of a telephone sump, checked in the third model zone (road side). |
| `TW*` | Buildings and Structures/Buildings | Line along the top of a wall. |
| `TX*` | Utilities/Communications | Plan outline of a public telephone box. |
| `TY*` | Utilities/Communications | Small cable that joins a property to the telephone network, shot as a line along its route. |
| `TZ*` | Utilities/Communications | Telephone line traced from imagery or plans and not located on the ground. |
| `U1*` | Kerbs and Drainage/Pipes and Culverts | Stormwater pipe 225 mm across, shot as a line along its route. |
| `U2*` | Kerbs and Drainage/Pipes and Culverts | Stormwater pipe 300 mm across, shot as a line along its route. |
| `U3*` | Kerbs and Drainage/Pipes and Culverts | Stormwater pipe 375 mm across, shot as a line along its route. |
| `U4*` | Kerbs and Drainage/Pipes and Culverts | Stormwater pipe 450 mm across, shot as a line along its route. |
| `U5*` | Kerbs and Drainage/Pipes and Culverts | Stormwater pipe 525 mm across, shot as a line along its route. |
| `U6*` | Kerbs and Drainage/Pipes and Culverts | Stormwater pipe 600 mm across, shot as a line along its route. |
| `U7*` | Kerbs and Drainage/Pipes and Culverts | Stormwater pipe 750 mm across, shot as a line along its route. |
| `U9*` | Kerbs and Drainage/Pipes and Culverts | Stormwater pipe 900 mm across, shot as a line along its route. |
| `UB*` | Kerbs and Drainage/Pipes and Culverts | Rectangular box culvert whose height was not measured. |
| `UC*` | Buildings and Structures/Structures | Line on the ground under a bridge that marks the limit of its clearance overhead. |
| `UE*` | Utilities/Electricity | Main overhead power line that carries bulk supply between substations. |
| `UL*` | Utilities/Electricity | Overhead power line of the local network, below the major transmission lines. |
| `UO*` | Utilities/Water | Water pipeline that runs above ground on supports. |
| `UP*` | Utilities/Unknown Services | Pipeline whose service has not been identified. |
| `UR*` | Roads and Pavements/Carriageway | Edge of the formed surface of an unsealed road. |
| `US*` | Utilities/Unknown Services | Plan outline of a box where service lines are joined, whose service is not known. |
| `UT*` | Buildings and Structures/Structures | Plan outline of a tank buried in the ground. |
| `UU*` | Kerbs and Drainage/Pipes and Culverts | Stormwater pipe whose diameter was not measured. |
| `UX*` | Kerbs and Drainage/Pipes and Culverts | Stormwater pipe of any diameter, for a pipe whose size is given in its attributes. |
| `UZ*` | Kerbs and Drainage/Pipes and Culverts | Stormwater pipe traced from imagery or plans and not located on the ground. |
| `V1*` | Kerbs and Drainage/Pipes and Culverts | Stormwater pipe 1050 mm across, shot as a line along its route. |
| `V2*` | Kerbs and Drainage/Pipes and Culverts | Stormwater pipe 1200 mm across, shot as a line along its route. |
| `V3*` | Kerbs and Drainage/Pipes and Culverts | Stormwater pipe 1350 mm across, shot as a line along its route. |
| `V5*` | Kerbs and Drainage/Pipes and Culverts | Stormwater pipe 1500 mm across, shot as a line along its route. |
| `V6*` | Kerbs and Drainage/Pipes and Culverts | Stormwater pipe 1650 mm across, shot as a line along its route. |
| `V8*` | Kerbs and Drainage/Pipes and Culverts | Stormwater pipe 1800 mm across, shot as a line along its route. |
| `VC*` | Terrain and Breaklines/Rock and Cliff | Top edge of a cliff or escarpment, with the ticks pointing over the drop. |
| `VE*` | Buildings and Structures/Buildings | Edge of a concrete slab laid flat on the ground. |
| `VL*` | Terrain and Breaklines/Breaklines | Scarp left by a landslide or by water eating into a bank. |
| `VT*` | Roads and Pavements/Carriageway | Edge of a track that vehicles use, formed by wheels and not built. |
| `WC*` | Kerbs and Drainage/Channels and Waterways | Centre of a natural watercourse. |
| `WE*` | Terrain and Breaklines/Water Edges | Edge of water, with a level taken on it. |
| `WI*` | Utilities/Water | Pipe that carries water for irrigation, shot as a line along its route. |
| `WL*` | Terrain and Breaklines/Water Edges | Shoreline of a lake. |
| `WM*` | Utilities/Water | Water main, shot as a line along its route. |
| `WR*` | Terrain and Breaklines/Water Edges | Bank edge of a river or creek. |
| `WW*` | Buildings and Structures/Structures | Wall that flares out from the end of a bridge abutment to hold back the embankment. |
| `WY*` | Utilities/Water | Small pipe that joins a property to the water main, shot as a line along its route. |
| `WZ*` | Utilities/Water | Water main traced from imagery or plans and not located on the ground. |
| `X0*` | Terrain and Breaklines/Contours | Contour at the regular interval between index contours. |
| `XB*` | Survey Control and Annotation/Investigation Points | Bottom edge of a trench dug to expose buried services. |
| `XJ*` | Buildings and Structures/Structures | Line of a joint that lets a bridge deck expand. |
| `XP*` | Buildings and Structures/Structures | Edge of a steel plate that covers an expansion joint. |
| `XQ*` | Terrain and Breaklines/Contours | Contour round a hollow at the regular interval, with the ticks pointing downhill. |
| `XT*` | Survey Control and Annotation/Investigation Points | Top edge of a trench dug to expose buried services. |
| `Y0*` | Terrain and Breaklines/Contours | Extra contour drawn at each whole metre between the regular ones. |
| `YG*` | Utilities/Gas | Polyethylene gas main, shot as a line along its route. |
| `YQ*` | Terrain and Breaklines/Contours | Extra contour drawn at each tenth of a metre, for flat ground. |
| `Z*` | Terrain and Breaklines/Contours | Index contour, the heavier line drawn at every fifth contour. |
| `Z0*` | Terrain and Breaklines/Contours | Index contour at the regular index interval. |
| `ZG*` | Utilities/Gas | Gas main traced from imagery or plans and not located on the ground. |
| `ZQ*` | Terrain and Breaklines/Contours | Index contour round a hollow, with the ticks pointing downhill. |

## Decisions, and what was rejected

- **The keys are the NSW customisation's.** The owner ruled that Katana Standard has the same survey codes as the NSW one, with different symbols and linestyles, so that a field file works with either. The library had its own three-letter scheme (245 codes, `UWM` for a water main); it is gone, with its documents, tests and pictures, since the library is new and nothing but those depended on it. Rejected: keeping both vocabularies, which would double the codes a surveyor learns and leave two answers to one field file.
- **The structure follows the keys, the look does not.** A key makes the same kinds of entity in both libraries because the rule kinds, the draw mode, the text and pipe rules and the surface and hide flags are the other's; every visible choice is Katana's. Rejected: copying a look that a person could copy back, and a looser match (the same key but another draw mode), under which the same field file would make a point in one library and a line in the other.
- **One clean rule set per key.** The other library repeats and shadows rules, names pen colours that resolve to nothing and draws a symbol through a linestyle slot. Reproducing that would give `CODE CHECK` hundreds of warnings on a new library; each difference is a counted exception that `parity.py` holds.
- **A service code asks for its service's attributes.** Where the other library has no attributes rule for a service key, one is added last (grammar rule 9), so that `UTILITY DRAW` reads a coded string as a typed service. Rejected: strict equality of the rule kinds, which would drop the typing the utility tools read and leave every coded service "unknown".
- **Paper units for every definition, no `twoPoint`.** Measured in the engine, a stretched definition scales its marks with the line's length, so an arrowhead grows with the leader. Rejected: `world` units, whose marks would change size with the zoom and be unreadable at plan scale.
- **No left or right suffix.** The rule engine matches a key exactly or by prefix and has no other form, so a handed pair is two keys (`CV*` and `CY*`, `DA*` and `DC*`, `LH*` and `LR*`, `HE*` and `HR*`) and otherwise handedness is by direction of travel: ticks, barbs and chevrons fall to the LEFT.
- **Every key is matched as written.** A typo is then reported as a code no rule answers, unless some prefix key fits it by chance; the invented field file's `PWSW` is none of the 632 keys' prefixes.
- **The library says nothing of linework spellings or automation.** Loading it onto a session must not reset a colleague's control words, so `linework` and `automation` are absent from the file, not written at their defaults.
- **A dot is a short dash.** Katana draws a `dot` stroke as one pen-width point whatever its radius: a speck a third of the line's weight on a plot and one pixel on screen. Dash-dot lines and the centres of symbols are therefore drawn with 0.6 mm dashes and a very small ring, measured in the renderer, and the definitions say so.
- **Layers are what one switches on and off together.** One per family, three levels, never one per code: 161 layers for 632 codes.
- **Edges that are one hard line stay plain.** 143 of the 360 line codes use Katana's plain continuous line, since a solid line is the right drawing for a hard edge or a design string; where two lines of one class are NOT the same thing they have a pattern each.
- **Stormwater pits and pipes are typed for the utility tools.** The pits and structures and the pipes and culverts of class K set `utility.type` to `stormwater`, though they are catalogued as drainage; the kerbs, channels and creeks do not. Rejected: leaving them untyped, which makes `UTILITY DRAW` read a coded stormwater pipe as an unknown service.
- **A size the key states is applied.** A pipe `U3*` is 375 mm, an invert `PI04` is in a 450 mm pipe and a box culvert `D1*` is 1200 mm high, so each applies `Diameter (mm)` or `Height (mm)`; the surveyor is not asked what the code says.

## How it is made and held

- **The data** is plain Python in `tools/katana_standard/`: `colours.py`, `linestyles.py`, `symbols.py` and `codes.py`, each importing nothing of the outside and each with its own `--check`; the check of `colours.py` also reads `kNearWhiteChannel` from `include/katana/cad/plot.hpp`, so the white rule it prints by is the plot's. `codes.py --check` also holds that every definition is named by a code and every colour of the palette is used. `make_katana_standard.py` is the only thing that knows the file format. `--json` writes the file, `--docs` this document, `--check` exits 1 when either committed output differs from what the data makes, `--images` draws the sheets with `render_images.py`. Run twice, it gives the same bytes.
- **The bytes are the writer's.** The generator writes the canonical form `customisationToJson` writes (`docs/customisation.md`, "What the writer writes"): a file loaded by `CUSTOMISE <file>` and written again by `CUSTOMISE EXPORT <out> CODES LINESTYLES SYMBOLS` comes back byte for byte, except for the one line the session adds, its record of what it was loaded from (`make_katana_standard.py --check --round-trip <katana_cli>`). The tests below check the same on the value itself.
- **A second opinion.** `tools/katana_standard/census.py` counts the committed file with the standard `json` module and no Katana code, and `tools/customisation_census.py` counts the generic figures. The tests pin the figures those scripts gave.
- **The comparison with the other library** is `tools/katana_standard/parity.py` (keys and structure, with `parity_exceptions.json` and its own tests in `test_parity.py`) and `originality.py` (what else is shared). `katana_standard_parity`, `katana_standard_parity_script` and `katana_standard_originality` run them against the two tracked files; `KatanaStandard.EveryKeyOfTheNswBuiltInIsHereAndNoOtherKey` and `KatanaStandard.FortyCodesAcrossAllClassesResolveToTheKindOfEntityTheNswBuiltInMakes` do the same in C++, from the two files alone.
- **Tests.** `KatanaStandard.*` (`tests/cad/customisation/test_katana_standard.cpp`) reads the committed file with the strict reader and holds the counts, the grammar, every name resolved, every definition used and catalogued, the weights, the utility words, the geometry bounds of the contract and the byte-for-byte writer round trip. `cli.katana_standard_imports_a_field_file_and_reports_the_mistyped_code` imports the invented field file; `cli.katana_standard_strings_control_words_into_a_curve_a_rectangle_and_a_closed_line` strings control words, which no field-file reader carries, into a curve, a rectangle and a closed line. A counts-only test cannot see a swapped colour, weight or linestyle, so `katana_standard_current` runs `make_katana_standard.py --check` (the committed file and this document are what the data makes) and `katana_standard_colours`, `_linestyles`, `_symbols` and `_codes` run each data module's own check; they exist where Python does.
- **Images.** `docs/images/katana-standard-symbols.png`, `-linestyles.png`, `-plan.png` and `-showcase.png` are drawn by Katana itself from the committed file, headlessly (`tools/katana_standard/render_images.py`; `docs/headless.md`): the symbols are the Symbol Library's own icons, tinted with the colour of the code that uses each and laid out on one sheet; the linestyles are a 300 dpi plot; the plan is the plan view at twice the pixels, each feature named beside it by the script; the showcase is the plan view at the plot scale 1:500, in a few screenshots laid side by side. `-palette.png` is laid out from the palette's values by `palette_preview.py`, since a palette is a table and not a drawing. The images are not byte-reproducible, since fonts differ between machines, so `--check` does not compare them. Each is under 500 KB.

## Not done

- **Yellow and cream print light.** AS 5488's gas and sewer colours are under 3:1 on white paper, and the plot has no rule that darkens a light colour: it turns only white and near-white pens black (`cad::paperColour`). A rule that did, for any pen under 3:1, would change every plot, so it is the owner's decision; until then they print light, as the palette section says.
- **The earlier file names of an old project** belonged to the NSW customisation and are answered by it alone (`docs/customisation.md`, "Several built-ins"): opened under Katana Standard such a project lists them as not loaded, and `CUSTOMISE BUILTIN NSW` clears them. The warning does not name that verb for them, since what is missing is the old file names. Narrow: a project saved in the first days of the built-in.
- **Whether the licence reaches this library is the owner's to settle.** `LICENSING.md` lists what the project licence does not cover. The library was written by AI agents under the owner's direction, and its survey code keys are another library's identifiers, reused on the owner's decision for field-data compatibility; what either means for copyright is a question for the owner and a lawyer.
- **The meaning of some keys is a reading.** Several dozen keys have a label in the other library and no stated purpose (the unclassified road design strings, the hinges and bases of a cross-section, a few marks and lane lines). Their meanings are worded as generally as the label allows, and the unclassified design strings share one layer family each; a surveyor who knows what one is for should say so.
- **Katana does not yet apply** `weight`, `group`, `hide`, `surface`, text rules, pipe rules, or a symbol's rotation, offset and raise. The library carries them for the day it does.
- **A curve cannot come from a field file.** The readers Katana has strung points by the file's own strings and close one with opcode 20; control words (`BC`, `EC`, `ST`, `END`, `CL`, `RECT`) reach the linework only from a code kept in a point's property, so the curve and the rectangle are tested through `LINEWORK PROPERTY` and not through an import.
- **Four keys are spelled like a control word** (`BC*`, `CL*`, `EC*`, `ST*`), as in the other library. A control word is read only after the key, so a point coded `ST` is a string called ST, a septic tank. Nothing is lost, but a surveyor who types a control word in the code column gets a feature.
- **Undo leaves the parent layers.** Katana's layer table makes every ancestor of a layer path a layer of its own, and Undo removes the layer a command made and not its ancestors: `LAYER NEW x/y/z` then `UNDO` leaves `x` and `x/y`. The library's layers are three levels deep, so one undo of the street corner's import takes the points, lines and styles and the ten layers they sit on away and leaves 15 empty parent layers. It is the layer table's behaviour, not the library's. Not changed here.
- **39 linestyles are shared by two to 22 codes**, so the plot legend uses the linestyle's name for them and not the code's label.
- **Some symbols sit off the centre of their box**: Winged Drill Hole, Star Picket, Stay Pole, Petrol Pump, Building Corner and Water Level Mark, by 0.39 to 0.60 mm. The surveyed point is not the middle of their drawing, and each description says where the origin is.
