# GIS > Online Data - imagery, elevation and features from web services

Importing raster and vector data from public web services, Australian and
global, into the project's coordinate system: the catalogue of providers, how
each kind of service is asked, the limits, the cache, the record every import
keeps of its licence and attribution, the dialog and the `ONLINE` verbs.

## Purpose

A survey or civil drawing is started on top of what is already published: the
cadastre, aerial imagery, a DEM, roads and buildings. Before this, each had to
be downloaded by hand, reprojected in another program and imported as a file.
GIS > Online Data does it in one step, from inside the drawing's area and in
its CRS, and an agent can do everything a person can through the `ONLINE`
verbs - the dialog has no path of its own.

## Where it lives

The layering is `docs/interop.md`'s: GDAL stays in `katana_io` (Rule 4),
the domain conversion in `katana_interop`, the window in `katana_qt`.

| File | What |
|---|---|
| `include/katana/gis/web_access.hpp` | `gis::httpFetch` over GDAL's libcurl (`CPLHTTPFetchEx`), `gis::redactUrl`, `gis::percentEncode`, `gis::sha256Hex`, `gis::parseXml` |
| `include/katana/gis/reproject.hpp` | `gis::crsAxisIsYX`, `gis::transformBox`, `gis::reprojectFeatures`, `gis::warpToGeoTiff`, `gis::buildTrueColourVrt`, `gis::clipVectorFile`, `gis::probeRaster` |
| `include/katana/interop/online_catalogue.hpp` | the catalogue: types, parsing, merging, validation |
| `include/katana/interop/online_requests.hpp` | pure request builders for every service type |
| `include/katana/interop/online_discovery.hpp` | what a custom URL offers, from its capabilities or description |
| `include/katana/interop/online_fetch.hpp` | `interop::fetchOnlineLayer`, the cache, discovery and catalogue search over the network |
| `include/katana/interop/online_verbs.hpp` | the verbs' grammar and replies |
| `resources/online/online_sources.json` | the built-in catalogue, compiled in with `#embed` |
| `src/katana_qt/gis_online.hpp` | the workbench: the menu section, the one executor, jobs, settings |
| `src/katana_qt/gis_online_dialog.hpp` | the dialog |

`importVector` gained three options for this and nothing else:
`VectorImportOptions::targetCrs`, `VectorImportOptions::assumedSourceCrs` and
`VectorImportOptions::sourceFilter`. A file import leaves them empty and still
never reprojects (`docs/interop.md`, "No reprojection"): web data is different
because nobody chose the service's CRS, and a Web Mercator tile on an MGA
drawing is useless until it is moved. A reference raster records
`RasterOverlay::sourceUrl`, `RasterOverlay::licence` and
`RasterOverlay::attribution`.

## The catalogue

