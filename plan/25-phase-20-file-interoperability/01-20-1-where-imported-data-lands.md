<!-- Katana plan, section 25 of 47, subsection 20.1. Index: ../../PLAN.MD. Previous: 00-overview.md. Next: 02-20-2-12d-model-programme.md -->

## 20.1 Where imported data lands (delivered)

`katana::interop::advisePlacement` compares the incoming bounds with the
drawing's and says so when the two cannot usefully be seen together. This is the
survey case, not a corner case: a DXF in a projected CRS carries coordinates
like (255440, 7410850) and a drawing started from scratch sits near the origin,
so the merge succeeds and the original content becomes a dot smaller than a
pixel with nothing saying why.

The criterion is the SYMPTOM, not a distance: a part is lost when showing both
would shrink it below 1% of the combined diagonal. A threshold in metres would
be wrong for a site plan and wrong again for a national grid - the same 20 km
gap is far apart for two 100 m drawings (0.7% of the view) and not for two
50 km ones (54%). CORRECTED 2026-09-23 (audit IO-02): the loss must be the
placement's doing, so a part counts only if it would plainly be visible beside
the other with the gap closed (over 2% of the other's diagonal), and a part with
no extent never counts. The old rule reported correctly placed data that was
merely small - one control point, a 10 m detail inside a 2 km site - and the
default button moved it to the drawing's corner. A far-off single point still
reports the drawing it would hide.

`IMPORT <file> LOCAL` in the CLI shifts the data so its corner lands at the
ORIGIN (this said "meets the drawing's"; for rasters and point clouds, which
are reference data drawn at their own coordinates, LOCAL is refused by name -
it used to be left on the path and fail as a missing file, audit QT-13/QT-14,
fixed 2026-09-23), using the
`VectorImportOptions::originShift` that already existed and
that nothing had ever set. Verified on a 51 417-feature DXF at
(255440, 7410850): without LOCAL the warning fires and names the separation;
with it the data lands at 0,0 to 2272,1271 beside the existing plan.

The GUI asks BEFORE anything is added: Shift Alongside / Keep Survey
Coordinates / Cancel, with the file's extent shown. Shifting is the default,
because the drawing the user is already looking at is the one they want to keep
seeing.

**OUTSTANDING:** no CRS is read from the source and no reprojection is done, so
a shift is a translation, not a transformation - two files in different
projections still will not line up. Raster and point-cloud imports get no
equivalent check.

