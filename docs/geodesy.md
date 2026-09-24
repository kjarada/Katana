# Geodesy: coordinate systems, transformations, units and site calibration

`katana_geodesy` (`include/katana/geodesy/`, `src/katana_geodesy/`): the
coordinate reference systems a project's data is in, the one route by which a
coordinate moves between two of them, the grid and ground factors a surveyor
reduces distances with, geodesics on the ellipsoid, the unit catalogue, and
the four-parameter site calibration that ties a local grid to a projected one.

## Where it sits

Beside geometry: it sees `core` and `math` and nothing else, and `cad` sees it
(`docs/architecture.md`, "Layering"). PROJ is a PRIVATE dependency, confined to
`src/katana_geodesy/` behind Katana's own types (Rule 4): no PROJ type or
header appears under `include/katana/geodesy`, and the `layering` test checks
it. `entity` cannot see geodesy, which is why the unit ratios every layer
needs live one layer down in `math/unit_ratio.hpp` and `geodesy/units.hpp` is
the catalogue built on them - one definition of a foot, not two.

Users today: the Survey menu's coordinate converter (`cad::convertCoordinates`,
`include/katana/cad/survey_tools.hpp`), which reads source and target systems
as EPSG codes or anything PROJ accepts and reports the operation PROJ applied,
its declared accuracy, and the grid factors at each point on each projected
side; and the survey points' coordinate system (`declaredSystemFromEpsg`, and
`transformSurveyProject`, which moves a surveyed job between two EPSG systems;
`src/katana_cad/customisation/survey_points.cpp`).

| Header | What it holds |
|---|---|
| `coordinate.hpp` | the coordinate value types and the axis-order convention |
| `coordinate_reference_system.hpp` | `CoordinateReferenceSystem`, an immutable value; `projRuntimeInfo` |
| `coordinate_transformer.hpp` | `CoordinateTransformer`, the single entry point for transformations |
| `grid_factors.hpp` | `GridFactorCalculator`: point scale factor and grid convergence |
| `geodesic.hpp` | `geodesicInverse`, `geodesicDirect`, `gaussianMeanRadius`, `elevationFactor` |
| `ellipsoid.hpp` | `Ellipsoid` with the WGS 84 and GRS 80 defining constants |
| `units.hpp` | `LengthUnit`, `AngleUnit`, exact conversions and name parsing |
| `similarity_transform2d.hpp` | `SimilarityTransform2D` and `fitSimilarity2D`, the site calibration |

## Conventions

These are the pitfalls of the subject, each fixed once.

**Axis order.** Prefer the two NAMED types: `GeographicCoordinate{latitude,
longitude, height}` in decimal degrees, north and east positive, and
`ProjectedCoordinate{easting, northing, height}` in the CRS's own linear unit -
never silently converted to metres. A name cannot be transposed. The generic
`Coordinate{x, y, z, t}` is ALWAYS in traditional GIS order - x is longitude or
easting, y is latitude or northing - whatever order the authority declares
(EPSG:4326 is officially latitude first); PROJ gives that order through
`proj_normalize_for_visualization`, which reorders axes and never flips their
direction, so a westing/southing system keeps x as the westing. A named type
handed to a transformer whose CRS is of the other kind is `InvalidArgument`.

Other modules have their own orders: `survey::Coordinate2` is (northing,
easting), while the drawing's `Point2` and `SimilarityTransform2D`'s `Vec2`
are (x = easting, y = northing). The drawing and the survey model meet in two
places, the survey import (`src/katana_cad/survey_import.cpp`) and the parcel
report (`src/katana_cad/parcel.cpp`), and the swap is made there and nowhere
else.

**Angles.** Geodesic azimuths and `GridFactors` are DEGREES CLOCKWISE FROM
NORTH in [0, 360), the surveying convention, and every field name carries its
unit. `SimilarityTransform2D::rotation` is RADIANS COUNTER-CLOCKWISE in the
(easting, northing) plane, as the rest of Katana's geometry is, so a grid
bearing changes by minus the rotation. The survey module works in radians
clockwise from grid north. A geographic value in a named type is degrees even
for a CRS whose angular unit is the grad.

**Heights** travel in `height` or `z` in the unit of the CRS's vertical axis and
are transformed only when both systems are three-dimensional; between 2D
systems they pass through unchanged. The coordinate converter does not convert
heights.