Providers hold services, services hold layers. A service's CRS, type,
endpoint and limits are inherited by its layers, a provider's licence,
attribution and coverage by its services, and a level overrides what it
needs; `interop::parseCatalogue` resolves it all, so every `OnlineLayer`
carries its whole description. The fields: `id`, `title`, `kind` (imagery,
elevation, vector, catalogue), `type` (below), `endpoint` (a base URL, or a
template with `{z}` `{x}` `{y}` `{layer}` `{time}` `{lat}` `{lon}` `{key}`),
`layer` (the service's own layer id, name, type name, collection or tag),
`crs`, `coverage` (WGS 84 `[west, south, east, north]`),
`maxRequestPixels`, `pageSize`, `maxZoom`, `maxTiles`, `resolution` (metres),
`cacheDays`, `maxArea` (km2), `timeout`, `maxCloud`, `days`, `asset`,
`idField`, `time`, `tiling`, `licence`, `attribution`, `keyRequired`,
`keyName`, `verified` (the date it answered Katana's own requests), and
`evidence` (where the address and terms were checked).

A person's own catalogue - `online_sources.json` in Katana's configuration
folder, or the file `KATANA_ONLINE_CATALOGUE` names - is merged over the
built-in one by provider id (`interop::mergeCatalogues`): a provider with a
built-in id replaces it in place, a new one is added. Add Custom Service and
`ONLINE CUSTOM` write there, through a temporary file and a rename, and not
at all when the existing file cannot be read (it would be replaced by the one
new provider). A discovered provider's id is `custom-`, the host, and eight
hex digits of the address's SHA-256, so two services of one server are two
providers. A discovered address's service keeps its vendor parameters
(MapServer's `map=`) and loses only the OGC request's own. `interop::validateCatalogue` names every problem
of a catalogue, and `OnlineCatalogue.TheBuiltInCatalogueParsesAndPassesItsOwnValidation`
runs it on the built-in one.

### Providers and their state on 2026-09-25

This was built in a cloud environment whose network policy refused every
host but Amazon S3's (the egress proxy answered 403 to `CONNECT`). So the
services on S3 were seen to answer from Katana's own code, and every other
entry was taken from the publisher's own service directory or documentation
and carries that in `evidence`; its `verified` is empty until it answers. The
`OnlineLive.EveryBuiltInEndpointAnswers` test (below) is how an owner with a
network marks them.

| Provider | Group | Layers | Service | State |
|---|---|---|---|---|
| NSW Spatial Services | Australia/NSW | imagery, base map, topographic map (tiles); cadastre lots, suburbs, LGAs, places, Roads (ArcGIS query); 5 m DEM (ImageServer) | xyz, arcgis-query, arcgis-exportimage | documented; CC BY 4.0 |
| Queensland Spatial | Australia/QLD | latest state imagery (tiles), cadastral parcels | xyz, arcgis-query | documented; CC BY 4.0 |
| Vicmap open data | Australia/VIC | property parcels | wfs | documented; the type name is the least certain entry |
| Location SA | Australia/SA | topographic and street maps | arcgis-export | documented |
| Land Tasmania (theLIST) | Australia/TAS | cadastral parcels, orthophoto, topographic | arcgis-query, arcgis-export | documented; CC BY 3.0 AU |
| ACTmapi | Australia/ACT | 2016 imagery in MGA 55 | arcgis-export | documented |
| Geoscience Australia | Australia/National | 1 second DEM-H (WCS), topographic and national base maps | wcs, arcgis-export | documented |
| Digital Earth Australia | Australia/National | GeoMAD, fractional cover, intertidal | wms | documented |
| data.gov.au | Australia/National | a search for WMS, WFS, WMTS, WCS and ArcGIS services | ckan | documented |
| OpenStreetMap | Global | buildings, roads, waterways, land use, any tag (Overpass); standard tiles | overpass, xyz | documented; ODbL |
| Copernicus DEM | Global | GLO-30 | cog (one-degree tiles) | **answered**, imported live |
| Sentinel-2 L2A | Global | true colour (TCI), true colour from bands | stac (Earth Search) | the search is documented; the COGs **answered** and were imported live through an item |
| NASA GIBS | Global | MODIS Terra and VIIRS true colour | wmts | documented |
| Natural Earth | Global | countries, states, places, coastline, rivers | file | **answered**, imported live |

Left out, because their terms forbid use by a third party's application or
could not be established: Vicmap Basemaps (a licensed service with a fee),
Landgate SLIP's cadastre (a personal-use licence), the South Australian
government imagery mosaic (for its stakeholders only), the TASMAP rasters
(CC BY-NC-ND) and the Northern Territory services found (the licence version
is not stated). No Northern Territory provider is listed for that reason.

## How each service is asked

Every request is built by a pure function of `online_requests.hpp`, tested byte
for byte against the service's specification (`tests/interop/test_online_requests.cpp`).

| Type | Request | Paging or tiling | Answer |
|---|---|---|---|
| `xyz` | the template, `{z}/{x}/{y}` on the Web Mercator grid | the zoom whose pixel is at least as fine as asked; tiles covering the area, at most `maxTiles`, falling back up to three zooms coarser (with a warning) and refusing beyond that | tiles, each placed by its index |
| `wmts` | a REST template or the KVP GetTile, on a GoogleMapsCompatible matrix set | as `xyz` | tiles |
| `arcgis-export` | MapServer `/export?bbox&bboxSR&imageSR&size&format=png32&transparent=true&f=image` | the area split into requests of at most `maxRequestPixels` a side, at most 64 | PNGs placed by their boxes |
| `arcgis-exportimage` | ImageServer `/exportImage`, Float32 GeoTIFF for elevation, PNG for imagery | as export | values |
| `wms` | GetMap 1.3.0 (`CRS=`, the authority's axis order) or 1.1.1 (`SRS=`, x,y) | as export | images |
| `wcs` | GetCoverage 1.0.0, GeoTIFF, `BBOX` x,y | as export | values |
| `arcgis-query` | `/<layer>/query` with an envelope in WGS 84, `outFields=*`, `outSR=4326`, `f=geojson` | `resultOffset` advanced by what each page held, until the page is short and `exceededTransferLimit` is false | GeoJSON pages |
| `wfs` | GetFeature 2.0.0 (`TYPENAMES`, `COUNT`, `STARTINDEX`, `BBOX` in the CRS's axis order with the CRS appended - as its URN when latitude comes first, which every server reads in that order) or 1.1.0 (`MAXFEATURES`, one page, with a warning when it came back full) | `STARTINDEX` until a page comes back empty, since a server may cap pages below `COUNT` | GML or GeoJSON |
| `oapif` | `/collections/{id}/items?bbox&limit&f=json`, the bbox in CRS84 | the answer's `next` link, followed only to the same host | GeoJSON pages |
| `stac` | an Item Search POST (`collections`, `bbox`, `datetime`, `eo:cloud_cover` through the query extension, sorted by cloud) - or, for an item's own URL, the item | the acquisition day that covers the area (tested on a 5 x 5 grid) with the least cloud, the latest on a tie | COGs read over `/vsicurl/`: the `visual` asset, or red, green and blue stretched 0..3000 to bytes |
| `cog` | `/vsicurl/` of the COG, or of each Copernicus one-degree tile meeting the area (a missing tile, over the sea, is skipped) | GDAL's range requests | values or imagery |
| `overpass` | a POST of `[out:xml][timeout][maxsize][bbox]; (node[..]; way[..]; relation[..];); (._;>;); out body;` | one request; the area is refused over `maxArea` (25 km2) | OSM XML, read by GDAL's OSM driver |
| `file` | a download (a zipped shapefile is read as `.shp.zip`) | clipped to the area by `gis::clipVectorFile` before it is read | features |
| `ckan` | `package_search` with `res_format` of the web-service formats | - | services to add with `ONLINE CUSTOM` |

**A STAC asset's address is resolved, not trusted.** An absolute http(s) or
s3 href is read over `/vsicurl/`; a relative one is resolved against the
item's own address; a `file://` href is honoured only in an item that was
itself read from a file, so a service's answer cannot make Katana open a file
on the computer it runs on.

**The tag filter cannot escape its statement.** An Overpass filter is
`key`, `key=value` or `key!=value`, joined by `;`, of letters, digits and
`_ : - .` only; a quote, bracket, brace or backslash is refused
(`OnlineOverpass.NothingTypedCanBecomeASecondStatement`).

## Coordinates, axis order and resolution

The area is taken in the project's CRS (view, drawing extents, selection with
a margin of a tenth and at least 10 m, or a typed box) or in WGS 84 degrees
(`lonlat:`), and moved to WGS 84 and to the service's CRS with
`gis::transformBox`, which densifies the edges so the curved image of the box
is enclosed. Every coordinate in the program is in traditional GIS order;
the two protocols that follow the authority's axis order, WMS 1.3.0 and WFS
2.0, ask `gis::crsAxisIsYX` and swap in the request builder, and nowhere else
(`OnlineRequests.Wms130WritesAGeographicBoxLatitudeFirst`,
`OnlineRequests.Wfs20PagesAndFollowsTheAuthoritysAxisOrder`).

A resolution is in project units per pixel. It becomes the service's by the
**local scale** at the area's centre - two short steps east and north
measured in both CRSs. The ratio of the two boxes' areas was used first and
chose a zoom one level too fine: a box in MGA is rotated against Web Mercator
by the grid convergence, so its bounding box is a few per cent larger than the
area. The default is the layer's own resolution for elevation (5 m for NSW,
30 m for Copernicus) and the area's longer side over 2048 pixels for imagery.

Rasters are warped by `gis::warpToGeoTiff` into one GeoTIFF on the project's
grid - cubic and RGBA for imagery (whatever the source was: a paletted PNG, a
grey JPEG, an RGB COG), bilinear Float32 with no-data -32767 for elevation -
and read by the existing `importRaster`. Surface From Raster reads that file's
true values, so an online DEM becomes a surface like any other. Vectors are
read page by page by the existing `importVector`, moved into the project's
CRS on the way (`VectorImportOptions::targetCrs`), with a GeoJSON that
declares no CRS taken as WGS 84 (RFC 7946).

## Limits

Every limit is refused before the work it would stop, with what to do instead;
nothing is truncated silently.

| Limit | Value | Refusal says |
|---|---|---|
| a finished raster | 64 M pixels (`OnlineEnvironment::maxPixels`) | the size, and the `res=` that fits |
| image requests for one import | 64 | the resolution that fits |
| tiles for one import | the layer's `maxTiles` (64 for OpenStreetMap's tiles, 256 for GIBS, 400 for the state imagery) | choose a smaller area or a coarser resolution |
| one answer | 256 MB (`OnlineEnvironment::maxResponseBytes`), enforced as the bytes arrive | ask for a smaller area |
| features in one import | 250 000, pages 500 | the count; choose a smaller area |
| an Overpass area | the layer's `maxArea`, 25 km2 | the area in km2 and the limit |
| an XML document | 64 MB | - |
| a request | 120 s, 20 s to connect, abandoned below 16 bytes/s for 30 s | how long it waited |

## Paging and joining

Pages are joined by `PageJoiner` in `online_fetch.cpp`: a feature a later
page repeats is dropped, known by the layer's `idField` (for an ArcGIS layer
its own `objectIdField`, found by discovery, else `objectid`; matched without
regard to case, and sent as `orderByFields` so pages cannot overlap) or by
its attributes AND its geometry's type and box - attributes alone took two
buildings with the same tags for one; within one page nothing is dropped, because a multi-part feature
arrives as several entities with the same attributes on purpose. A server
that ignores `resultOffset` repeats its first page; a page that adds nothing
new ends the loop with a warning, rather than running to the page limit
(`OnlineFetch.AServerThatIgnoresPagingEndsWithAWarningNotALoop`). The offset
advances by what each page actually held, not by the page size, so a server
that returns fewer than asked loses nothing
(`OnlineFetch.ArcgisPagesAreJoinedWithoutLossOrRepeatAndMovedIntoTheProjectCrs`).

## Caching

Every answer is kept in the cache folder (Katana's cache location, `online`;
`KATANA_ONLINE_CACHE` overrides it), under `http/`, named by the SHA-256 of
the method, URL and body, so no two requests share a file and no key appears
in a file name. It is written to a temporary name and renamed, so a cancelled
transfer never leaves a partial answer that a later run would take for a
whole one; an answer that is an error document (an ArcGIS `{"error"}`, a WMS
ServiceException) is removed rather than cached. An answer is fresh for the
layer's `cacheDays` - 30 by default, 7 for OpenStreetMap's tiles as its
policy asks, a day for a STAC search, whose answer changes daily. Finished
rasters are kept under `products/`, keyed by everything that changes the
output (the service, its CRS, version and limits, the layer, the area in the
project's CRS, the CRS, the resolution, the dates, the cloud ceiling, the
time), dated afresh each time one is used, so the same import a
second time costs no request at all (`OnlineFetch.TilesAreFetchedMosaickedAndWarpedAndCachedForTheNextTime`).
The cache is pruned when an import starts, to 90 days and 2 GB, oldest first
(`interop::pruneCache`). Descriptions for discovery are always fetched afresh,
and an answer that turns out not to be what was asked for - an error page
where a tile, a zip, a GML page or a STAC search was expected - is removed,
never kept for the next run to trip on.

## Licences, attribution and keys

Every import records where it came from and on what terms. A raster:
`RasterOverlay::sourceUrl`, `licence`, `attribution`. Every entity: the
metadata `online.source`, `online.licence`, `online.attribution` and
`online.retrieved` (the UTC date), which are saved with the project. The
reply to the import (below) carries the same, so the command log shows the
attribution of everything imported.

The URL recorded is the service's address with any key removed
(`gis::redactUrl`: `key`, `apikey`, `api`, `token`, `access_token`, `sig`,
`auth`, `subscription-key` and any parameter ending in `key` or `token`, and
`user:password@`). The command log shows a typed `ONLINE KEY` with its value
as `***` (`OnlineDataWorkbench::loggedLine`). A key is stored
by `ONLINE KEY <name> <value>` or the dialog's Save Key in Katana's settings
(`online/keys/<name>`), is filled into an endpoint's `{key}` when a request is
made - a key found in a discovered address (a WMTS template's `?api=`) is
moved to the settings and the address keeps `{key}` (`interop::extractKey`) - and is never written to the project, a log line, an error, a reply or
a cache file name (`OnlineFetch.AKeyIsSentButNeverRecorded`). No built-in
provider needs one; the mechanism is for a user catalogue's commercial
services.

Every request says what it is: `Katana/<version> (survey and civil-engineering
CAD; +https://github.com/kjarada/Katana)`, as the OpenStreetMap tile and
Overpass policies require.

## Threading, cancellation, retries

`interop::fetchOnlineLayer` blocks its thread and touches no Document. The
workbench runs it as a background job (`src/katana_qt/jobs.hpp`) on inputs
copied on the GUI thread; the job's Apply runs on the GUI thread and adds the
entities, with the layers they need, as ONE command - one Ctrl+Z removes an
import - or the raster to the reference data (which is not undoable, by
design: `docs/interop.md`, "Two kinds of imported data"). Cancel in the status
bar stops the transfer between packets (GDAL's progress callback) and the warp
between chunks; nothing is applied. A headless run waits for the job with a
deadline, `timeout=` (600 s by default), after which it is cancelled. In a
person's session `timeout=` is a deadline too when it is given - the dialog's
Give up after - and the job is cancelled then as Cancel would; without it a
person cancels when they choose. Until 2026-09-26 `timeout=` was read only by
a headless run, and typed in the window it did nothing.

A request is retried three times, the delay doubling from a second, for a 429,
a 5xx, a timeout or a dropped connection - never for another 4xx, which would
fail the same way - and a stop during the wait ends it at once.

## The dialog

GIS > Online Data... (`onlineData`, in the section "Online - Web Services")
opens `onlineDataDialog`, non-modal: the provider tree (`onlineProviders`)
grouped Australia by state, Global and Custom, filtered by `onlineSearch`
(which chooses the first layer left when it hides the chosen one); the chosen
layer's details (`onlineDetails`); the area (`onlineArea`, `onlineBox`,
`onlineBoxCrs`), the resolution (`onlineResolutionAuto`, `onlineResolution`),
the target layer (`onlineTargetLayer`), an OpenStreetMap tag (`onlineTag`),
Sentinel-2 dates and cloud (`onlineUseDates`, `onlineFrom`, `onlineTo`,
`onlineCloud`), the project's CRS (`onlineProjectCrs`) and one for a project
that has none (`onlineCrs`), a key (`onlineKey`, `onlineSaveKey`), Add Custom
Service (`onlineCustomUrl`, `onlineAddCustom`), Import (`onlineImport`) and a
status line (`onlineStatus`).

Since 2026-09-26 it also has what the verbs had and it did not:

- **The details say what `ONLINE INFO` says**: the service's CRS (or "the
  service's own"), its version, and its limits worded for its kind of
  service (`OnlineDataDialog::limitsText`, from the same fields
  `formatInfo` reports): tiles to a zoom and so many an import, features a
  page, pixels a request, square kilometres an import, or a STAC search's
  days and cloud.
- **A catalogue is searched in the dialog** (`onlineCatalogue`, shown only
  for a catalogue layer): `onlineCatalogueSearch` and `onlineCatalogueRun`
  run `ONLINE LAYERS <provider> <words>` through the workbench, as a typed
  line does; what the search found comes back from the workbench's job
  (`OnlineDataDialog::showCatalogueResults`) into `onlineCatalogueResults`
  (title, dataset, format, the address with any key taken out), as well as
  to the log; `onlineCatalogueAdd`, or a double click, runs `ONLINE CUSTOM
  <address>` for the chosen one. The details had said to type the verb.
- **A date for a time-enabled layer**: `onlineTimeDefault` (Default,
  latest) or `onlineTime`, which writes `time=YYYY-MM-DD`. Offered only for a
  layer with a time dimension or `{time}` in its address
  (`OnlineDataDialog::hasTime`); NASA GIBS's daily imagery is one.
- **Advanced** (`onlineAdvanced`): `onlineTimeoutOn` and `onlineTimeout`,
  Give up after [600] s, which writes `timeout=`; 600 is where it starts
  because it is a headless run's own deadline, so ticking it alone changes
  nothing.

`qt_widgets.GisOnlineDialog.*` drive each by its object name, and
`qt_the_online_dialog_shows_the_services_system_and_imports_within_a_deadline_headless` reads the details and
imports through the dialog with a deadline set. A live catalogue search runs
only with `KATANA_ONLINE_TESTS=1`: a CKAN search is a query, which the local
catalogue's `file://` layer cannot answer.

**The dialog does not import.** It writes the line `ONLINE IMPORT ...` would
be, reads it with the verb's own parser (`OnlineDataDialog::command`) and
hands the command to the workbench, which is also what the command line's
`ONLINE` lines reach (`OnlineDataWorkbench::run`). So nothing can be done in
the dialog that the verbs cannot do
(`qt_widgets.GisOnlineDialog.ImportHandsOnExactlyTheCommandTheVerbWouldParse`,
`qt_online_dialog_headless`).

**What arrives is framed.** An import of any area but `view` moves every
plan view to what it brought (`ViewWorkspace::zoomTo`): a typed box is
usually elsewhere, and web data in a real coordinate system lands thousands
of kilometres from a drawing in local coordinates - left alone, the view
showed the old drawing and a successful import looked like one that did
nothing. `area=view` leaves the view where the person is looking
(`qt_online_verbs_headless` reads the plan's count of what it drew).

## Verbs

```
ONLINE PROVIDERS [filter]
ONLINE LAYERS <provider> [search words, for a catalogue provider]
ONLINE INFO <provider> <layer>
ONLINE IMPORT <provider> <layer> area=view|drawing|selection|x0,y0,x1,y1|lonlat:w,s,e,n
              [res=<units per pixel>] [layer=<target layer>] [from=YYYY-MM-DD] [to=YYYY-MM-DD]
              [cloud=<percent>] [tag=<key[=value]>] [time=<date>] [crs=EPSG:<code>] [timeout=<s>]
ONLINE CUSTOM <url>
ONLINE KEY <name> [value]
```

In the window's command line and in a headless run
(`katana <project> --command "ONLINE ..." --screenshot out.png`). The
project's coordinate system is set with File > Project Coordinate System, the
dialog's Set Project CRS or `CRS SET` (`docs/cad.md`). `crs=` is for
a project with no coordinate system, which it sets when the import succeeds,
so the next import agrees with this one; given for a project that has one,
it must be the same system (by EPSG code, however it is spelt) or it is
refused. An ONLINE line reaches the workbench before a running drawing tool,
which would otherwise take it for an answer. Replies are records, one per line, a word and then `key=value`
fields quoted by the Logger's rule (`docs/architecture.md`, "Logging"):

```
providers count=1
provider id=nsw-spatial group=Australia/NSW layers=9 custom=no title="NSW Spatial Services"
layer provider=osm id=buildings kind=vector service=overpass key=no verified=no licence="ODbL 1.0" title=Buildings
field name=licence value="Copernicus DEM licence (free, worldwide)"
imported provider=copernicus layer=dem kind=elevation width=47 height=38 resolution=30 reference=1 name="..." requests=0 cache_hits=0 remote_sources=1 pages=0 duplicates=0 cached_result=no retrieved=2026-09-25 licence="..." attribution="..." source=https://...
warning text="scene date 2026-09-20, cloud cover 0%"
error verb=IMPORT code=InvalidCRS message="the project has no coordinate system, ..."
```

`katana_cli` and `katana_mcp` do not have the verbs yet ("Not done"): the
verbs' executor is the window's workbench (`src/katana_qt/gis_online.cpp`),
not the session's geo executor. The session holds reference data now, so
that is no longer what stands in the way.

An online raster is reopened with the project from the file it was cached
in (`docs/interop.md`, "Reference layers"), with its web source, licence and
attribution.

## Tests

With no network (the default): `tests/interop/test_online_catalogue.cpp` (the
built-in catalogue complete and valid), `tests/interop/test_online_requests.cpp`
(every request byte for byte, axis order, tile arithmetic against the slippy
map formula, SHA-256 against FIPS 180-2's vectors, STAC choice, Overpass),
`tests/interop/test_online_fetch.cpp` (discovery from the capabilities
fixtures in `tests/interop/data/online/`, and `fetchOnlineLayer` end to end
through a transport that answers from fixtures: paging, joining, the cache,
cancellation, every limit, keys, a local GeoTIFF and GeoJSON through the same
path as `file://` URLs), `tests/interop/test_online_verbs.cpp`,
`tests/qt_widgets/test_gis_online.cpp` (the dialog by object name), and
`qt_online_verbs_headless` and `qt_online_dialog_headless` (the application,
a user catalogue of a local file: four lots in MGA 56, one UNDO).
`OnlineEnvironment::transport` is the seam: empty, requests go to the
network; the tests put a function there.

With `KATANA_ONLINE_TESTS=1`, and only then:
`OnlineLive.CopernicusDemAtSydneyIsAboutSeaLevelToFiftyMetres` and
`OnlineLive.EveryBuiltInEndpointAnswers`, which asks every built-in service
for its description or one tile and names each that does not answer.

## Verified live, 2026-09-25

Headless, through the verbs, `QT_QPA_PLATFORM=offscreen`, in the cloud
environment described above:

- Copernicus GLO-30 over the Sydney CBD (151.20,-33.875 to 151.215,-33.865)
  into MGA 56 at 30 m: 47 x 38 cells, -4.4 to 111.8 m - a surface model, so
  the towers stand on the ground; the second time from the product cache
  (`cached_result=yes`, no request). Surface From Raster on a larger area:
  28 250 triangles.
- Natural Earth's coastline over 150.5..151.8 E: clipped to the area, and
  lying exactly on the land-sea edge of the Copernicus DEM - two independent
  sources agreeing in MGA 56.
- Sentinel-2 S2B_56HLH_20260920 through its STAC item (ONLINE CUSTOM, and a
  user catalogue for the band composite) at 10 m: the Opera House's pixel,
  at its MGA 56 coordinate by gdaltransform (334900.57, 6252288.75), reads
  (247, 241, 239) and Circular Quay's water beside it (4, 16, 19); the
  raster's west edge equals the transformed corner of the area to the
  millimetre.
- A headless import with `timeout=1` over a large area: cancelled at the
  deadline, nothing added. An area too large at the asked resolution:
  refused with the resolution that fits.

On the owner's Windows machine the same evening, with an ordinary network
(outside Australia), `bin/katana.exe` headless over 151.205,-33.870 to
151.210,-33.866 into MGA 56 - the first time these services were asked by
Katana:

- NSW Imagery at 0.23 m (240 tiles, 2048 x 1967), NSW cadastre lots (284),
  the NSW 5 m DEM (95 x 91), NSW roads (62), OpenStreetMap buildings through
  Overpass (230) and OpenStreetMap standard tiles (64): all imported, lying
  on one another in the view.
- `OnlineLive.EveryBuiltInEndpointAnswers`: every built-in service answered
  but Location SA's two base maps, whose CloudFront distribution refuses
  requests from outside Australia (403 "Request blocked", from a browser's
  User-Agent too); the test names such a service and does not fail on it.
- Before this, every request from `bin/katana.exe` had failed: the deploy
  left out the certificates libcurl checks `https://` against
  (`docs/building.md`), which no test saw because the tests load the
  toolchain's libcurl.

Not verified live: Queensland, Victorian, Tasmanian, ACT, Geoscience
Australia, DEA, data.gov.au, Earth Search search and NASA GIBS imports
(their services answered the live check above; an import over their areas
has not been run). Their request builders are tested against the
specifications and their answers against fixtures of the services'
published shapes.

## Failure modes

| Condition | Result |
|---|---|
| the project has no CRS and no `crs=` | `InvalidCRS`, naming `crs=` |
| the area is outside the provider's coverage | `InvalidArgument`, naming the coverage |
| a limit above | `InvalidArgument` or `Unsupported`, saying what fits |
| the service answers with an error document | `FileImportFailure` with the service's own words; not cached |
| 404 | `NotFound`; a missing tile or Copernicus tile is a hole, with a warning |
| 401, 403 | `FileImportFailure`: it may need a key, not allow this use, or not answer from where this computer is |
| CloudFront's own 403 "Request blocked" page | `FileImportFailure`: the service may answer only from its own country |
| a proxy refuses the host | `FileImportFailure`: the host may be blocked where Katana runs |
| the certificate bundle beside libcurl is missing (curl 77, "trust anchors") | `FileImportFailure` naming `etc/ssl/certs/ca-bundle.crt` |
| the service's certificate does not verify (curl 60) | `FileImportFailure`: a proxy or antivirus inspecting `https://` may be in the way |
| no answer in time | retried, then `FileImportFailure` saying how long it waited |
| cancelled | `InvalidState` "cancelled"; nothing applied |
| a feature outside the project CRS's domain | `InvalidCRS` naming the feature |
| a malformed verb | `InvalidArgument` with the verb's usage |
| a key needed and not stored | `NotFound`, naming `ONLINE KEY <name>` |

## Not done

- The catalogue's `verified` fields are still empty for the services that
  answered on 2026-09-25 (above); filling them in is the owner's call.
- WMTS is read only on the Web Mercator (GoogleMapsCompatible) grid; a service
  offering only a national grid is listed by discovery as unsupported. WCS is
  1.0.0 only.
- A raster is not re-fetched as the view zooms: it is an import at one
  resolution, as a file import is.
- Vector features that meet the area are imported whole, except from a `file`
  layer, which is clipped; an ArcGIS lot crossing the area's edge arrives
  entire, which is what a cadastre wants.
- `katana_cli` and `katana_mcp` have no `ONLINE` verbs, and there is no
  `katana_online_*` tool. The executor has to move from the window's
  workbench into the session's geo executor (`docs/geoprocessing.md`) - a
  job in the window, inline in a session, the keys store and cache paths
  passed in. The GDAL integration's plan left it out as not algorithm work;
  it belongs with the retrofit below.
- `area=view|drawing|selection|x0,y0,x1,y1` is a second scope grammar, where
  every tool is to act on the one scope and filter: it should be the shared one
  (`[SELECTION|VIEW|DRAWING|AREA x0,y0,x1,y1|LAYERS a,b [ONLY]]`,
  `include/katana/cad/scope_verbs.hpp`), with VIEW refused headless in
  favour of AREA, and the dialog's area choice the shared
  `ScopeFilterWidget`. `lonlat:` has no counterpart there yet. The
  scope-retrofit list from the shared scope's audit owns it.
- The dialog's own lines are not echoed in the command log the way the
  window's one executor echoes a dialog's line: the dialog hands the
  workbench a parsed command, which predates the executor. The replies are
  logged.
- A cached file that is gone when the project reopens is warned of, not
  fetched again - `ONLINE IMPORT` fetches it.
