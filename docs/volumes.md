# Volumes

Earthwork quantities between the drawing's surfaces: surface to surface,
surface to a level, stockpiles and pits, along an alignment by cross
sections, on a grid, inside polygons, and the cut/fill map. The owner's
request of 2026-10-15 asked for "volumes: all kinds". Volumes are what an
earthworks estimate, a tender and a claim are paid on, so every method here
is exact where it says it is, says which method it is in every reply, and is
held in the tests to a solid whose volume is known in closed form or to an
independent method.

The terrain layer does the arithmetic (`rust/crates/katana_terrain/src/overlay.rs`,
`rust/crates/katana_terrain/src/volume.rs`, `rust/crates/katana_terrain/src/section_volume.rs`);
the verbs are `katana_cad`'s (`rust/crates/katana_cad/src/terrain/volume_verbs.rs`),
so the window, `katana_cli` and `katana_mcp` run the same code; the window's
Terrain > Volumes only builds their lines
(`rust/crates/katana_window/src/terrain/volumes_dialog.rs`).

## The verbs

```
VOLUME SURFACES <base> <top> [INSIDE <polygon> [EACH]] [bulk=<f>] [shrink=<f>]
       [csv=<file> [OVERWRITE]]
VOLUME DATUM <surface> <level> [INSIDE <polygon> [EACH]] [bulk=] [shrink=] [csv=]
VOLUME STOCKPILE <surface> INSIDE <polygon> [base=boundary|lowest] [level=<z>] [EACH]
       [bulk=] [shrink=] [csv=]
VOLUME SECTIONS <base> <top> ALIGNMENT <name> [interval=10] [from=] [to=] [left=] [right=]
       [method=endarea|prismoidal|both] [curvature=on|off] [bulk=] [shrink=] [csv=]
VOLUME GRID <base> <top> cell=<m> [INSIDE <polygon>] [bulk=] [shrink=] [csv=]
SURFACE DIFFERENCE <base> <top> [NAME <new>] [PREVIEW]
SURFACE ENVELOPE <a> <b> MAX|MIN [NAME <new>] [PREVIEW]

<polygon> := #id [#id ...] | x,y[,z] x,y[,z] x,y[,z] ... | FROM <scope> [WHERE k=v ...]
```

`HELP VOLUME` prints the same (`volume_verb_help`), and the window's Command
Reference has it as its Volumes section. Words are case-insensitive; a
surface or alignment name with a blank is quoted. The `key=value` options
are read wherever they stand - none is a WHERE key, so one may follow a
scope's filter - and a key the form does not take is refused by name, never
ignored.

