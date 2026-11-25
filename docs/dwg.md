# DWG - read and written through acadrust

`katana_dwg` (`crates/katana_dwg/src/`) reads and writes DWG drawings with
acadrust 0.6.8, a pure Rust library (MPL-2.0, its DWG code ported from
ACadSharp, MIT; `THIRD_PARTY_NOTICES.md`). Every front end sends a `.dwg` to
it: File > Import > Import Any File, File > Export > Export Drawing, the
`IMPORT` and `EXPORT` verbs of the window's command line, of `katana_cli` and
of `katana_mcp` (`katana_import`, and `katana_export` with its `release`). A
DWG comes in and goes out through the DXF module, the one mapping between
Katana's entities and a CAD drawing (`docs/dxf.md`): what this document does
not say about a drawing's entities, that one does.

## Why

The owner asked on 2026-11-25 for DWG to be read and written as the
open-source CAD program CADCraft does it, through acadrust. Until then a
`.dwg` went to GDAL: a GDAL built with the CAD driver (conda-forge's, in WSL)
read it as GIS features - the driver is read-only and reads R2000 alone, as
libopencad does - and MSYS2's GDAL, the Windows build's, has no such driver,
so on Windows a DWG was not read at all.

## What CADCraft does

Read from CADCraft's repository on 2026-11-25
(<https://github.com/storytold/cadcraft>, its docs/file-format-parity.md)
and the bridge it uses (<https://github.com/storytold/craft-libs>,
crates/dwg/src/lib.rs; CADCraft's Codeberg address answered 404 that day).
CADCraft never maps acadrust's document to its own model. Reading, its bridge
reads the DWG with acadrust in failsafe mode (`acadrust::DwgReader` with
`acadrust::DwgReadOptions::failsafe`, `read_with_stats`), refuses the file when the
record stream could not be read, counts the records it could not decode, and
writes the document out as DXF (`acadrust::DxfWriter`), which CADCraft's own DXF reader
then reads. Writing, CADCraft writes its own R2000 DXF, acadrust reads it
(`acadrust::DxfReader`), the document's version is set to the release asked for, and
`acadrust::DwgWriter::write_to_vec` writes it; with no release asked for it writes at
least R2004, and new drawings are saved as 2018. It writes R2000 to R2018,
reads R13 to R2018 (R13 and R14 untested), guards every acadrust call
against a panic, runs it in a child process for a crash it cannot catch, and
counts what the DWG writer dropped. It says what it loses through DWG:
whatever its DXF loses, plus what acadrust's DXF omits.

## The choice

**Chosen: DWG to acadrust's document to DXF text to `katana_dxf`, both ways.**
`katana_dwg::read_dwg` gives back a `katana_dxf::DxfImport`, the DXF
reader's own, so a DWG import is a DXF import - the same tally, the same
warnings, the same one undo step (`katana_dxf::import_command::import_command`); and
`katana_dwg::write_dwg` writes the model with `katana_dxf::write_dxf`, so a
DWG export writes what a DXF export writes, extended data and all.

*Rejected: mapping acadrust's document to Katana's entities directly, in
`katana_dwg`, with `katana_dxf`'s helpers for names, colours and text codes.*
Judged on what a drawing keeps:

- **Entity kinds, blocks and inserts, attributes, dimensions, 3D.** The DXF
  reader is 3 000 lines because a CAD drawing's entities are not plain: an
  insert expands its block, nested blocks too, with scale per axis, arrays,
  layer 0 and ByBlock colour taking the insert's; attributes become
  properties; a dimension is its picture block; an entity in a mirrored or
  tilted plane is brought into plan; heights follow Katana's "absent is not
  zero" rules. A direct mapping is all of that written a second time over
  acadrust's types - and a second way of doing something that has one is a
  defect, since the two come to differ. Through DXF, every rule that holds for
  a DXF holds for a DWG, and a fix to one is a fix to both.
- **Layers and their paths, colours, linetypes, heights.** The writer's
  extended data (`KATANA`: a layer's path, a colour no index is, heights at
  only some vertices) is DXF's; acadrust keeps it on the entity and in the
  layer record through DXF and DWG alike (its document's extended data of a
  table record, which its own tests carry through ASCII DXF, binary DXF and
  DWG, in its tests/issue132.rs), so a Katana drawing written and read back is the
  drawing it was. What R2000's DXF cannot say but a DWG from R2004 can - a
  true colour - the bridge puts back into the document before the DWG is
  written ("Writing").
- **Text and its encoding.** The one place the path costs anything: the DXF
  between the two must not lose a character. Read, it is written as R2007's
  DXF at the oldest, whose text acadrust writes as UTF-8 whatever the DWG's
  code page was; written, the declaration of a code page is left out of the
  DXF acadrust reads, so that it turns the writer's `\U+XXXX` escapes into the
  characters ("Writing"). Both are tested in every release.
- **Errors and warnings an agent reads.** One tally and one set of warnings,
  the DXF's, with acadrust's own account of the file said first. A direct
  mapping would have needed its own of each.
- **Cost on a large drawing** (an estimate: nothing here is measured yet).
  The DXF between the two is held in memory, about 1.2 kB an entity (the
  owner's 27 886-entity drawing is 34.1 MB of DXF, `docs/dxf.md`), and is
  written and parsed once more than a direct mapping would. The DXF module
  wrote and read that drawing in 0.26-0.46 s in-process (the C++ build's
  figures, `docs/dxf.md`, "Measured"); acadrust's DXF writer and reader are of
  the same order of work, so the path costs perhaps a second or two on such a
  drawing beside acadrust's DWG decoding, which either path pays. CADCraft
  reports a user's 10 MB DWG taking two minutes before its limits; Katana has
  not measured acadrust (Not done).

*Rejected: GDAL's CAD driver*, which a GDAL build may or may not have: R2000
only, read-only, and GIS features - an arc a curve string, text dropped - the
loss `docs/dxf.md` ("Why") records for GDAL's DXF driver.

## Where it sits

```
core < math < geometry < entity < commands
                                     |
                                    dxf
                                     |
                                    dwg        (acadrust; no GDAL)
                                     |
                           app (katana_cli, katana_mcp), window (katana)
```

`dwg` may see core, math, geometry, entity, commands and dxf
(`tools/check_layering.cmake`); app and the window see it. acadrust is used
in one file, `crates/katana_dwg/src/bridge.rs`, private to the crate: no type
of acadrust's is in its public interface. acadrust is built without its
default `parallel` feature, so its work runs on the calling thread (the
window's job worker) and Rayon's pool is not a second pool beside
`katana_core::task_pool`.

| File | What |
| --- | --- |
| `crates/katana_dwg/src/reader.rs` | `read_dwg(bytes, options)`, `read_dwg_file(path, options)` -> `katana_dxf::DxfImport`, its version and release the DWG's; `is_dwg_path`. |
| `crates/katana_dwg/src/writer.rs` | `write_dwg(model, options, release)`, `write_dwg_file(model, path, options, release)` -> `DwgExport`: the bytes (or the file), the DXF writer's counts, warnings. |
| `crates/katana_dwg/src/release.rs` | `DwgRelease`: the releases written, their names and version strings, `DwgRelease::parse` of the word a line gives. |
| `crates/katana_dwg/src/bridge.rs` | Every acadrust call, each with the file and line of acadrust's it relies on. |
| `crates/katana_app/src/dwg_verbs.rs` | `IMPORT` and `EXPORT`'s steps for a `.dwg`, the DXF's own (`crates/katana_app/src/dxf_verbs.rs`: `apply_drawing_import`, `export_drawing`, `export_drawing_scoped`) called with the DWG's words. |

## Releases

**Read:** R12 (AC1009, which is R11's format too), R13 (AC1012), R14
(AC1014), R2000 (AC1015), R2004 (AC1018), R2007 (AC1021), R2010 (AC1024),
R2013 (AC1027) and R2018 (AC1032) - every one acadrust reads. A DWG says its
release in its first six bytes; one older (R10, AC1006) or one this reader
does not know (AC1040) is refused by name, before acadrust is given it.

**Written:** R2000, R2004, R2007, R2010, R2013 and R2018 - the six CADCraft's
bridge writes. **R2018 by default:** the newest release, the one AutoCAD has
written since 2018, acadrust's own default and CADCraft's. An older program
needs an older release: R2013 for AutoCAD 2013 to 2017, and so on back to
R2000. R12, R13 and R14 are read and not written, refused by name: CADCraft
writes none of them, and Katana has no test of them as written.

**Choosing one:** `release=` on the `EXPORT` line, as its name (`R2013`), its
year (`2013`) or its version string (`AC1027`), in any letter case; the
Export Drawing dialog's DWG release (`vectorExportDwgRelease`, newest first,
enabled for a `.dwg` alone), which writes that word; `katana_export`'s
`release`. `release=` for a file that is not a DWG is refused by name, never
ignored.

## Reading

1. **The first bytes.** Empty, shorter than a version string, a DXF under the
   wrong extension (it begins `0` / `SECTION`, or a `999` comment), a binary
   DXF, anything that does not begin with a version string, or a release not
   read: each refused, saying which ("When the file is not one").
2. **acadrust reads the DWG** in failsafe mode
   (`acadrust::DwgReadOptions::failsafe`). A file whose header or section map
   cannot be read gives back nothing, which is the one failure past the first
   bytes. A record it cannot decode is left out and counted.
3. **What would come in wrong is taken out.** acadrust's DXF writer writes a
   number that is not one as 0.0, where the DXF reader would have refused it,
   and the entity would come in at the origin: an entity of a kind the DXF
   reader reads that holds one (a coordinate, a radius, an angle, a scale) is
   left out and counted. A constraint group acadrust could not decode, which
   its DXF writer refuses and which draws nothing, is left out of the
   document the DXF is written from.
4. **The DXF.** R2007's at the oldest, its text UTF-8, which the DXF reader
   reads as it is; acadrust has decoded the DWG's strings by its code page
   (before R2007) or as UTF-16 (from R2007).
5. **The DXF reader** reads it with the import's options (the curve
   tolerance, the placement's shift, the source name), exactly as it reads a
   `.dxf`. The import's version and release are the DWG's.

What becomes what is `docs/dxf.md`'s "Reading" table, entity for entity:
inserts expanded, attributes as properties, a dimension as its picture, paper
space counted and not imported, hatches, solids, 3D faces, meshes and the
rest in the tally as read and not imported. The tally names the kinds by
their DXF names (`LINE`, `INSERT`), the names a DWG's entities have in DXF.

The warnings, in order: acadrust's - how many of the file's records it could
not decode, by kind; up to eight of its notes on the file as a whole (a
section it could not read), the rest counted; how many entities were left
out for a number that is not one - then the DXF reader's. A DWG of which
nothing is imported says so ("nothing was imported: the DWG holds no entity
Katana reads"): not a failure, since a drawing may be empty, but said, so that
an import of nothing is not taken for one that failed quietly.

### When the file is not one

| What | Code | Says |
| --- | --- | --- |
| an empty file | ParseFailure | not a DWG file: it is empty (0 bytes) |
| fewer than 6 bytes | ParseFailure | ... it is N bytes long, shorter than the 6 of a DWG's version string |
| a DXF named `.dwg` | ParseFailure | ... it is a DXF (text); give it the .dxf extension to import it |
| a binary DXF named `.dwg` | ParseFailure | ... it is a binary DXF; save it as an ASCII DXF ... |
| no version string | ParseFailure | ... it does not begin with a DWG version string (AC1009 to AC1032) |
| a release not read | Unsupported | R10 DWG (AC1006) is not read: Katana reads R12 (AC1009) and R13 (AC1012) to R2018 (AC1032) |
| nothing readable (damaged, cut short) | ParseFailure | the R2018 DWG file is damaged, and nothing of it could be read (acadrust: its words) |

**Limits.** acadrust bounds every size a DWG declares for its own structures
before it allocates (its src/io/dwg/size_limits.rs: 16 MiB a system page,
4 MiB a data page, 1 GiB a section, each allocation one that can fail rather
than abort); the DXF reader's limits then hold (20 million entities, blocks
16 deep, `docs/dxf.md`). acadrust's DWG reader catches a panic of its own
decoders; every other acadrust call is guarded the same way here, and a
panic in one is a failure that says so. Damaged copies of DWGs Katana wrote
are read in `crates/katana_dwg/tests/robustness.rs`: each is refused through
the Result or read into geometry the model accepts.

## Writing

1. **The DXF writer** writes the model (or what the export's options take of
   it), exactly as for a `.dxf`: `docs/dxf.md`'s "Writing" table, extended
   data and all.
2. **acadrust reads that DXF** in its failsafe mode: the DXF is Katana's own,
   so a record it cannot read is a defect in one of the two - and the drawing
   is still written, the records left out said in a warning, rather than the
   whole export lost to one record (the round-trip tests count every entity,
   so such a defect fails them). The writer's
   `$DWGCODEPAGE` is left out of the text it reads: with a code page named,
   acadrust keeps an escape of a character that page holds as the escape
   itself (`\U+00E9` for "é"), and the DWG would hold the escape; with none
   named, it reads every escape as its character.
3. **The release** is set on the document, which decides what acadrust's DWG
   writer writes.
4. **From R2004, the true colour.** R2000's DXF holds an indexed colour
   alone, so a colour no index is goes out as the nearest index with the
   colour beside it (`docs/dxf.md`, "Colours"). A DWG from R2004 holds the
   colour itself: the colour beside an entity or a layer becomes its colour
   in the document, so that every program draws it as Katana does. The
   extended data stays, and Katana reads the same colour from either.
5. **Before R2007, the 255-byte string.** A DWG before R2007 holds an extended
   data string behind a length of one byte, and acadrust writes the length as
   a byte whatever the string, which would corrupt the record. A layer's path
   - up to 512 bytes, written beside a name that cannot hold its "/" - can be
   longer: such a drawing is refused for R2000 and R2004 (Unsupported, naming
   R2007), and written from R2007, whose strings are UTF-16 with a two-byte
   length.
6. **The file** is written beside its place and renamed over it, as a DXF is,
   so a failed write leaves the previous file.

The reply is the DXF's, with the DWG's words:

```
exported file=<path> kind=dwg driver=DWG format="DWG R2018" entities= skipped= layers= bytes=
imported file=<path> kind=dwg format="DWG R2018" entities= layers= linetypes= bounds=
```

Not written, as for a DXF: properties, survey attributes, alignments,
surfaces, styles and symbols, blocks (what Katana draws it writes as the
entities drawn).

## On every front end

- **`katana_cli` and the window's command line:** `IMPORT <file.dwg>
  [LOCAL | ALONGSIDE | OFFSET=dE,dN]` and `EXPORT <file.dwg> [<scope>]
  [release=<release>] [PREVIEW]`, the executor's own verbs
  (`crates/katana_app/src/geo/import_verb.rs`, `export_verb.rs`), which take
  a `.dwg` before GDAL is asked what the file is. An import option of another
  kind of data (`band=`, `layers=`) and a GDAL format's option on an export
  (`split=`, `co=`) are refused by name, as for a DXF.
- **The scope and filter** are the shared ones, exactly as a DXF export takes
  them: the annotation is drawn from the whole drawing - a smart leader reads
  the entity its tip is on, which the scope may have left out - and then the
  copy is cut to what the scope took, by the DXF's own function
  (`export_drawing_scoped`). The `scope` record says what it took.
- **`katana_mcp`:** `katana_import` reads a DWG; `katana_export` writes one,
  its `release` the line's `release=`; the replies are the records above, in
  `structuredContent.records` (`docs/mcp.md`).
- **The window:** File > Import routes a `.dwg` to the step a DXF has, where
  it lands (`importPlacementDialog`), before GDAL is asked what the file is;
  File > Export's file dialog offers "DWG drawing", and the Export Drawing
  dialog takes the scope and the DWG release, its Run handing the `EXPORT`
  line to the window's executor - one code path, logged and undoable as a
  typed line is.

## Tests

No DWG file is committed. Every DWG a test reads is made in the test from
values it states, so that what is expected is worked out from those values
and not read off a file of unknown origin: Katana's drawings written by
`katana_dwg` itself, and drawings "another program" wrote, built with
acadrust's own document and writer (blocks and inserts, a true colour, paper
space, the R12 format, a coordinate that is not a number).

- `crates/katana_dwg/tests/round_trip.rs`: every kind, layer, linetype,
  colour, height and script back as it went out, in every release written
  (`dwg_round_trip::every_kind_comes_back_as_the_geometry_it_was_in_every_release`).
- `crates/katana_dwg/tests/reading.rs`: a drawing another program wrote, its
  block expanded through the insert
  (`dwg_reading::a_drawing_another_program_wrote_comes_in_with_its_blocks_expanded_and_its_layers`),
  the import as one undo step, and every refusal of the table above.
- `crates/katana_dwg/tests/writing.rs`: what another program finds in the
  file (the true colour from R2004, the nearest index in R2000, the layer
  name), the long path refused before R2007.
- `crates/katana_dwg/tests/releases.rs`, `robustness.rs`.
- `crates/katana_app/tests/geo_io/dwg_verbs.rs`: the verbs through the
  executor - records, `release=`, the scope, placement, undo, refusals.
- `mcp_server::an_agent_writes_a_dwg_of_the_release_it_asks_for_and_reads_it_back`,
  and the window's dialog and routing in
  `crates/katana_window/tests/gis_vector/export_dialog.rs` and
  `crates/katana_window/tests/main_window/dwg_routing.rs`.
- `cli.a_drawing_exported_to_dwg_and_imported_back_keeps_its_curves_text_and_layer`,
  the DXF test's drawing through `katana_cli` as a DWG, and the refusals
  (`cli.a_dxf_under_the_dwg_extension_is_refused_as_a_dxf`,
  `cli.an_empty_dwg_is_refused_as_not_a_dwg`).

## Not done

- **Nothing is measured.** A benchmark beside `katana_dwg`, reading and
  writing the DXF module's survey drawing as DWG in each release, would give
  the figures the estimate above stands in for.
- **No DWG written by AutoCAD is tested.** acadrust stands in for another
  program; a small drawing saved by AutoCAD (and one by another CAD program),
  with its contents listed beside it and its provenance stated, would be the
  first fixture worth committing.
- **acadrust runs in the session's process.** A panic in it is caught; a
  stack overflow or an abort in it is not, and ends the session. CADCraft
  runs its bridge in a child process with a time limit for that reason. A
  file that sent acadrust into a loop would hold the import's job until it
  is cancelled, and a cancel lands only when the read ends.
- **No budget for a whole read.** acadrust's own limits bound each structure
  (1 GiB a section); `acadrust::DwgReadOptions::max_decompressed_bytes` would bound the
  whole read, and wants a measured ratio of decompressed to file size on real
  drawings before a figure is set.
- **acadrust's other notes are not said.** Its notifications (a code page it
  does not know, read as Windows-1252; objects read as unknown) are not
  passed on; its diagnostics are.
- **`INFO <file.dwg>`** still describes the file through GDAL, which needs its
  CAD driver.
- **R12, R13 and R14 are not written** ("Releases").
- **Blocks are not written as blocks**, nor anything Katana does not read
  kept for a save back: an import makes Katana entities, and an export a new
  drawing, as for a DXF.