**Epochs.** `Coordinate::t` is the coordinate epoch in decimal years;
`kUnspecifiedEpoch` (infinity, which PROJ reads as "none") means no epoch. A
literal 0 would be the year 0 and would drift a plate-motion transformation by
two millennia.

**Units.** Length units are exact rationals of a metre (international foot
381/1250, US survey foot 1200/3937), so a conversion is `(value * p) / q` with
exactly representable integers: at most two correctly rounded operations.
Angle units other than the radian are exact fractions of a turn (360 degrees =
400 gon = 1 296 000 arc-seconds), so converting between them never touches pi,
and the degree-radian factors are bit-identical to `math::kDegToRad` and
`kRadToDeg`. There is NO default unit: an unknown name is `ParseFailure`,
never a silent factor of 1. "foot" and "ft" always mean the international
foot, as EPSG (9002) and PROJ do; the US survey foot must be named ("ftUS",
"US survey foot"). They differ by 2 ppm - 4 ft at a state-plane coordinate of
2 000 000 ft - and `lengthUnitFromMetresPerUnit` compares with
`math::tolerance::kRelative`, 600 times finer than that, so the two are never
confused.

## Coordinate reference systems

`CoordinateReferenceSystem` is an immutable VALUE: the definition text handed
to PROJ plus what PROJ reported about it when it was made - name, kind,
authority and code, axes in the authority's order, horizontal unit, ellipsoid,
area of use, and the canonical WKT2:2019 text. It holds no PROJ handle, so it
is cheap to copy and safe to share between threads. The factories are
`fromEpsg`, `fromWkt` (WKT1 in either flavour, or WKT2), `fromProjString`
(which appends `+type=crs` so the text is read as a system, not a bare
operation), `fromUserInput` (anything PROJ accepts, `"EPSG:4326+3855"`
included) and `localEngineering(name, unit)`, a site grid with no datum that
needs no database and that PROJ cannot relate to anything - a site calibration
does that. Each factory validates with PROJ and fails with `InvalidCRS`,
carrying PROJ's message in the context; there is no "unknown CRS" state.

Two notions of sameness: `operator==` compares the definition text (exact and
cheap, so `"EPSG:32630"` and the equivalent PROJ string differ), and
`isEquivalentTo` asks PROJ whether datum, projection and axes agree, names
aside, optionally ignoring axis order. `areaOfUse()` matters because PROJ
evaluates a projection wherever its formulas are defined, far outside the zone
it was designed for; `GeographicExtent::contains` is the test a caller that
must reject such input uses.

## Transformations

`CoordinateTransformer::create(source, target, options)` asks PROJ for every
candidate operation between the two systems, ranked by area of use, accuracy
and grid availability, and PINS the first that can run on this installation:
every point is transformed by the same operation, which `operation()`
describes (name, pipeline, declared accuracy, grids). `candidates()` lists all
of them, and `missingGrids()` names the grid files that would unlock a more
accurate one.

- **Ballpark operations are refused.** Where PROJ knows no datum
  transformation it can fall back to one that applies NO datum shift, wrong by
  up to hundreds of metres. Surveying cannot take that silently, so `create`
  fails with `Unsupported` unless `TransformerOptions::allowBallpark` is set.
- **An area of interest** lets PROJ rank first the operation valid where the
  data is, rather than the one with the widest coverage.
- **A required accuracy** rejects candidates declared worse than it.
- **Time-dependent operations demand an epoch.** A coordinate without one is
  refused rather than transformed at the operation's reference epoch, which
  would be wrong by the plate motion since (about 2.5 cm a year).
- **The network is off.** Each PROJ context is created with downloading
  disabled, so a result cannot depend on what happened to be reachable
  (Rule 7).
- **Batches are one PROJ call** (`proj_trans_generic`) and bit-identical to the
  per-point calls. A point PROJ cannot transform is not an error of the batch:
  its coordinates become NaN, so it can never pass for a position, and it is
  counted in the `BatchReport`, which a caller must read.

## Grid and ground

```
grid distance = ellipsoidal distance x point scale factor
grid bearing  = geodetic azimuth - convergence        (+ arc-to-chord on long lines)
ground to grid: combined factor = point scale factor x elevation factor
elevation factor = R / (R + h),   R = gaussianMeanRadius(latitude) = sqrt(M N)
```