**Cut and fill are read one way everywhere.** The first surface named is the
existing one (the base), the second the design (the top): fill is where the
top is above the base, cut where it is below, and `net = fill - cut`
(positive: material to bring in) in every record. That is `compare_surfaces`'
reading, which the corridor quantities share. A level is the design for
DATUM, so what stands above it is cut and what lies below is fill
(`volume_to_datum`'s "level design" reading); for STOCKPILE the level, or the
base made from the toe, is the reference, so a pile is cut - the volume that
removes it - and a pit is fill. DATUM and STOCKPILE add `above=` and
`below=`, which say the same without the earthworks reading.

**Replies.** One record per line (`docs/cad.md`), numbers to the millimetre
(cubic or square, as the DEM tools give theirs), a quantity of nothing never
signed:

```
volume method=exact base=ground top=design cut=12.345 fill=100.000 net=87.655 area=400.000 cut.area=95.000 fill.area=305.000
```

With polygons, first what the scope took (`boundary scope= ... matched=`,
the shared scope record), then
`polygons used= holes= skipped.open= skipped.other=` (`holes=` the rings tagged
as holes that joined the polygon they lie in, and so are no polygon of their own
and not in `used=`; a `warning holes.alone=` record follows when some lie in no
polygon of the scope), then with EACH one `polygon
index= entity= polygon.area= ...` record per polygon (`holes=` when it has
some, `polygon.area=` already less them; `chorded=yes` when an arc, a circle,
an ellipse or a spline was chorded), then the total with `polygons=` and
`polygon.area=`.
`area=` is always the plan area measured - where both surfaces (or the
surface and the polygon) have ground - and `polygon.area=` the polygons'
own, so a polygon reaching off a surface shows it. A stockpile adds what it
measured against: `lowest=<z>` for `base=lowest` (the level found) and
`heights=own|surface` for `base=boundary` (whether the toe's own heights were
used, or the surface's because a vertex had none) - on each polygon's record
and, because the total is the only record without EACH, on the total when
every polygon says the same, which one polygon always does (two polygons that
found different levels have no one `lowest=`, and EACH gives each). SECTIONS gives one
`section station= [key=yes] cut.area= fill.area= width=` record per section,
each after the first with its panel's `cut= fill=`, the running `cut.total=
fill.total=` and the mass haul ordinate `mass=` (and, on a panel that lies on a
curve, `radius= turn=left|right curvature.cut= curvature.fill=`: its radius at
the middle, and what the curvature correction below moved its volumes by), then
one `volume method=endarea` and/or `volume method=prismoidal` total, each
saying `curvature=on|off` and, when on, `curvature.cut= curvature.fill=` - what
the correction moved that total by, signed, so a figure that differs from the
plain end area says by how much.

**What changes.** The VOLUME forms change nothing. `csv=<file>` writes the
quantities as a table (RFC 4180: CRLF, a field with a comma or quote quoted),
refusing a file already there unless OVERWRITE, as every verb that writes a
file does. The table of the surfaces, a level, a stockpile or a grid has a row
per polygon with EACH and one for the total, with `record, polygon, entity,
polygon_area, area, cut, fill, net` first (and the adjusted three after them
when factors are given) and then what the reply also says: `cut_area,
fill_area` (not for the grid), `above, below` (a level or a base), `cells`
(the grid), and on every row the method and its inputs, the head of the reply
- `method, base, top` or `method, surface, level` or `method, base, top, cell`
- and `bulk, shrink` when factors are given, so a file handed on says whether
it is the exact method or the grid, and of what. The SECTIONS table is one row
per section: the section's areas and its panel's volumes, running totals and
mass ordinate, then `method` (endarea, prismoidal or both - with
`method=prismoidal` the plain `cut`, `fill`, `cut_total`, `fill_total` and
`mass` columns hold the prismoidal volumes, which only this column tells from
an end-area file, and `both` adds the `*_prismoidal` columns), `curvature` (`on`
or `off`: whether the volumes carry the curvature correction, which only this
column tells a file made one way from one made the other), `base, top,
alignment, interval, left, right`, and `bulk, shrink` when given. The totals
are the last row's running totals; the prismoidal correction is the difference
of the two sets of columns under `both`. Rejected: appending the totals as rows
of the section table, which would put a volume under the station column.
When the scope takes no polygon nothing runs and nothing is written, whatever
OVERWRITE says, and the reply says so (`csv file=<f> written=no reason=...`)
where it used to say nothing, so a file an earlier run left is known to be as
it was
(`volume_verbs::a_csv_with_a_scope_that_takes_no_polygon_writes_nothing_and_says_so`,
`volume_verbs::a_table_names_its_method_and_inputs_and_carries_the_areas_levels_and_cells_of_the_reply`,
`volume_verbs::a_section_table_says_whether_its_volumes_are_end_area_or_prismoidal_and_what_made_them`). SURFACE DIFFERENCE and ENVELOPE add a surface to the drawing
through `Document::set_surfaces`: one undo step, named after the verb; with
PREVIEW they say what they would add and add nothing.

**Refused, saying why:** a surface or an alignment the drawing has not got
(NotFound); surfaces with no ground in common, or polygons with nothing of
it inside (InvalidGeometry, naming each surface and the extent it covers,
whether their boxes do not meet or meet over triangles that do not -
`volume_verbs::a_refusal_that_two_surfaces_share_no_ground_names_what_each_covers` runs
every form; SECTIONS also when no section of the range reaches ground both
have); sections that cross one another, or stretches of the alignment that run
side by side (InvalidArgument, below); `curvature=` that is neither on nor off; an
`#id` that is no polygon - not a closed polyline, curve polyline or spline, circle
or ellipse (InvalidGeometry, saying what it is); fewer than three typed corners; EACH
without INSIDE (and EACH on VOLUME GRID, which counts each cell once across
all the polygons: the refusal names the forms that take it); a factor that is
not more than 0; polygons that overlap without EACH (not for GRID). A scope
that takes no closed polygon is said - `polygons used=0` and `ran=no` - not
refused (`docs/cad.md`, "Scope and filter").

## One overlay

Every volume between two surfaces integrates their difference, and the
difference is linear only where both surfaces are: inside one triangle of
each. `katana_terrain::overlay::visit_overlay` cuts the common ground into such pieces - each
the intersection of a triangle of each surface, a convex polygon of at most
six corners, fanned from its first corner - and hands each to a visitor with
both surfaces' heights at its corners. It is the pair clipping
`compare_surfaces` already did (`docs/terrain.md`, "The fix: clip triangle
pairs"), moved out so the volumes inside a polygon, the stockpiles and the
difference and envelope surfaces cut the ground the same way.
`compare_surfaces` is now a visitor of it, with the same arithmetic in the
same order: its results are the bits they were. Nor did it slow down:
`BM_CompareSurfaces` (the C++ build's `bench_terrain` benchmark) in Release on the
24-thread development machine, five repetitions each, the two builds run
alternately twice on 2026-10-15, gave a median of 51.5 and 51.9 ms before
and 50.0 and 50.6 ms after, and `BM_CompareSurfacesSerial` 491 and 508 ms
before and 504 and 507 ms after - each within the spread of its own runs.

**The same bits at any thread count.** The base surface's triangles are cut
into blocks of a fixed `OVERLAY_BLOCK_TRIANGLES` (256), and a block's pieces
arrive on one thread in a fixed order: base triangle ascending, the top
triangles it meets ascending, then a region's triangles ascending, then the
fan. A visitor that sums each block into its own slot and combines the slots
in block order is deterministic, which `compare_surfaces`, the region
comparison and the difference surface are held to
(`volume_inside_polygon::gives_the_same_bits_on_one_thread_as_on_many`,
`difference_surface::is_the_same_surface_at_any_thread_count`).

**Coincident ground is neither cut nor fill.** `cut.area=` and `fill.area=`
are the plan area where the top is below and above the base, and a piece's
corner heights are read off each triangle's plane by barycentric weights that
sum to 1 only to a few ulps: two surfaces that coincide (one plane
triangulated two ways, a flat surface against its own level inside a polygon,
a stockpile's ground beyond its toe against a base made of that ground) differ
there by +/- 1e-15, not 0. Sorted by the sign of the last bit, whole pieces of
unchanged ground were booked as cut area or fill area: 44.6 m2 and 23.3 m2
inside a skewed polygon of a surface at its own level, 750 m2 and 9 m2 on one
plane in two triangulations (100 m2 the other way round), 700 m2 of cut and 75 of
fill round a toe of 400. A pay item measured by area would have been paid on
rounding. The areas are now sorted with every corner difference within
`GEOMETRIC` (0.1 micrometre) of zero taken as zero - the one tolerance of the
policy, not a second for heights - and a piece whose corners are all within it
is neither. The volumes are summed from the differences as they are, so their
bits are unchanged, and a coincidence within 0.1 micrometre keeps the (at most
that times the area) volume it holds: `compare_surfaces` of a 5e-8 rise over
10 000 m2 is 5e-4 m3 of fill and no fill area, a rise of 1 mm is 10 m3 and all
10 000 m2
(`compare_surfaces::one_plane_triangulated_two_ways_has_no_cut_area_and_no_fill_area_in_either_order`,
`volume_to_level_inside_polygon::a_flat_surface_against_its_own_level_has_neither_cut_area_nor_fill_area`,
`stockpile::on_sloping_ground_with_a_polygon_wider_than_the_toe_only_the_toe_is_cut_area`).
Rejected: a tolerance relative to the heights (a few ulps of the largest), which
a sliver triangle's weights, off by the triangle's aspect ratio times that,
would have overrun, and which would be a second constant beside `GEOMETRIC`.
Heights are in metres; a model in millimetres would need the tolerance scaled.

**Joining the pieces into a surface.** A difference or an envelope is a TIN
made of the pieces, so a corner two pieces share must be one vertex. Each
corner is named by what it is - a vertex of one surface, or the crossing of
an edge of one with an edge of the other - and a crossing is placed from the
two edges' own end points in a fixed order, never from the corners of the
polygon being clipped, so the pieces on either side of an edge compute it to
the same bits. Two names for one place (a base vertex where the top has one
too, common where a design was built on the ground's own points) are welded.
The joined mesh goes to `TinSurface::create`; if floating point has left two
pieces overlapping by a sliver, which `create` refuses as two triangles along
one edge in the same direction, the same vertices and every piece edge go to
the constrained triangulation instead, whose exact predicates settle it, and
the triangles outside every piece are left out again (`retriangulated=yes`
in the reply). `difference_surface::made_through_the_triangulation_it_measures_the_same`
asks for that path and holds its volumes to the direct one's.

## Inside a polygon

`katana_terrain::OverlayRegion` is a polygon and its holes triangulated once - a
constrained Delaunay triangulation of its own vertices, so its triangles tile
it exactly and add no point - and the overlay's pieces are cut again by those
triangles. The polygon's edges are therefore followed exactly, through the
middle of triangles, not by whole triangles: a 1 m slab under a triangle
whose edges cross both surfaces' diagonals measures the triangle's shoelace
area, 2088.635 m3
(`volume_inside_polygon::the_edges_of_a_polygon_cut_triangles_where_they_lie_not_by_whole_triangles`).

- **The polygon** is an `#id`, typed corners, or what a scope takes, read
  by the one reader of the drawing's polygons that the surface edits use too
  (`rust/crates/katana_cad/src/terrain/drawing_shapes.rs`, which says what each kind
  of entity gives); `read_volume_polygons` only words its refusals. A closed
  polyline or curve polyline, one drawn back to its first vertex without
  being closed, a circle, a full ellipse and a closed spline are polygons,
  each at its heights; lines, arcs, elliptical arcs, open polylines and
  outlines of fewer than three corners are counted as `skipped.open` (they
  bound no area), and points, text and splines that do not return to their
  start as `skipped.other`. An arc, a circle, an ellipse and a spline are
  chorded within `katana_geometry::curves2d::CURVE_CHORD_TOLERANCE` (1 mm sagitta), and the
  record says `chorded=yes`.
- **A spline that returns to its start is a polygon.** `Spline2` has no closed
  flag, so a spline whose end is within `GEOMETRIC` (0.1 micrometre) of its
  start (`Spline2::is_closed_shape`) is a closed outline, flattened by
  `Spline2::tessellate` within the same 1 mm sagitta as the circle and the
  ellipse, its repeated end taken off, every corner at the entity's elevation
  property when it has one (a spline holds no height of its own; the ellipse's
  and the circle's are read the same way, so the three agree). One that does
  not return to its start is no outline, `skipped.other` as it always was
  (`drawing_shapes::a_closed_spline_is_a_polygon_chorded_within_the_tolerance_at_the_entitys_elevation`,
  `drawing_shapes::a_spline_is_closed_when_its_end_is_within_the_geometric_tolerance_of_its_start_and_not_otherwise`,
  `volume_verbs::a_spline_drawn_back_to_its_start_is_a_volume_polygon_and_an_open_one_is_counted_as_other`).
  A spline of 5e-8 between its ends is closed and one of 5e-7 is not. Rejected:
  closing an open spline with a chord when its ends are near: a spline is not a
  polyline that was meant to close, and a chord is a guess at the outline.
  Because the reader is the surface edits' as well, SURFACE EDIT polygons take a
  closed spline now too, by the rule of "One reader, the more capable rule".
- **A lot with a hole.** The drawing has no polygon with a hole: a ring drawn
  inside another is a polygon of its own (a building in a lot is not a hole in
  the lot). A ring is a hole only when it says so - `source.ring=hole`, which
  IMPORT writes into an entity's metadata, or `gis.ring=hole`, which a
  geoprocessing result writes into its properties (`rust/crates/katana_entity/src/ring_tags.rs`) -
  and then it joins the polygon it lies in, as the GIS conversion joins it
  (`drawing_dataset::a_tagged_hole_joins_its_exterior`): the polygon whose part
  (`source.part` or `gis.part`) it shares, else the smallest that holds every
  vertex of it, else the one made nearest before it. The rule is one function,
  `katana_entity::HoleJoiner`, which the conversion (`katana_interop::geo::drawing_dataset::drawing_dataset`) calls
  as well, so a lot with a hole has the same area whichever reads it; it lives
  in the entity layer because interop may not see cad and cad may not see
  interop. The polygon is then `OverlayRegion::from_polygon(ring, holes)`: its
  area is the ring's less the holes', the overlay is cut by it, the grid and
  the volume to a level take it, and a stockpile is measured round the holes
  (`StockpileOptions::holes`: their edges count as boundary for
  `base=lowest`, and `base=boundary` is made round them at the surface's
  heights - a hole has none of its own, and one with a vertex off the surface
  is refused naming it). A 50 x 50 lot with a 10 x 10 hole under a 1 m slab holds
  2400 m3, by every method that takes a polygon
  (`volume_verbs::a_lot_with_a_tagged_hole_is_measured_less_the_hole_by_every_method_that_takes_a_polygon`,
  `cli.volume_inside_a_lot_with_a_tagged_hole_measures_the_lot_less_the_hole`).
  The rings are read together, in the one call that reads the polygons (an
  `#id` list or a scope): a hole named before its lot joins it all the same, and
  a tagged ring that no polygon of the call holds is measured as a polygon of
  its own and said (`warning holes.alone=`), as the conversion keeps it. A ring
  not tagged is a polygon, and a building in a lot overlaps it
  (`volume_verbs::a_ring_not_tagged_as_a_hole_is_a_polygon_of_its_own_and_overlaps_the_lot`).
  Rejected: reading a ring inside another as a hole by its position alone (a
  building in a lot is not one), and a copy of the conversion's rule in cad
  beside the one in interop (two rules for the same words would drift, and the
  area of a lot would depend on which verb asked).
- **One reader, the more capable rule.** The volumes and the surface edits
  were written side by side with a reader each, and the two had come to
  disagree: these verbs refused a full ellipse and a polyline drawn back to
  its start that the edits took, and the edits dropped a polygon's heights
  and refused FROM with no scope word, which these took as the selection.
  Where they differed the shared rule is the one that takes more, since each
  refusal was an accident of which reader was written, not a reason: so a
  polygon here may now be an ellipse or a polyline drawn back to its start
  (`volume_verbs::a_full_ellipse_and_a_polyline_drawn_back_to_its_start_are_volume_polygons`).
  A closed outline of fewer than three corners, which used to be taken and
  then refused by the overlay, is now counted with what bounds no area.
  Rejected: keeping `VolumePolygon` as a thin adapter over the shared shape,
  which would have kept a second representation of the same polygon.
- **A volume to a level inside a polygon** compares the surface with the
  region made flat at the level (`OverlayRegion::at_level`): the same
  overlay, so the same exactness.
- **Several polygons** are measured one at a time and summed. Two that
  overlap would count the ground they share twice: without EACH that is
  refused, naming them and the area; with EACH each has its own record and a
  `warning overlap=` record says the total counts it twice. The overlap is
  found by the overlay itself - a 1 m slab over one polygon cut by the other
  - and anything under a square millimetre is rounding, so polygons sharing
  an edge (cadastral lots) are not refused. VOLUME GRID is the exception:
  it counts cells, not ground, and counts each cell once across all the
  polygons (see "The grid method"), so it takes overlapping polygons and has
  no EACH.
- **A polygon partly off the ground** measures the ground under it, and
  `area=` against `polygon.area=` says how much that was
  (`volume_to_level_inside_polygon::a_polygon_partly_off_the_surface_measures_the_ground_under_it`).

## Stockpiles and pits

`katana_terrain::volume::stockpile_volume` measures a surface inside a boundary against a
base made from the boundary:

- **`base=boundary`** (the default): the boundary's heights triangulated
  inside it, the toe of the pile as it stands. The boundary's own height at
  every vertex when it has one at each (a 3D string surveyed along the toe,
  or typed `x,y,z` corners); otherwise the surface's at each vertex - all or
  none, never a mixture, so one vertex without a height means the others' own
  heights are not used either. A vertex off the surface is then refused,
  naming it: its height would be a guess, and a base at 0 there would add a
  cone of nothing. Where that vertex has a height of its own the refusal says
  so and names the vertex that has none (`boundary vertex 2 is off the
  surface, and its own height is not used: the boundary's own heights are used
  only when every vertex has one, and vertex 1 has none`), and not that it has
  no height of its own, which was untrue and sent a person to the wrong vertex
  (`stockpile::an_off_surface_vertex_with_its_own_height_is_refused_naming_the_vertex_that_has_none`).
  Rejected: a per-vertex fallback (own where present, the surface's elsewhere),
  which would change the documented rule and the `heights=` it reports.
- **`base=lowest`**: a level plane at the lowest point of the surface along
  the boundary - its vertices and every place an edge of the surface crosses
  it, read exactly by the segment profiler below.
- **`level=<z>`**: a level plane at z.

A pile of facets on level ground is a pyramid, a third of its base times its
height: twelve faces on a toe of radius 20 hold (12/2) 20^2 sin 30 deg x 9 /
3 = 3600 m3. On sloping ground the toe's base follows the slope and the
answer does not move, because a pyramid's volume is its plan area times its
height plumb from the base plane. 360 faces come within 0.19 m3 of the cone's
pi r^2 h / 3, the circle's area beyond the polygon's
(`stockpile::many_facets_approach_the_cone_formula`).

## Along an alignment

`katana_terrain::section_volume::volumes_along_alignment` cuts cross sections square to an alignment
and takes the volume between consecutive ones from their areas, the way a
road job is measured and paid.

- **Stations:** every whole multiple of `interval=` (10, 20, 30 ... for 10,
  whatever the start), the range's ends and the alignment's key stations (TS,
  SC, CS, ST - `SolvedAlignment::key_stations`), where the geometry changes
  and a section is always cut. Two closer than `COORDINATE` (0.1 mm) are
  one. More than 100 000 sections is a unit mistake and refused, the cap
  `katana_terrain::contours` puts on its levels.
- **Reach:** `left=` and `right=` from the centreline; absent, the diagonal
  of the ground the two surfaces share, so a section reaches across all of
  it wherever it is cut. That is right on a straight run and wrong anywhere
  the alignment turns or doubles back: see "Sections that cross" and
  "Stretches side by side" below, which refuse it.
- **Sections that cross are refused.** The average end area takes the volume
  of a panel from the areas of the two sections that bound it, and is right
  only while the panels tile the ground. Where two sections cross - the
  inside of a curve cut deeper than its radius, where they all pass through
  its centre; the inside of a corner (a PI with no radius) deeper than the
  interval over the sine of its deflection; two straight legs at an angle,
  each swept by sections that reach across the other - the ground beyond the
  crossing is swept by both and counted by both. A 1 m slab holding 40 000 m3
  measured 62 101 m3 round a curve of radius 40 with the default reach, and
  62 101 again at an interval of 0.1 m: a shorter interval does not help,
  because the sections cross at the centre however close they are. The
  verbs now refuse it (InvalidArgument), naming the first two stations that
  cross and the longest reach at which none do - for a curve its radius -
  and measure nothing. `left=15 right=15` over the same curve gives
  30 x (240 + 20 pi) = 9084.956 m3, the band's width times its centreline's
  length
  (`volumes_along_alignment::sections_across_a_curve_deeper_than_its_radius_are_refused_not_counted_twice`).
  Sections that only meet at their ends (a reach equal to the radius, which
  ends every section of the curve on its centre) are not crossing; the test
  is a proper crossing by more than `GEOMETRIC` at both ends. In the window
  the Left and Right fields are blank for "across the common ground", so a
  person on a curve gets the refusal in the status line and types a reach.
- **Stretches side by side are refused too.** Two parts of an alignment that
  run side by side nearer than twice the reach sweep the same ground twice, and
  a section of one need not cross a section of the other to do it: the legs of
  a U-turn made of corners, or two roads laid in one alignment, have parallel
  sections. So the ground each panel sweeps - the quadrilateral between the
  reach segments of its two sections, as two triangles that tile it, cut along
  the diagonal that lies inside it because a panel across a corner can be
  concave - is tested against that of every panel but its neighbours (which
  share a section and touch along it), by the separating axis test: two panels
  overlap when their shadows overlap by more than `GEOMETRIC` on every normal
  of every edge, which is the shortest way one would have to move to clear the
  other, so strips that only touch, or that overlap by less than 0.1
  micrometre, are measured. The pairs come from the same kind of box index as
  the crossing test (`SpatialIndex`), one task per panel on the pool, and the
  stretches are named from the panels' slots in station order, so the refusal
  is the same words at any thread count. It names the first stretch (the run of
  consecutive panels from the first that overlaps another, by station), the
  stretch it overlaps, how many pairs of panels overlap, and says to shorten
  `left=` and `right=` (with the same reach each side, under half the distance
  between the stretches). Legs 15 m apart with 10 m each way are refused
  ("from station 0.000 to 100.000 runs side by side with the stretch from station
  120.000 to 215.000"); 7 each way is measured, 7.5 each way touches along one
  line and is measured, 7.5 + 1e-6 is refused and 7.5 + 1e-8 is not
  (`volumes_along_alignment::two_legs_of_au_turn_closer_than_twice_the_reach_are_refused_naming_the_stretches`,
  `volumes_along_alignment::strips_that_only_touch_or_clear_one_another_are_measured`,
  `volumes_along_alignment::legs_whose_strips_are_asymmetric_are_refused_only_when_the_strips_meet`,
  `cli.volume_sections_along_two_legs_side_by_side_are_refused_naming_the_stretches`).
  A curve cannot do it (the legs either side of a curve of radius R are 2R
  apart, and a reach over R crosses at its centre), so the test costs a curve
  nothing. Cost: it is the crossing test's again - the pairs whose boxes
  overlap - and a pair is four triangle pairs of at most six axes each, with
  an early exit at the first axis that separates; not measured at the sizes
  where it matters, see "Not done".
- **A section on a surface's boundary is whole.** A corridor's finished
  surface ends on its first and last sections, and a person measuring it along
  its own alignment cuts exactly there. The ends of a section are computed
  from the bearing, so on a north-going alignment they are tilted by
  cos(pi/2) = 6e-17 of the reach, 9e-15 m at 141 m, and an exact test keeps
  whichever half of the segment the tilt leaves inside the edge. Both ends of a
  segment within `GEOMETRIC` (0.1 micrometre) of an edge's line put it ON the
  edge: inside, as the exactly parallel case always was. A 1 m slab holding
  10 000 m3 measured 9500 along north, south and west alignments (500 m3 gone
  from a paid quantity, with nothing in the reply to say so) and 10 000 along
  east, where sin 0 = 0 exactly
  (`volumes_along_alignment::a_slab_measured_along_any_cardinal_alignment_is_whole_where_its_end_sections_lie_on_its_boundary`,
  and at every fifteenth degree with the slab's corners cut by the alignment
  itself, as a corridor's are). Rejected: cutting the end sections a hair inside
  the range, which fixes the ends only, to within that hair, and not a boundary
  that runs along the range.
- **The section areas are exact.** A section is a straight segment; each
  surface along it is the run of pieces where the segment crosses one of its
  triangles (`katana_terrain::section_volume::SegmentProfiler`, Cyrus-Beck clipping against each
  triangle the segment's box meets), and where a piece of one meets a piece
  of the other the difference is linear, so each span is a trapezoid - two
  triangles of opposite sign where the surfaces cross in it
  (`katana_terrain::section_volume::signed_trapezoid`). An edge's crossing and its height are
  computed from the edge's own vertices, so neighbouring pieces meet at the
  same bits (`segment_profiler::a_plane_read_along_a_segment_is_the_plane_on_every_piece_and_the_pieces_meet_exactly`).
  A section's `width=` is the length on which both surfaces had a height.
  When no section of the range has any, the surfaces share no ground there
  (their boxes may meet over triangles that do not) and the verb is refused
  as every other form is, naming the alignment, both surfaces and what each
  covers; some sections empty and some not is a measurement, with
  `sections.empty=` saying how many.
- **The volumes are the methods'.** Between stations L apart with areas A1
  and A2, the average end area is L (A1 + A2) / 2 and the prismoidal formula
  L (A1 + 4 Am + A2) / 6, Am the area of the section cut at the middle
  station - not the mean of A1 and A2 - both as Ghilani and Wolf give them
  (*Elementary Surveying: An Introduction to Geomatics*, the chapter
  "Volumes"). The prismoidal formula is Simpson's rule on the area as a
  function of station, exact when that is a polynomial of degree three or
  less (Burden and Faires, *Numerical Analysis*, "Elements of Numerical
  Integration": Simpson's rule has degree of precision three). `method=both`
  reports both and the prismoidal correction, end area less prismoidal.
- **On a curve the volumes carry the curvature correction (Pappus).** The
  length L between two sections is the centreline's, but the ground at offset
  o (left positive) on a curve of curvature k (positive turning left) is swept
  along an arc (1 - k o) times as long: a point on the inside travels less than
  the centreline, one on the outside more. A panel of a constant section of
  area A, between two normals of a curve of radius R, therefore holds A (R -
  e) theta, not A R theta, with e the centroid's offset from the centreline,
  positive toward the centre of the curve; the end areas along the centreline
  are off by the fixed factor R / (R - e) however short the interval (and a
  section on the outside, e negative, holds more). A quarter circle of radius 50
  with a strip 10 m wide on its inside, 1 m deep, holds 500 + (pi/4)(50^2 -
  40^2) + 500 = 1000 + 225 pi = 1706.858 m3 and measured 1785.398 (the 10 m2
  over the centreline's length 100 + 25 pi) at every interval - 4.6 % over; on
  its outside 1000 + 275 pi = 1863.938 against the same 1785.398, 4.2 % under.
  In the section engine, so every consumer gets it (the verbs, the window,
  katana_mcp): each section has the first moment M of its cut and of its fill
  about the centreline - the integral of the depth times the offset, in closed
  form on every strip of the section (`signed_trapezoid_moments`), the same
  strips the areas are cut into - and the volume between two normals is the
  integral of A(s) - k(s) M(s), each section's cut and fill on their own,
  since each has its own centroid. The average end area is then L ((A1 + A2) - k
  (M1 + M2)) / 2, the prismoidal formula L ((A1 + 4 Am + A2) - k (M1 + 4 Mm +
  M2)) / 6, with k the curvature at the **middle of the panel**: every panel
  lies on one element of the alignment (the key stations are all sectioned), so
  on an arc that is its constant 1/R, on a tangent 0, and on a spiral, where
  curvature is linear in station, k L at the middle is exactly the angle the
  panel turns - the sum of them is the alignment's whole deflection, whatever
  the spirals do (a strip of 10 m2 and M = 50 along tangent, spiral, circle,
  spiral and tangent loses 50 x pi/2 = 78.540 m3 to a 90 degree PI, which the
  test holds to 1e-6). A panel that is not a constant section carries the
  error of the rule it was always under: the panel sum is the trapezoid rule
  on F = A - k M, whose error is (h^2/12)(F'(b) - F'(a)) and at most h^2 L
  max|F''| / 12 for panels h long over a length L, and Simpson's for the
  prismoidal; for the sloping design z = 0.02 x over the strip above, F = 9 +
  8.1333 cos(phi) per metre of station on the arc and the exact volume is 2313.525
  against 2435.398 uncorrected, the corrected sum within 0.0215 h^2 of it at
  every interval - 0.086 at 2 m, 0.0215 at 1 m, 0.0013 at 0.25 m - while the
  uncorrected stays 121.9 away
  (`volumes_along_alignment::a_sloping_surface_on_a_curve_is_corrected_to_its_exact_volume_as_the_interval_shrinks`).
  The standing tests are the strips above
  (`volumes_along_alignment::a_strip_on_the_inside_of_a_quarter_circle_is_the_annulus_between_its_radii_not_the_centreline_arc`,
  `volumes_along_alignment::a_strip_on_the_outside_of_a_quarter_circle_is_longer_than_its_centreline_arc`),
  the same way back, turning right, where the inside is the right side
  (`volumes_along_alignment::the_same_strip_on_a_way_that_turns_right_has_its_inside_on_the_right`), a
  strip symmetric about the centreline, 20 m2 and M = 0, 2000 + 500 pi =
  3570.796 either way (`volumes_along_alignment::a_strip_symmetric_about_the_centreline_is_the_same_with_or_without_the_correction`),
  and a straight, where nothing changes by a bit
  (`volumes_along_alignment::on_a_straight_the_correction_changes_no_bit_of_an_asymmetric_section`).
  **The default changed the figures of a curved alignment with asymmetric
  sections**; a straight and a symmetric section did not move.
  `curvature=off` gives the plain end area along the centreline (1785.398 above,
  and the reply and the CSV's `curvature` column say `off`). A PI with no radius
  is a deflection, not a curve: k is 0 on each leg and the correction does not
  see it. The reach on the inside is at most the radius (the crossing test
  refuses more), so 1 - k o is never negative.
  Rejected: a `CURVATURE OFF` word pair - every other choice on this verb is a
  `key=value` and the reply says `curvature=on|off` in the same words;
  leaving the correction off by default - it is the standard method and the
  plain figure is 4 % wrong on a 10 m strip at a radius of 50; correcting only
  the sections' areas by a factor of the whole alignment's radius - a
  spiral and a compound curve have no one radius; the curvature at each end
  of a panel in the place of its middle - on a spiral that is the
  trapezoid on k M, whose error for a section that changes is twice the
  midpoint's.
- **The mass haul ordinate** is the cut less the fill, summed from the start
  of the range, with the factors applied - the mass diagram's convention,
  cut positive. Rising, the job has cut to spare; falling, fill to bring.

The test of all three is a wedge whose section grows in width and depth
along the alignment, so its area is s^2 / 50: exactly 100^3 / 150 = 6666.667
m3, which the exact overlay also gives (0.1 x the triangle's 1000 m2 x its
centroid's x of 200/3). On sections 10 m apart the prismoidal formula gives
it exactly and the average end area 6700, over by the trapezoid rule's
(b - a) h^2 A'' / 12 = 33.333
(`volumes_along_alignment::on_a_wedge_the_prismoidal_formula_is_exact_and_the_end_area_overstates_it`).

## The grid method

`katana_terrain::volume::grid_volume`: square cells on lines at whole multiples of the cell
(so grids of one cell size line up, as RASTER GRID's do), each counted when
its centre is on both surfaces - and inside the polygon, when there is one -
as a prism of the cell's area and the difference at its centre: the midpoint
rule. It is a cross-check of the exact methods and the way a grid of cells is
measured, and it converges on the exact answer as the cell shrinks. How fast
is known on a pyramid: its height is linear on every cell but those its four
ridges run through, and there the larger of two uniform offsets averages c/6
more than at the centre, so each such cell's prism stands 0.1 c too high
(for the 30 m pyramid on 100 m) - over the 200 / c ridge cells, 20 c^2 in
all, which the test holds to 1e-6 m3 on 5, 2, 1 and 0.5 m cells
(`grid_volume::the_midpoint_rule_overstates_a_pyramid_by_a_tenth_of_a_cell_cubed_in_each_cell_on_a_ridge`).
More than 25 million cells is refused, naming the cell that fits, as a
surface's raster export caps its grid.

**Several polygons: each cell once.** A centre on a polygon's edge is inside
it (`OverlayRegion::contains`: the boundary counts as inside), and cell
centres sit at (k + 0.5) x cell, so lots whose shared edge falls on a line of
centres hold those cells in both. Measured one at a time and summed, a 1 m slab
under lots [5, 25.5] x [5, 45] and [25.5, 45] x [5, 45] on 1 m cells gave
840 + 800 = 1640 cells where their union [5, 45]^2 holds 40 x 40 = 1600 (and
the exact method 1600 m3): the 40 centres on x = 25.5 twice, +432 m3 of cut on
a site of four quadrants. Each polygon is now measured with the regions of the
polygons before it as `claimed` (`grid_volume`), and a cell any of them holds is
left to the first, so the cells counted, summed, are those of the union
(`grid_volume::lots_sharing_an_edge_on_a_line_of_centres_count_its_cells_once_not_twice`,
`volume_verbs::lots_that_share_an_edge_on_a_grid_line_are_counted_once_by_the_grid_as_by_the_exact_method`).
Because the grid never counts a cell twice it has no use for the refusal the
exact forms need: overlapping polygons are taken, `area=` is the union's,
`polygon.area=` is the polygons' own summed (so it counts an overlap twice, and
a `warning overlap=` record says so), and EACH is refused, naming the forms that take it, since the
cells are shared out among the polygons and not each one's own. The order of the polygons
decides only which of two lots a shared line of cells is credited to, never the
total. A polygon's own edge cells still count (`area=` may exceed
`polygon.area=` by the midpoint rule's usual boundary cells).

The DEM tools' RASTER DIFFERENCE (`docs/terrain.md`, "The DEM tools") is
the same midpoint rule on rasters, but NOT on the same cells, so the two
agree only where the cells coincide. VOLUME GRID puts its cells on lines at
whole multiples of the cell. A `SURFACE <name> CELL <m>` source of RASTER
DIFFERENCE is sampled by `surface_grid` on the surface's own lattice, anchored
at its (min x, max y) corner, `ceil(width / cell)` by `ceil(height / cell)`
cells whose last column and row reach up to half a cell past the surface; and
where the two rasters' lattices differ RASTER DIFFERENCE resamples the second
bilinearly onto the first (`aligned=no resampling=bilinear`), so its depths are
not the two surfaces read at the same centres. On the sample terrain against a
tilted design the two verbs count 23 986 and 23 674 cells and their fills are
2.9% apart and their nets 46% (the exact 2542.912; VOLUME GRID 2107.914; RASTER
DIFFERENCE 3078.654). They agree exactly where both surfaces' rasters happen to
start on whole multiples of the cell - two surfaces over [0, 100]^2 with a cell
of 1 m give 10 000 cells each - so set them side by side only there, and take
the exact method as the reference. Not made to agree by snapping the surface
raster to whole multiples (a new option on `surface_grid`): that would change the
grid RASTER EXPORT writes, for a cross-check nobody asked to be exact.

## Factors

`bulk=` multiplies the cut and `shrink=` the fill, each as given, in the
reply only (`cut.adjusted= fill.adjusted= net.adjusted=`, and the mass
ordinate) - never the geometry, so the measured volumes stay what was
measured. Which measure the factors put the two in (bank, loose, compacted)
is the person's: the program does not decide it.

## The difference and envelope surfaces

SURFACE DIFFERENCE makes the top minus the base over their common ground
(`katana_terrain::overlay::difference_surface`): an isopach, linear on every overlay piece,
so exact. Positive is fill, negative cut, and it is given the built-in "Cut
Fill" style (blue cut below 0, red fill above) when the drawing still has
it, so it draws as the cut/fill map. Integrated against 0 its above and below
are the comparison's fill and cut
(`difference_surface::it_integrates_to_what_compare_surfaces_measures`). Its default
name is `<top>-<base>`, made unique; NAME must be free.

SURFACE ENVELOPE makes the higher (MAX) or lower (MIN) of two surfaces over
their common ground (`katana_terrain::overlay::envelope_surface`): a piece on which the two
cross is split along the line where they meet - a vertex per edge it crosses,
placed from the edge's ends in a fixed order so the pieces either side share
it - and each part is one surface's plane. It is drawn in the drawing's
default style, NOT Cut Fill: an envelope holds real levels (24 to 39 m on the
sample terrain), and the Cut Fill ramp ends at +2, so every face would be
painted the one colour of fill, a uniform red sheet whose legend read -2 to 2
(`volume_verbs::the_envelope_is_drawn_in_the_default_style_not_the_cut_fill_map_of_a_difference`).
The style is the caller's choice in `add_made_surface`: the difference passes
"Cut Fill", the envelope none.

Both reply with the first record of what they made (`difference` or
`envelope`) and then the surface as every reply that names one writes it,
`katana_cad::surface_record` (the same keys and values SURFACE INFO gives, `visible=`
among them, and a `style=` that names a style: the drawing's default once
Cut Fill has been deleted, where the reply used to say `style=default`, which
names none), with `preview=yes` appended under PREVIEW
(`volume_verbs::a_surface_these_verbs_make_is_reported_as_surface_info_reports_it_with_or_without_cut_fill`).

## In the window

Terrain > Volumes (`terrainVolumes`, a Terrain toolbar button, the painted
`Volumes` icon) opens `volumesDialog`, kept between uses and following the
drawing while it is open (a `DocumentWatcher` reads the surfaces, alignments
and layers again from the event loop on every execute, undo, redo, New and
Open, keeping what is chosen where it is still there - an UNDO of the map the
dialog made no longer leaves it offered, nor a Measure refused NotFound on it
(`volumes_dialog::the_lists_follow_the_drawing_while_the_dialog_stays_open`)): a page per method,
the boundary - Everywhere, the polygons the shared "Apply to" and "Only
those that match" controls take (`ScopeFilterWidget`, prefix `volumes`), or
typed corners - with EACH, the bulk and shrink factors, and the line the
fields make in `volumesCommand`. The Along Alignment page has a "Correct for
curvature" box (`volumesCurvature`, checked, the verb's default): unchecked, the
line gets `curvature=off`
(`volumes_dialog::the_curvature_box_is_on_by_default_says_nothing_then_and_curvature_off_when_unchecked`).
Measure runs it through the window's one
executor; the reply's records fill `volumesResults` (a column per key, in
the order first met) and, along an alignment, the mass haul diagram
(`volumesMassHaul`). Copy puts the table on the clipboard; Save CSV runs the
same line with `csv=` and OVERWRITE (the file dialog asks before choosing a
file) and says "Saved the table to ..." only when the reply carries the `csv`
record with its rows - a scope that takes no polygon writes nothing, and the
status says the file was not written
(`volumes_dialog::save_csv_of_a_scope_that_takes_no_polygon_says_nothing_was_saved_and_leaves_the_file`); Make Cut/Fill Map (`CutFillMap` icon) runs SURFACE DIFFERENCE of the
page's two surfaces and offers the new surface at once; Make Envelope
(`volumesEnvelope`, beside `volumesEnvelopeSide`, Higher or Lower of the two,
and `volumesEnvelopeName`) runs SURFACE ENVELOPE of the same two the same way.
Until the review of 2026-10-15 SURFACE ENVELOPE was typed on the command line
only, with no window control, and docs/terrain.md said it had a dialog. Every
widget's object name is listed in `rust/crates/katana_window/src/terrain/volumes_dialog.rs`.

## Through katana_mcp

No tool of its own: VOLUME and SURFACE DIFFERENCE are session lines, so
`katana_run_commands` runs them as the window does, and the replies are the
records an agent reads (`docs/mcp.md`, "Volumes through katana_run_commands").
A tool would restate the verbs' options as a schema and add nothing the
records do not carry.

## Tests

- Terrain (`rust/crates/katana_terrain/tests/overlay.rs`,
  `rust/crates/katana_terrain/tests/volume_methods.rs`,
  `rust/crates/katana_terrain/tests/section_volume.rs`): the region; the overlay's pieces;
  the difference and envelope surfaces against crossing planes and against
  `compare_surfaces`; two parallel planes 1 m apart over a 10 x 10 polygon,
  100 m3; a tilted plane against a level inside a square; a prism under a
  non-convex polygon; the stockpile pyramid on level and sloping ground and
  against each base; the grid's 20 c^2; the wedge; parallel planes along a
  curved alignment; sections on a surface's boundary on every cardinal
  bearing and at every fifteenth degree, and at survey coordinates; sections
  that cross, on a curve, a corner and two legs, and the band that clears
  them; the first moments of a strip and of a section; the curvature
  correction on a quarter circle inside, outside, symmetric, turning right,
  along spirals and on a sloping design, against the annulus and the exact
  integral; two legs side by side, touching and a hair over; every refusal; and
  the same bits at any thread count.
- The tagged rings (`rust/crates/katana_entity/tests/ring_tags.rs`): what makes a hole, the
  part, and which exterior a hole joins - held by all of it, the part's, the
  smallest, the nearest before.
- The verbs (`rust/crates/katana_cad/tests/terrain_utility_verbs/volume_verbs.rs`, the `VolumeVerbs`
  suite): every form through `CommandInterpreter::run`, EACH per polygon with
  an open line skipped, an ellipse and a polyline drawn back to its start as
  polygons, a lot with a tagged hole by every method that takes a polygon, a
  spline drawn back to its start, overlapping polygons, the factors, the CSV,
  the curvature correction and its words, two legs side by side, the
  difference as one undo step, the envelope, the refusals.
- The polygons' reader (`rust/crates/katana_cad/tests/terrainverbs/drawing_shapes.rs`, the
  `DrawingShapes` suites, shared with the surface edits): what each kind of
  entity gives and why one gives none, a closed spline, typed corners with and
  without heights, the words, a scope that takes nothing, and the tagged rings
  joined to their polygons and left alone.
- The window (`rust/crates/katana_window/tests/surfaces/volumes_dialog.rs`): each
  page's line by object name, the curvature box included; Measure with the
  interpreter as the executor, filling the table and the mass haul diagram;
  Make Cut/Fill Map; Make Envelope; Save CSV.
  And the real window, headless (`tests/geo/headless/volumes.cmake`):
  `qt_volumes_dialog_measures_inside_typed_corners_and_makes_the_cut_fill_map_headless`
  draws the two surfaces, opens Terrain > Volumes, measures inside typed
  corners (16 m3), makes the cut/fill map and makes the envelope.
- `katana_cli` (`tests/cli/geo/volumes.cmake`):
  `cli.volume_between_two_surfaces_is_the_slab_whole_and_inside_a_square`,
  `cli.volume_to_a_level_reads_what_lies_below_it_as_fill`,
  `cli.volume_sections_along_an_alignment_give_the_end_area_total`,
  `cli.volume_sections_northwards_over_the_slab_are_whole_at_its_edges`,
  `cli.volume_sections_round_a_curve_give_the_band_when_the_reach_is_short_of_its_radius`,
  `cli.volume_sections_round_a_curve_with_the_default_reach_are_refused_not_counted_twice`,
  `cli.volume_sections_round_a_curve_are_corrected_for_a_strip_on_its_inside`,
  `cli.volume_sections_round_a_curve_with_curvature_off_give_the_plain_end_area`,
  `cli.volume_sections_along_two_legs_side_by_side_are_refused_naming_the_stretches`,
  `cli.volume_inside_a_lot_with_a_tagged_hole_measures_the_lot_less_the_hole`,
  `cli.volume_grid_over_lots_sharing_an_edge_counts_the_cells_on_it_once`,
  `cli.volume_datum_of_a_flat_surface_at_its_own_level_has_no_cut_area_or_fill_area`,
  `cli.volume_stockpile_total_names_the_level_it_was_measured_against`,
  `cli.surface_difference_keeps_the_cut_fill_map_as_a_surface`,
  `cli.volume_of_a_surface_that_is_not_there_is_refused`.
- `katana_mcp`: `mcp_server::a_volume_between_two_surfaces_is_measured_through_the_command_tool`,
  `mcp_server::sections_round_a_curve_say_whether_they_were_corrected_for_curvature_and_by_how_much`.
- Benchmarks: the C++ build's `bench_volumes` timed the refusal tests on a long
  run and the difference surface's join (see "Not done"); it went with the
  C++ tree and has no Rust twin yet.

## Decided

- **The overlay is the pair clipping, exposed - not the global constrained
  triangulation.** The earlier global overlay was 5.7 times slower
  (`docs/terrain.md`, "Where the time actually goes"), and the pieces a pair
  clip gives are the partition it built. The region is a third cut of the
  same clipper rather than a polygon boolean: the geometry layer has no
  polygon booleans (`rust/crates/katana_geometry/src/polygon.rs`), and a triangle is
  convex, so the clipper already does it.
- **The difference surface joins the pieces directly, with the constrained
  triangulation as the fallback.** Re-triangulating every time would be
  exact too, but it is the cost the overlay was rid of; joining by name is
  linear in the pieces. Rejected: welding corners by distance, which merges
  two crossings a hair apart into one and tears the surface somewhere else.
- **The section areas are cut by segment clipping, not by `extract_section`.**
  The section view's sampler (`rust/crates/katana_cad/src/section.rs`) finds a surface's breaks by
  bisection to `GEOMETRIC` between interval samples: right for a drawing,
  but an area integrated from it is exact only to the bisection, and it is
  in `cad`, where the terrain layer cannot reach. The clipper is closed form.
- **Sections that cross are refused, not cut short at the crossing.** Cutting
  each section off at its nearest crossing would give a number, but a silent
  one: the width measured is not the width asked for, and the ground beyond
  the crossing on a curve's inside belongs to no panel. A refusal that names
  the reach at which none cross costs a person one field retyped. Rejected too:
  refusing the default reach on any alignment that has a curve, which would
  also refuse an almost straight one whose sections cross nowhere within
  reach; the test is the crossing itself, so it follows the geometry.
- **A section on a boundary is on the surface within `GEOMETRIC`.** The one
  tolerance of the policy (`rust/crates/katana_math/src/numerics.rs`) is used, not a
  second one for sections; 0.1 micrometre is four orders above the rounding
  of a bearing at survey coordinates (9e-15 m at a 141 m reach, 1e-9 at the
  ulp of 6 000 000) and far below any ground that could be measured.
- **Average end area is the default method.** It is the method measurement
  standards and road contracts pay on (`docs/cad.md`, "Corridor
  quantities"), so it is what a quantity from here is compared with;
  prismoidal and both are a word away.
- **The mass ordinate takes cut as positive**, the mass diagram's convention,
  although `net=` takes fill as positive as every comparison here does: each
  is the convention of the document it is read against.
- **Overlapping polygons are refused for one total**, not unioned. A union
  needs polygon booleans this codebase does not have, and summing silently
  would count ground twice; EACH says each one's and warns of the overlap.
- **Numbers to the millimetre.** A cubic millimetre is far below any survey
  that made the surfaces; the full double in a reply would be noise, as the
  surface records found (`docs/terrain.md`, "One surface record"). The CSV
  carries the same numbers as the reply, so the two never disagree.
- **Volumes take no PREVIEW.** They change nothing; PREVIEW is for what does
  (SURFACE DIFFERENCE and ENVELOPE).
- **The curvature correction is on by default, and says by how much.** It is the
  standard method, and the plain figure is 4 % wrong for a 10 m strip at a
  radius of 50; a person on a curve would otherwise get the wrong quantity
  unless they knew to ask. Symmetric sections and straights do not move, so the
  change is confined to the asymmetric sections on a curve, which were the
  wrong ones. `curvature.cut=`/`curvature.fill=` carry the size of the
  difference so an agent can see it, and `curvature=off` gives the old figure.
  See "On a curve the volumes carry the curvature correction" for the
  alternatives rejected.
- **Stretches side by side are refused, not measured to the point where they
  overlap.** For the reason sections that cross are: the ground the two strips
  share belongs to neither, and a quantity cut at the overlap is a number that
  is not the one asked for. Rejected: testing the distance between the
  centrelines' parts - it is not the reach that sweeps the ground twice, the
  strips are, and a curve's own sides are close without sweeping anything twice.
- **A tagged hole joins by one rule, in the entity layer.** The GIS conversion
  had the rule and cad could not reach it, and a second copy in cad would have
  made the area of a lot depend on which verb asked. Off by default in the
  shared reader (`ShapeOptions::join_tagged_holes`) so the surface edits, whose
  polygons are rings, read what they always read; the volume verbs ask for it.

## Not done

- **A background job.** The verbs run on the thread that runs the line; on
  the window's that is the GUI thread, and a difference of two 200 000
  triangle surfaces takes a second or two. SURFACE FROM's job pattern is the
  way to move it, through the window's executor: one mechanism for every verb
  that is slow, and a second one for the volumes is not built.
- **The refusal tests on a very long run, measured and extrapolated.** Both the
  crossing test and the side-by-side test index boxes and test the pairs whose
  boxes overlap, on the task pool. On an alignment along an axis no boxes
  overlap; on a diagonal one each section pairs with every section within a
  reach of it. Measured on 2026-10-16 (Release, a 24-thread machine,
  `BM_VolumeSectionsAlongAnAxis` and `BM_VolumeSectionsAlongADiagonal`,
  the C++ build's `bench_volumes` benchmark, the whole VOLUME SECTIONS run): along an
  axis 1250 / 2500 / 5000 / 10 000 sections take 2.6 / 7.7 / 27 / 99 ms; along
  a diagonal 23 / 94 / 360 / 1566 ms. Both grow as n^2 (the fitted
  coefficients: 1.0 ns and 15.6 ns times n^2), so a diagonal alignment cut
  every 0.1 m over 10 km (100 000 sections, a 14 km default reach) would take
  about 156 s and an axis one about 10 s - extrapolated from the fit, not
  timed. At a road's usual 10 to 20 m interval it is a few hundred sections,
  well under a millisecond. The side-by-side test is a separating-axis test of up to four
  triangle pairs where the crossing test's is one cross product, so it is
  the dearer of the two per pair; a cheaper whole-strip reject before the four
  would be the first thing to try if the figure is large.
- **The difference surface's join, measured.** The overlay it is made of is
  (`compare_surfaces`, "One overlay" above); on two scatters of 100 000 points,
  about 200 000 triangles each, that cross (the C++ build's `bench_volumes` benchmark,
  2026-10-16, Release) the overlay takes 66 ms and the whole difference
  surface 1.45 s, of 1.87 million pieces: the join of the pieces into one
  surface is about 1.4 s, twenty times the measuring. It runs on the thread
  that runs the line, as the background-job item above says.
- **The corridor quantities do not take the curvature correction.** The road
  engine's end areas (`docs/road_design.md`, `katana_cad::road::road_section::road_section`) are sections
  cut by a template, not by a pair of surfaces, so they do not go through
  `volumes_along_alignment` and are not corrected; on a curve they have the same
  fixed factor of the centroid's offset over the radius for an asymmetric
  section. A corridor's `compare_surfaces` of its built surface against the
  ground is exact and has none of it.
- **A PI with no radius is not looked at by the correction.** The curvature
  there is 0 on each side and the ground in the wedge between the two legs'
  normals is whatever the sections the engine cuts there make of it (the
  crossing rule refuses a reach that sweeps the inside twice); the outside
  wedge has not been examined.