`GridFactorCalculator` gives, at a geographic or a grid position of a
projected system, the meridional and parallel scales, the areal scale, the
angular distortion and the grid convergence, from PROJ's `proj_factors`, which
differentiates the projection numerically. For the conformal projections every
survey grid uses (Transverse Mercator, Lambert Conformal Conic, stereographic,
oblique Mercator) the point scale factor is the same in every direction;
`pointScaleFactor()` returns the meridional scale h rather than the parallel
scale k, because k divides by cos(latitude) and degrades towards the poles.
Convergence is documented as the azimuth of grid north, positive east of the
central meridian in the northern hemisphere, about (lon - lon0) sin(lat).

The Gaussian mean radius is `a sqrt(1 - e^2) / w^2` with
`w^2 = 1 - e^2 sin^2(phi)`, which is `sqrt(M N)` with the meridional radius
`M = a (1 - e^2) / w^3` and the prime-vertical radius `N = a / w`.

**What is not established.** The header's figure for `proj_factors`, "relative
accuracy around 1e-9", has no measurement behind it, and the convergence SIGN
has never been checked: if PROJ's convention differed from the documented
one, every grid bearing would be rotated by twice the convergence - about
2 x 1.4 degrees at 3 degrees from a UTM central meridian at latitude 30. The
oracles to check both exist in `tests/geodesy/geodesy_test_support.hpp`
(`krugerFactors`, from the Krueger series differentiated along the meridian)
and nothing calls them (audit SUR-01, OPEN).

## Geodesics

`geodesicInverse` and `geodesicDirect` use the geodesic routines that ship
with PROJ (C. F. F. Karney, "Algorithms for geodesics", J. Geodesy 87, 2013):
accurate to about 15 nanometres and convergent for every pair of points,
including the nearly antipodal ones where Vincenty's iteration fails. Heights
are ignored - a geodesic lies ON the ellipsoid - and azimuths are wrapped from
the library's (-180, 180] into [0, 360), a tiny negative value that would round
up to 360 becoming 0. For coincident points the distance is 0 and the azimuths
carry no information. The functions are stateless and thread-safe. They are
untested (SUR-01): `quarterMeridian` in the test support is the oracle for the
equator-to-pole distance, unused.

## Site calibration: the 2D similarity

A local engineering grid is tied to a projected system by a four-parameter
conformal (Helmert) transformation fitted to control points seen in both:

```
X = tx + a x - b y          a = s cos(theta)
Y = ty + b x + a y          b = s sin(theta)
```

with scale s and rotation theta counter-clockwise. Reflections are not
modelled: control with swapped axes shows up as residuals the size of the
site, and a mirrored set is refused.

**The fit is closed form.** Centre both point sets on their centroids:
`x_i = source_i - sourceOrigin`, `X_i = target_i - targetOrigin`, so that the
sums of x_i and of X_i are zero. The least-squares problem minimises
`sum |X_i - M x_i|^2` with `M = [a -b; b a]` plus a translation. Setting the
derivative by the translation to zero puts it at the centroids exactly, because
the centred sums vanish; that is why the transformation is STORED about the two
centroids rather than as (tx, ty). The derivatives by a and b give

```
sum (x X + y Y) = a sum (x^2 + y^2)      a = sum (x . X) / S
sum (x Y - y X) = b sum (x^2 + y^2)      b = sum (x cross X) / S,   S = sum |x_i|^2
```

The cross terms `sum(-x y + y x)` cancel, so the 2x2 normal matrix is
`diag(S, S)`: there is nothing to invert and no conditioning problem, which is
why a closed form is used rather than a general least-squares solver.

**Why the origins.** At UTM magnitudes (1e6 to 1e7 m) a coordinate carries
about 1e-9 m of rounding. `apply()` forms `source - sourceOrigin` first - for
points on the same site that difference is nearly exact - and only then scales
and rotates the small site-sized vector and adds the target origin, so the
result is accurate to about one unit in the last place of the target
coordinate. Multiplying the full coordinate by M and adding (tx, ty) would
lose that; `translation()` is offered for display and export and absorbs the
rounding of `M * sourceOrigin`. `fromCoefficients` rebuilds a stored
transformation exactly from a, b and the two origins, since storing scale,
rotation and translation instead would not round-trip bit for bit. The
centroids themselves are accumulated relative to the first point, so the sums
hold site-sized numbers, and what rounding leaves of their mean is removed and
folded back into the target origin.

**What the fit reports.** The residual of each pair (target minus the
transformed source, in target units), their RMS and largest magnitude, and,
with more than two pairs, the a-posteriori standard deviation of one
coordinate `sqrt(sum |v|^2 / (2n - 4))`: 2n observations, 4 parameters. Two
pairs fit exactly and leave nothing to judge the control by, so that figure is
absent rather than zero. Refused, as `InvalidArgument`: fewer than two pairs;
a non-finite coordinate; source or target points that all coincide within
`math::tolerance::kCoordinate`, which leaves scale and rotation undetermined;
and control whose best scale collapses to zero, typically a mirrored set.

The fit and the transformation are untested (SUR-01): the suggested tests are
exact synthetic pairs at (500000, 5000000) that must give back s and theta, two
pairs giving no standard deviation, and a mirrored set refused.

## Threading model

- `CoordinateReferenceSystem`, the coordinate types, `Ellipsoid` and the unit
  functions are values or pure functions: safe to share and call from any
  thread.
- `CoordinateTransformer` and `GridFactorCalculator` each own a private PROJ
  context. A PROJ context may be used by one thread at a time, so these
  objects are NOT thread-safe, not even for concurrent calls on different data
  - which is why their transform and `at` functions are non-const. Use one per
  thread; `clone()` makes an independent, identically configured one. Distinct
  objects never share state. `OneTransformerPerThread` runs four clones on the
  same points at once and requires the reference result bit for bit.
- The geodesic functions are stateless and thread-safe.
- Every CRS factory, `toWkt` for a flavour other than the cached WKT2, and
  `isEquivalentTo` create a fresh PROJ context and open `proj.db`: correct
  from any thread, and too slow for an inner loop. Make the systems once.

## The PROJ installation

PROJ reads its database, `proj.db`, and any grid files from its data
directory. The build and the bundle put it at `<its DLL>/../share/proj`, where
PROJ finds it by itself (`docs/building.md`). `projRuntimeInfo()` reports the
PROJ version, the database it resolved and its search paths, and fails with
`NotFound` when the database cannot be opened - as does every CRS factory
except `localEngineering`, and every transformer.

## Tests

`tests/geodesy/` (`katana_geodesy_tests`): the CRS values, the transformer
and the units. Every assertion on a coordinate is tagged with where its
expected value comes from: `[identity]` (an exact property of the CRS
definition), `[oracle]` (a formula coded in `geodesy_test_support.hpp`, which
never calls PROJ for a number it then asserts), `[published]` (an authority's
worked example, quoted to its printed precision) or `[self]` (PROJ's
self-consistency only, which says nothing about absolute truth). The
published examples are the Ordnance Survey's National Grid worked example and
IOGP Guidance Note 7-2's Lambert Conic Conformal (one and two parallels, in US
survey feet), Oblique Stereographic and geographic-to-geocentric examples;
the oracle is Transverse Mercator by the Krueger series to sixth order
(Karney 2011), checked across a UTM zone. Tolerances are built from
`math::tolerance::kCoordinate` expressed as an angle with the longest degree
on the ellipsoid, so a bound never understates a ground distance.

Not tested: grid factors, geodesics and the site calibration (SUR-01, above).

## Failure modes

| Condition | Behaviour |
|---|---|
| Text that is not a CRS | `InvalidCRS` with PROJ's message |
| `proj.db` not found | `NotFound` from `projRuntimeInfo`, every factory but `localEngineering`, and `create` |
| No operation, only a ballpark one, or only ones needing missing grids | `Unsupported`, the grids named in the context |
| A local engineering system in a transformation | `Unsupported`: relate it with a site calibration |
| A time-dependent operation and a coordinate without an epoch | refused |
| A point outside the projection's domain, beyond a pole, or non-finite | `InvalidArgument` for one point; NaN and counted in a batch |
| A named type of the wrong kind | `InvalidArgument` |
| An unknown unit name | `ParseFailure`, never a factor of 1 |
| Grid factors outside the projection's domain | `InvalidArgument` naming PROJ's reason |
| Too few, coincident or mirrored control points | `InvalidArgument` |
