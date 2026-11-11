# Third-party notices

Katana's own code is licensed under GPL-3.0-or-later with the additional terms
in `ADDITIONAL_TERMS.md` (`LICENSE`, `LICENSING.md`). This file lists the
third-party components that are compiled into Katana's programs (`katana`,
`katana_cli`, `katana_mcp`) or travel beside them in its packages. For each:
its licence, how it is linked, what that asks of Katana, and where to get its
licence text and its source. Every component keeps its own licence; none is
relicensed by Katana's.

It is an audit made on 2026-10-08, not legal advice. Three words are used
about how sure each fact is:

* **Verified**: read on the build machine, in the component's own header or
  licence file, or in the project's own documentation.
* **Recorded**: taken from the package manager's metadata, which can be
  imprecise, and not checked against the source.
* **Unverified**: said so where it applies; section 7 collects them.

In sections 1 and 2 a licence was read in the installed package's licence file
or in a header, except where the cell says recorded.

**What was audited.** The Windows x86-64 package: built with MSYS2 UCRT64
(GCC 16.2, Qt 6.11.2, CGAL 6.2, GDAL 3.13.2, PDAL 2.10.2, PROJ 9.8.1), from the
200 DLLs in a release build tree's `bin/` and the package versions in MSYS2's
database. The Windows ARM64 (MSYS2 CLANGARM64), Linux and macOS (conda-forge)
packages use the same sources with other package sets; they were not audited
(section 6).

**Where licence texts and source are obtained.** For every MSYS2 package:
the licence texts are in the package's own folder under
`share/licenses/<package>/` of the toolchain (`C:/msys64/ucrt64/share/licenses`),
the build recipe is at <https://github.com/msys2/MINGW-packages>, and the
source archive of the exact package version is under
<https://repo.msys2.org/mingw/sources/>. The upstream project of each is in
the tables.

## 1. Compiled into the programs

| Component | Version | Licence | What it asks of Katana |
|---|---|---|---|
| CGAL, 2D Triangulations package | 6.2 | GPL-3.0-or-later (or a commercial licence from GeometryFactory, which Katana does not hold) | **Decisive.** `src/katana_terrain/cdt_backend.cpp` includes it; CGAL is header-only, so it is compiled into `katana_terrain` and so into all three programs. The programs as a whole must be offered under GPL version 3 or later, with source. |
| CGAL, other packages used (kernel, spatial sorting, number types, STL extensions) | 6.2 | LGPL-3.0-or-later (or commercial) | Compatible with GPL-3.0-or-later. Keep the notices. |
| Eigen (`Eigen/Dense` only) | 5.0.1 (the Linux and macOS toolchain pins 3.4) | MPL-2.0; its licence folder also holds Apache-2.0, BSD and MINPACK texts for some parts | Keep the notices. MPL-2.0 section 3.2(a) also asks, when an executable that contains Eigen is passed on, that recipients be told how to obtain Eigen's source (<https://gitlab.com/libeigen/eigen>): the written offer of source has to name it. No LGPL file is reached through `Eigen/Dense` (searched in 5.0.1). |
| nlohmann/json | 3.12.0 | MIT | Keep the copyright and permission notice. |
| Boost headers (through CGAL), and the serialization DLL | 1.92.0 | BSL-1.0 | A notice is needed only with source; none for object code. |

CGAL, in detail (verified, the count made twice: `g++ -M` on `cdt_backend.cpp`
with the build's flags, then reading each header's `SPDX-License-Identifier`
line). With the build's `-DCGAL_USE_GMPXX=1`, the 653 CGAL headers that the file
includes, directly and through each other, carry: 601 `LGPL-3.0-or-later OR
LicenseRef-Commercial`, 29 `LGPL-3.0-or-later`, 22 `GPL-3.0-or-later OR
LicenseRef-Commercial`, 1 `BSL-1.0`. Without that define the same walk reaches
648 (596 of the first kind); the 22 are the same either way. The 22 are the 2D
Triangulations package and its data structure package, TDS_2 (20 headers:
`Triangulation_2.h`, `Constrained_Delaunay_triangulation_2.h`,
`Constrained_triangulation_plus_2.h`, `Triangulation_data_structure_2.h`,
vertex and face bases, and the rest of the two packages), and two helper
headers of the BGL package under `CGAL/boost/graph/internal/`
(`graph_traits_2D_TDS_helper.h`, `graph_traits_2D_triangulation_helper.h`),
which carry the GPL identifier although that package is otherwise LGPL. CGAL's
own package files at tag v6.2 (`Triangulation_2/package_info/Triangulation_2/license.txt`
and the one for `TDS_2`) say "GPL (v3 or later)", and those for the spatial
sorting, kernel, number types, STL extensions, property map and BGL packages
say "LGPL (v3 or later)". CGAL's package overview says "License: GPL" for the
first and "License: LGPL" for the kernel and number types
(<https://doc.cgal.org/latest/Manual/packages.html>); its licence header
`CGAL/license/Triangulation_2.h` says the package is used "under the terms of the
GPLv3+" unless a commercial licence is held. Source and licence texts:
<https://github.com/CGAL/cgal>, `LICENSE.GPL` and `LICENSE.LGPL`; also in
`share/licenses/cgal`.

## 2. Shared libraries beside the programs

These are DLLs in `bin/` next to the executables. A DLL can be replaced by the
user with a modified build, which is what the LGPL asks for (the project's
reasoning for DLLs and not static linking is in `docs/building.md`).

The "Imported by" column was read from the import tables of the programs and
DLLs of a Windows release build (2026-10-08). The three programs import
`libgdal`, `libgmp`, `libpdalcpp`, `libproj`, `libsqlite3` and the GCC runtime,
and `katana.exe` also imports Qt6Core, Qt6Gui, Qt6PrintSupport and Qt6Widgets.
The others are reached only through those, and "through" says which. A
library Katana only reaches through another is still shipped, and its licence
is still passed on.

| Component | Version | Licence | Imported by | What it asks of Katana |
|---|---|---|---|---|
| Qt 6: Core, Gui, Widgets, PrintSupport, with the platform, style, image-format and TLS plugins (`qwindows`, `qoffscreen`, `qmodernwindowsstyle`, `qgif`, `qico`, `qjpeg`, `qschannelbackend`, others) | 6.11.2 | The headers read (QtCore, QtGui, QtWidgets, QtNetwork, QtPrintSupport and the private RHI header; the Qt6Network DLL is shipped, but no Katana source or binary uses it and no DLL in `bin/` imports it, so `windeployqt` is presumed to copy it for a plugin) say `LicenseRef-Qt-Commercial OR LGPL-3.0-only OR GPL-2.0-only OR GPL-3.0-only`; Katana uses them under LGPL-3.0-only | `katana.exe` (the other two programs import no Qt library) | LGPL-3.0 section 4: give prominent notice that Qt is used and covered by the LGPL; accompany the program with the GPL and LGPL texts; where the program displays copyright notices while it runs (the About dialog), include Qt's copyright notice among them with a pointer to those texts; link through a shared-library mechanism, which the DLLs beside the program are. Katana also uses Qt's private GUI headers (`Qt6::GuiPrivate`, in the GPU renderer): the same licence, tied to one Qt version. |
| GDAL | 3.13.2 | MIT (recorded; the licence file collects the licences of GDAL's bundled parts) | all three programs | Keep the notice. |
| PDAL | 2.10.2 | BSD-3-Clause (its licence file collects the licences of its bundled parts, among them Apache-2.0) | all three programs | Keep the notice. |
| PROJ | 9.8.1 | MIT | all three programs | Keep the notice. |
| GEOS (through GDAL) | 3.15.0 | LGPL-2.1-or-later (recorded; `COPYING` holds the LGPL 2.1 text, "or later" was not checked) | through GDAL (also librttopo, libspatialite) | Allow replacing the DLL; give the licence text and source. |
| GMP | 6.3.0 | LGPL-3.0-or-later or GPL-2.0-or-later, at the recipient's choice (`gmp.h`) | all three programs (the number types CGAL uses) | As Qt: LGPL notice, texts, replaceable. |
| MPFR | 4.2.2 | LGPL-3.0-or-later (`mpfr.h`) | through libSFCGAL, which GDAL imports | As Qt. |
| libcurl | 8.22.0 | the curl licence (MSYS2 records "MIT") | through GDAL, PDAL, PROJ and others | Keep the notice. |
| OpenSSL (libcrypto, libssl) | 3.6.5 | Apache-2.0 | through libcurl, GDAL, PDAL and others | Keep the licence text (Apache-2.0 section 4). OpenSSL has no NOTICE file (none at its GitHub `master` or `openssl-3.6`, and none in the MSYS2 package), so section 4(d) has nothing to pass on. Compatible with GPL version 3, not with version 2 (<https://www.gnu.org/licenses/license-list.html#apache2>). |
| SQLite | 3.53.4 | public domain | all three programs | None. |
| zlib | 1.3.2 | Zlib | through Qt, GDAL and many others | Keep the notice in source. |
| GCC runtime (libstdc++, libgcc, libgomp, libgfortran, libquadmath) and libwinpthread | 16.2.0 | GPL-3.0-or-later WITH GCC-exception-3.1 (libquadmath LGPL-2.1-or-later; libwinpthread MIT and BSD-3-Clause-Clear) (recorded) | all three programs (libstdc++, libgcc, libwinpthread); the other runtime libraries through GDAL and others | The GCC Runtime Library Exception lets the object code be conveyed under another licence. |

Each of these is also in the table in section 3, with the version and licence
as MSYS2 records them.

## 3. Everything else in the Windows x86-64 package

The rest of the 200 DLLs come in as the dependencies of GDAL, PDAL, Qt and the
libraries above; Katana's own code does not call them directly. The table is every MSYS2 package that owns a DLL in
`bin/`, 120 in all, with the licence MSYS2 records for it (**recorded**). Where
MSYS2 lists several licences for a package, the package holds parts under each
(a library and its tools, say) or offers a choice; which applies to the DLL
shipped was not checked, except where a note says so. A note that names a
header means that header was read.

| Package | Version | Licence as recorded by MSYS2 | DLLs | Upstream | Note |
|---|---|---|---|---|---|
| abseil-cpp | 20260526.0-1 | Apache-2.0 | 31 | <https://abseil.io> |  |
| aom | 3.15.1-1 | BSD-2-Clause | 1 | <https://aomedia.org/> |  |
| armadillo | 15.6.0-1 | Apache-2.0 | 1 | <https://arma.sourceforge.io/> |  |
| arpack | 3.9.1-8 | BSD-3-Clause | 1 | <https://forge.scilab.org/index.php/p/arpack-ng/> |  |
| arrow | 25.0.1-3 | Apache-2.0 | 5 | <https://arrow.apache.org/> |  |
| aws-c-auth | 1.0.0-1 | Apache-2.0 | 1 | <https://github.com/awslabs/aws-c-auth> |  |
| aws-c-cal | 1.0.0-1 | Apache-2.0 | 1 | <https://github.com/awslabs/aws-c-cal> |  |
| aws-c-common | 1.0.0-1 | Apache-2.0 | 1 | <https://github.com/awslabs/aws-c-common> |  |
| aws-c-compression | 1.0.0-1 | Apache-2.0 | 1 | <https://github.com/awslabs/aws-c-compression> |  |
| aws-c-event-stream | 1.0.0-1 | Apache-2.0 | 1 | <https://github.com/awslabs/aws-c-event-stream> |  |
| aws-c-http | 1.0.0-1 | Apache-2.0 | 1 | <https://github.com/awslabs/aws-c-http> |  |
| aws-c-io | 1.0.0-1 | Apache-2.0 | 1 | <https://github.com/awslabs/aws-c-io> |  |
| aws-c-mqtt | 1.0.0-1 | Apache-2.0 | 1 | <https://github.com/awslabs/aws-c-mqtt> |  |
| aws-c-s3 | 1.1.2-1 | Apache-2.0 | 1 | <https://github.com/awslabs/aws-c-s3> |  |
| aws-c-sdkutils | 1.0.0-1 | Apache-2.0 | 1 | <https://github.com/awslabs/aws-c-sdkutils> |  |
| aws-checksums | 1.0.0-1 | Apache-2.0 | 1 | <https://github.com/awslabs/aws-checksums> |  |
| aws-crt-cpp | 0.43.7-1 | Apache-2.0 | 1 | <https://github.com/awslabs/aws-crt-cpp> |  |
| aws-sdk-cpp | 1.11.893-1 | Apache-2.0 | 5 | <https://github.com/aws/aws-sdk-cpp> |  |
| blosc | 1.21.6-3 | BSD-3-Clause | 1 | <https://blosc.org/> |  |
| boost-libs | 1.92.0-3 | BSL-1.0 | 1 | <https://www.boost.org/> | Section 1 (headers) and the serialization DLL. |
| brotli | 1.2.0-1 | MIT | 3 | <https://github.com/google/brotli> |  |
| bzip2 | 1.0.8-4 | custom | 1 | <https://sourceware.org/bzip2/> |  |
| cfitsio | 1~4.7.0-1 | CFITSIO | 1 | <https://heasarc.gsfc.nasa.gov/fitsio/> |  |
| crypto++ | 8.9.0-2 | BSL-1.0 | 1 | <https://github.com/weidai11/cryptopp> |  |
| curl | 8.22.0-1 | MIT | 1 | <https://curl.se/> | Section 2. The licence file in the package is the curl licence. |
| dav1d | 1.5.4-1 | BSD-2-Clause | 1 | <https://code.videolan.org/videolan/dav1d> |  |
| double-conversion | 3.4.0-1 | BSD-3-Clause | 1 | <https://github.com/google/double-conversion> |  |
| expat | 2.8.5-1 | MIT | 1 | <https://libexpat.github.io/> |  |
| fontconfig | 2.18.3-1 | custom | 1 | <https://wiki.freedesktop.org/www/Software/fontconfig/> |  |
| freetype | 2.14.3-1 | GPL-2.0-or-later OR FTL | 1 | <https://www.freetype.org/> |  |
| gdal | 3.13.2-2 | MIT | 1 | <https://gdal.org/> | Section 2. |
| geos | 3.15.0-1 | LGPL-2.1-or-later | 2 | <https://libgeos.org> | Section 2. LGPL; shipped as a DLL. |
| gettext-runtime | 1.0-1 | GPL-3.0-or-later AND LGPL-2.1-or-later | 1 | <https://www.gnu.org/software/gettext/> |  |
| giflib | 6.1.3-1 | MIT | 1 | <https://sourceforge.net/projects/giflib/> |  |
| glib2 | 2.90.0-1 | LGPL-2.1-or-later | 4 | <https://gitlab.gnome.org/GNOME/glib> |  |
| gmp | 6.3.0-2 | LGPL3; GPL | 2 | <https://gmplib.org/> | Section 2. gmp.h: LGPL-3.0-or-later or GPL-2.0-or-later, at the recipient's choice. |
| graphite2 | 1.3.15-1 | LGPL-2.1-or-later | 1 | <https://github.com/silnrsi/graphite> |  |
| harfbuzz | 14.5.0-1 | MIT | 1 | <https://harfbuzz.github.io/> |  |
| hdf4 | 4.3.0-1 | BSD-3-Clause | 2 | <https://github.com/HDFGroup/hdf4> |  |
| hdf5 | 2.2.0-2 | BSD-3-Clause | 2 | <https://www.hdfgroup.org/HDF5/> |  |
| highway | 1.4.0-3 | Apache-2.0 | 1 | <https://github.com/google/highway> |  |
| icu | 78.3-4 | ICU | 3 | <https://icu.unicode.org/home/> |  |
| imath | 3.2.3-3 | BSD-3-Clause | 1 | <https://www.openexr.com/> |  |
| jbigkit | 2.1-6 | GPL-2.0 | 1 | <https://www.cl.cam.ac.uk/~mgk25/jbigkit/> | MSYS2 records "GPL-2.0", which does not say "or later". The source header of `libjbig/jbig.c` in JBIG-KIT 2.1 (the version `jbig.h` of the package names) reads "either version 2 of the License, or (at your option) any later version", in Debian's copy of 2.1-6.1 and in a GitHub mirror, and Debian's copyright file says GPL-2+. So GPL-2.0-or-later, which fits GPL-3.0-or-later. The MSYS2 source archive itself was not opened. |
| json-c | 0.19-2 | MIT | 1 | <https://github.com/json-c/json-c> |  |
| kvazaar | 2.3.2-1 | BSD-3-Clause | 1 | <https://ultravideo.fi/kvazaar.html> |  |
| lcms2 | 2.19.1-1 | MIT AND GPL-3.0-or-later | 1 | <https://www.littlecms.com/color-engine/> |  |
| lerc | 4.1.1-1 | Apache-2.0 | 1 | <https://github.com/Esri/lerc> |  |
| libaec | 1.1.7-2 | BSD-2-Clause | 2 | <https://github.com/Deutsches-Klimarechenzentrum/libaec> |  |
| libarchive | 3.8.9-6 | BSD-2-Clause | 1 | <https://www.libarchive.org/> |  |
| libb2 | 0.98.1-3 | custom:CC0 | 1 | <https://blake2.net/> |  |
| libde265 | 1.1.3-1 | LGPL-3.0-or-later; MIT | 1 | <https://github.com/strukturag/libde265> |  |
| libdeflate | 1.26-1 | MIT | 1 | <https://github.com/ebiggers/libdeflate> |  |
| libffi | 3.8.0-1 | MIT | 1 | <https://sourceware.org/libffi> |  |
| libfreexl | 2.0.0-1 | MPL; GPL; LGPL | 1 | <https://www.gaia-gis.it/fossil/freexl> |  |
| libgcc | 16.2.0-4 | GPL-3.0-or-later WITH GCC-exception-3.1 AND GFDL-1.3-or-later | 1 | <https://gcc.gnu.org> | The record names the GCC Runtime Library Exception 3.1. |
| libgeotiff | 1.7.4-1 | MIT | 1 | <https://github.com/OSGeo/libgeotiff> |  |
| libgfortran | 16.2.0-4 | GPL-3.0-or-later WITH GCC-exception-3.1 AND GFDL-1.3-or-later | 1 | <https://gcc.gnu.org> | The record names the GCC Runtime Library Exception 3.1. |
| libgomp | 16.2.0-4 | GPL-3.0-or-later WITH GCC-exception-3.1 AND GFDL-1.3-or-later | 1 | <https://gcc.gnu.org> | The record names the GCC Runtime Library Exception 3.1. |
| libheif | 1.23.5-1 | LGPL-3.0 AND MIT | 1 | <https://github.com/strukturag/libheif> | libheif.h says LGPL version 3 or later. |
| libiconv | 1.19-1 | LGPL-2.1-or-later; (documentation) GPL-3.0-or-later | 1 | <https://www.gnu.org/software/libiconv/> |  |
| libidn2 | 2.3.8-4 | GPL-2.0-or-later; LGPL-3.0-or-later | 1 | <https://www.gnu.org/software/libidn/#libidn2> |  |
| libjpeg-turbo | 3.2.0-1 | custom:BSD-like | 1 | <https://libjpeg-turbo.virtualgl.org/> |  |
| libjxl | 0.12.0-1 | BSD-3-Clause | 3 | <https://jpeg.org/jpegxl/> |  |
| libkml | 1.3.0-11 | BSD-3-Clause | 3 | <https://github.com/libkml/libkml/> |  |
| libmariadbclient | 3.4.9-1 | LGPL-2.1-or-later | 1 | <https://mariadb.org/> |  |
| libpng | 1.6.59-1 | custom | 1 | <http://www.libpng.org/pub/png/libpng.html> |  |
| libpsl | 0.21.5-3 | MIT | 1 | <https://rockdaboot.github.io/libpsl/> |  |
| libquadmath | 16.2.0-4 | LGPL-2.1-or-later AND GFDL-1.2-or-later | 1 | <https://gcc.gnu.org> |  |
| librttopo | 1.1.0-2 | GPL-2.0-or-later | 1 | <https://gitea.osgeo.org/rttopo/librttopo> | librttopo.h says GPL version 2 or later. |
| libspatialite | 5.1.0-5 | MPL-1.1 OR LGPL-2.1-or-later OR GPL-2.0-or-later | 1 | <https://www.gaia-gis.it/fossil/libspatialite/index> |  |
| libssh2 | 1.11.1-2 | BSD-3-Clause | 1 | <https://libssh2.org/> |  |
| libstdc++ | 16.2.0-4 | GPL-3.0-or-later WITH GCC-exception-3.1 AND GFDL-1.3-or-later | 1 | <https://gcc.gnu.org> | The record names the GCC Runtime Library Exception 3.1. |
| libtiff | 4.7.2-1 | MIT | 1 | <https://libtiff.gitlab.io/libtiff/> |  |
| libunistring | 1.4.2-1 | LGPL-3.0-or-later OR GPL-3.0-or-later | 1 | <https://www.gnu.org/software/libunistring> |  |
| libutf8proc | 2.11.3-1 | MIT | 1 | <https://juliastrings.github.io/utf8proc/> |  |
| libwebp | 1.6.0-1 | BSD-3-Clause | 2 | <https://developers.google.com/speed/webp/> |  |
| libwinpthread | 14.0.0.r426.g4564ee4b5-1 | MIT AND BSD-3-Clause-Clear | 1 | <https://www.mingw-w64.org/> |  |
| libx264 | 0.165.r3222.b35605a-3 | custom | 1 | <https://www.videolan.org/developers/x264.html> | MSYS2 records only "custom"; x264.h says GPL version 2 or later. |
| libxml2 | 2.15.4-1 | MIT | 1 | <https://gitlab.gnome.org/GNOME/libxml2/-/wikis/home> |  |
| libzip | 1.11.4-1 | BSD-3-Clause | 1 | <https://libzip.org/> |  |
| lz4 | 1.10.0-1 | BSD; GPL2 | 1 | <https://lz4.github.io/lz4/> |  |
| md4c | 0.6.0-1 | MIT | 1 | <https://github.com/mity/md4c> |  |
| minizip | 1.3.2-2 | Zlib | 1 | <https://www.zlib.net/> |  |
| mpfr | 4.2.2-3 | LGPL-3.0-or-later | 1 | <https://www.mpfr.org> | Section 2. mpfr.h: LGPL-3.0-or-later. |
| netcdf | 4.10.1-2 | BSD-3-Clause | 1 | <https://www.unidata.ucar.edu/software/netcdf/> |  |
| nghttp2 | 1.70.0-1 | MIT | 1 | <https://nghttp2.org/> |  |
| nghttp3 | 1.18.0-1 | MIT | 1 | <https://nghttp2.org/nghttp3/> |  |
| ngtcp2 | 1.25.0-1 | MIT | 2 | <https://nghttp2.org/ngtcp2/> |  |
| nspr | 4.40-1 | MPL-2.0 | 3 | <https://developer.mozilla.org/en-US/docs/Mozilla/Projects/NSPR> |  |
| nss | 3.129-1 | MPL-2.0 | 3 | <https://firefox-source-docs.mozilla.org/security/nss/index.html> |  |
| openblas | 0.3.34-1 | BSD-3-Clause | 1 | <https://www.openblas.net/> |  |
| openexr | 3.4.15-2 | BSD-3-Clause | 4 | <https://www.openexr.com/> |  |
| openh264 | 2.6.0-2 | BSD-2-Clause | 1 | <https://www.openh264.org/> |  |
| openjpeg2 | 2.5.4-2 | BSD-2-Clause | 1 | <https://www.openjpeg.org/> |  |
| openjph | 0.32.0-1 | BSD-2-Clause | 1 | <https://openjph.org/> |  |
| openssl | 3.6.5-1 | Apache-2.0 | 2 | <https://openssl-library.org> | Section 2. Apache-2.0: compatible with GPL version 3, not with version 2. |
| pcre2 | 10.49-1 | BSD-3-Clause | 2 | <https://pcre.org/> |  |
| pdal | 2.10.2-1 | BSD-3-Clause | 1 | <https://www.pdal.io> | Section 2. |
| podofo | 1.1.1-2 | LGPL-2.0-or-later AND GPL-2.0-or-later | 1 | <https://podofo.sourceforge.io/> |  |
| poppler | 26.08.0-1 | GPL-2.0-or-later | 1 | <https://poppler.freedesktop.org/> | Headers say GPL version 2 or later. |
| postgresql | 18.6-4 | PostgreSQL | 1 | <https://www.postgresql.org/> |  |
| proj | 9.8.1-1 | MIT | 1 | <https://trac.osgeo.org/proj/> | Section 2. |
| qhull | 2020.2-3 | custom | 1 | <http://www.qhull.org/> |  |
| qt6-base | 6.11.2-2 | LGPL-3.0-only WITH Qt-GPL-exception-1.0 AND AFL-2.1 AND Apache-2.0 AND BSL-1.0 AND CC0-1.0 AND BSD-3-Clause AND CC-BY-4.0 AND GFDL-1.3-no-invariants-only AND GPL-2.0-only AND GPL-2.0-or-later AND GPL-3.0-only AND custom | 16 | <https://www.qt.io> | Used under LGPL-3.0-only (section 2). The package licence is the union of every part of the Qt tree. |
| rav1e | 0.8.1-1 | BSD-2-Clause | 1 | <https://github.com/xiph/rav1e/> |  |
| re2 | 20251105-3 | BSD-3-Clause | 1 | <https://github.com/google/re2> |  |
| sfcgal | 2.3.0-1 | LGPL-2.0-or-later | 1 | <https://www.sfcgal.org/> | Depends on CGAL (pacman). Whether it uses CGAL's GPL packages was not checked. |
| snappy | 1.2.2-2 | BSD-3-Clause | 1 | <https://github.com/google/snappy> |  |
| sqlite3 | 3.53.4-1 | PublicDomain | 1 | <https://www.sqlite.org/> | Section 2. |
| superlu | 7.0.1-1 | BSD-3-Clause | 1 | <https://portal.nersc.gov/project/sparse/superlu/> |  |
| svt-av1 | 4.2.0-1 | BSD-3-Clause-Clear | 1 | <https://gitlab.com/AOMediaCodec/SVT-AV1> |  |
| thrift | 0.24.0-1 | Apache-2.0 | 1 | <https://thrift.apache.org/> |  |
| uriparser | 1.0.2-1 | BSD-3-Clause | 1 | <https://uriparser.github.io/> |  |
| x265 | 4.3-1 | GPL | 1 | <https://www.x265.org> | x265.h says GPL version 2 or later. |
| xerces-c | 3.3.0-4 | Apache-2.0 | 1 | <https://xerces.apache.org/xerces-c> |  |
| xz | 5.8.4-1 | 0BSD AND LGPL-2.1-or-later AND GPL-2.0-or-later | 1 | <https://tukaani.org/xz> |  |
| zlib | 1.3.2-2 | Zlib | 1 | <https://www.zlib.net/> | Section 2. |
| zlib-ng | 2.3.3-2 | Zlib | 1 | <https://github.com/zlib-ng/zlib-ng> |  |
| zstd | 1.5.7-2 | BSD-3-Clause OR GPL-2.0-or-later | 1 | <https://facebook.github.io/zstd/> |  |

What the table shows. Most are permissive (MIT, BSD, Zlib, Apache-2.0, ISC and
similar). A smaller group is LGPL (GEOS, glib, libheif, libde265, libmariadb,
gettext's libintl, libiconv, graphite2, libunistring, the GCC runtime), shipped
as DLLs. A few are GPL, all "version 2 or later" where a header was read
(poppler, librttopo, x264, x265) and `jbigkit` (its source header): they are
loaded into the same process through GDAL, and "version 2 or later" can be
passed on under version 3, so they fit Katana being GPL-3.0-or-later. They
would fit GPL-2.0-only too: it is CGAL's version-3 code, not these, that rules
version 2 out. `libSFCGAL.dll` depends on CGAL and on MPFR.

## 4. Data shipped beside the programs

| What | Where | Licence | Note |
|---|---|---|---|
| PROJ data (`proj.db` and grids) | `share/proj` | PROJ's licence is MIT; `proj.db` is built from geodetic registries, notably the EPSG dataset, which has terms of its own | **Unverified**: the registry terms were not read. |
| GDAL data files | `share/gdal` | as GDAL (MIT, with the notices in GDAL's licence file) | Recorded. |
| Certificate bundle | `etc/ssl/certs/ca-bundle.crt` | from the `ca-certificates` package; MSYS2 records "MPL GPL"; the certificate data is Mozilla's, under MPL-2.0 | **Unverified** which part of the package the file falls under. |
| The sample drawings and data | `share/katana/samples` | **not covered by the project licence**: see `LICENSING.md` | The whole of `samples/` is installed. |

Katana's own resources (`resources/`: the icons, the plot frame, the online
source catalogue) are part of Katana. The one exception is the built-in
customisation, `resources/customisation/nsw.customisation.json`, which is
compiled into every program as an alternate built-in beside Katana Standard (the
default, whose linestyles, symbols, layers and colours are Katana's own) and is
**not** covered by the project licence. Katana Standard uses this file's survey
code keys (the identifiers), with what each key stands for and the kinds of rule
it has, reused for field-data compatibility on the owner's decision; what it
names, draws and words is Katana's own. The file is shipped on the owner's
decision and statement that the information in it is public; a build without
it is possible: see `LICENSING.md`, "What the licence does not cover".

## 5. Used to build or test, never shipped

| Component | Version | Licence | Use |
|---|---|---|---|
| GoogleTest | 1.14.0 | BSD-3-Clause | the tests; downloaded into `third_party/_cache` (not committed) |
| Google Benchmark | 1.9.1 | Apache-2.0 | the benchmarks; the release builds switch them off |
| NSIS | 3.12 (MSYS2) | zlib/libpng licence; its compression modules have their own (`share/licenses/nsis/COPYING`) | makes the Windows installer; its small runtime stub is inside every installer |
| Qt's tools (`moc`, `rcc`, `windeployqt`, and `qsb` where the GPU shaders are baked) | 6.11.2 | recorded for `qt6-base` and `qt6-tools`: LGPL-3.0-only WITH Qt-GPL-exception-1.0, with GPL-3.0-only parts | build and packaging only; none is shipped |
| CMake, Ninja, the compilers, Python, `osslsigncode` | n/a | their own licences | build, test and signing; not part of a package |

**The Rust port's crates** (`docs/rust.md`). Nothing of the port ships before
its step 8, so for now these are compiled only into the port's libraries and
tests; they move to section 1 with the first Rust program that ships. The
versions are those of `rust/Cargo.lock`, and the licences are each crate's own
`license` field as `cargo tree` reported it on 2026-11-07 (recorded). The
rows up to windows-sys are the crates the port names; the rest come with them. The
port's JSON is its own (katana_core::json, translated from nlohmann/json, below), so
no JSON crate is used. `autocfg`
and `paste` run only while building.

| Crate | Version | Licence | Brought in by |
|---|---|---|---|
| nalgebra | 0.34.2 | Apache-2.0 | katana_survey (dense storage, Cholesky) |
| spade | 2.15.1 | MIT OR Apache-2.0 | katana_terrain (the constrained Delaunay triangulation) |
| robust | 1.2.0 | MIT OR Apache-2.0 | katana_terrain (exact orientation and in-circle predicates) |
| miniz_oxide | 0.8.9 | MIT OR Zlib OR Apache-2.0 | katana_update (raw inflate) |
| ed25519-dalek, curve25519-dalek | 2.2.0, 4.1.3 | BSD-3-Clause | katana_update (Ed25519 verification and the RFC 8032 checks on the key) |
| windows-sys | 0.60.2 | MIT OR Apache-2.0, unverified (section 7) | katana_update, Windows only |
| windows-targets, windows_x86_64_gnu | 0.53.5, 0.53.1 | MIT OR Apache-2.0, unverified (section 7) | windows-sys, Windows only |
| equivalent, hashbrown, foldhash, allocator-api2 | 1.0.3, 0.16.1, 0.2.0, 0.2.21 | Apache-2.0 OR MIT; MIT OR Apache-2.0; Zlib; MIT OR Apache-2.0 | spade |
| simba, approx, num-traits, num-complex, num-integer, num-rational, typenum, matrixmultiply, rawpointer, wide, safe_arch, bytemuck | 0.9.1, 0.5.1, 0.2.19, 0.4.6, 0.1.47, 0.4.2, 1.20.1, 0.3.11, 0.2.1, 0.7.33, 0.7.4, 1.25.2 | Apache-2.0 (simba, approx); Zlib OR Apache-2.0 OR MIT (wide, safe_arch, bytemuck); MIT OR Apache-2.0 (the rest) | nalgebra |
| smallvec | 1.16.5 | MIT OR Apache-2.0 | spade |
| adler2 | 2.0.1 | 0BSD OR MIT OR Apache-2.0 | miniz_oxide |
| ed25519, signature, sha2, digest, block-buffer, crypto-common, generic-array, cpufeatures, cfg-if, subtle, zeroize | 2.2.3, 2.2.0, 0.10.9, 0.10.7, 0.10.4, 0.1.7, 0.14.7, 0.2.17, 1.0.5, 2.6.1, 1.9.1 | Apache-2.0 OR MIT (ed25519, signature, zeroize); MIT (generic-array); BSD-3-Clause (subtle); MIT OR Apache-2.0 (the rest) | ed25519-dalek, curve25519-dalek |
| autocfg, paste | 1.5.1, 1.0.15 | Apache-2.0 OR MIT; MIT OR Apache-2.0 | build-time only, a build script's helper and a macro, both reached through nalgebra |
| curve25519-dalek-derive, rustc_version, semver, version_check, proc-macro2, quote, syn, unicode-ident | 0.1.1, 0.4.1, 1.0.28, 0.9.5, 1.0.107, 1.0.47, 2.0.119, 1.0.27 | MIT OR Apache-2.0 (unicode-ident: (MIT OR Apache-2.0) AND Unicode-3.0) | build-time only: a derive macro and build scripts' helpers, reached through curve25519-dalek and generic-array |
| cc, find-msvc-tools, shlex | 1.11.1, 0.1.15, 2.0.1 | MIT OR Apache-2.0 (`cargo tree`, 2026-11-10) | build-time only: compiles katana_gis's C checks of its GDAL declarations and katana_pointcloud's PDAL shim from their build scripts |
| egui | 0.36.2 | MIT OR Apache-2.0 | katana_window (the immediate-mode UI) |
| eframe | 0.36.2 | MIT OR Apache-2.0 | katana_window (the desktop runner, `katana` only) |
| egui_dock | 0.21.1 | MIT | katana_window (the docks) |
| egui_extras | 0.36.2 | MIT OR Apache-2.0 | katana_window (the managers' tables) |
| wgpu | 30.0.1 | MIT OR Apache-2.0 | katana_gpu and katana_window (the GPU layer and egui's renderer) |
| pollster | 0.4.0 | Apache-2.0 OR MIT | the window's crates below (an older line, beside the port's 1.0.1) |
| pollster | 1.0.1 | Apache-2.0 OR MIT | katana_gpu and katana_window (waits on wgpu's requests) |
| tiny-skia | 0.12.0 | BSD-3-Clause | katana_window (the raster canvas) |
| cosmic-text | 0.19.0 | MIT OR Apache-2.0 | katana_window (text drawn in a drawing) |
| krilla | 0.8.2 | MIT OR Apache-2.0 | katana_window (PDF plots and sheets) |
| png | 0.18.1 | MIT OR Apache-2.0 | katana_window (PNG read and written) |
| image | 0.25.10 | MIT OR Apache-2.0 | katana_window (images in drawings and sheets) |
| arboard | 3.7.0 | MIT OR Apache-2.0 | katana_window (the desktop clipboard) |
| rfd | 0.17.2 | MIT | katana_window (the desktop's file dialogs) |
| epaint_default_fonts | 0.36.2 | (MIT OR Apache-2.0) AND OFL-1.1 AND Ubuntu-font-1.0 | epaint (egui's embedded UI fonts, Ubuntu-Light, Hack and Noto Emoji: font data; whether the GPL-3.0 rule applies to it, and which font the window carries, is the owner's decision, docs/rust.md "Not done") |
| accesskit_winit, codespan-reporting, gethostname, gl_generator, glutin, glutin_egl_sys, glutin_glx_sys, glutin_wgl_sys, khronos_api, spirv, unicode-general-category, unicode-linebreak, winit | 0.32.2, 0.13.1, 1.1.0, 0.14.0, 0.32.3, 0.7.1, 0.6.1, 0.6.1, 3.1.0, 0.4.0+sdk-1.4.341.0, 1.1.0, 0.1.5, 0.30.13 | Apache-2.0 | the window's crates above |
| dpi | 0.1.2 | Apache-2.0 AND MIT | the window's crates above |
| ryu | 1.0.23 | Apache-2.0 OR BSL-1.0 | the window's crates above |
| self_cell | 1.3.0 | Apache-2.0 OR GPL-2.0-only | the window's crates above |
| async-channel, async-executor, async-io, async-lock, async-process, async-signal, async-task, atomic-waker, atspi, atspi-common, atspi-proxies, bit-set, bit-vec, blocking, color, concurrent-queue, event-listener, event-listener-strategy, fastrand, fearless_simd, futures-lite, glifo, idna_adapter, indexmap, kurbo, linebender_resource_handle, nohash-hasher, parking, peniko, pin-project, pin-project-internal, pin-project-lite, polling, portable-atomic, portable-atomic-util, rustc-hash, rustc-hash, simd_cesu8, swash, utf8_iter, uuid, vello_common, vello_cpu, yazi, zeno | 2.5.0, 1.13.3, 2.6.0, 3.4.2, 2.5.0, 0.2.14, 4.7.1, 1.1.2, 0.29.0, 0.13.0, 0.13.0, 0.10.0, 0.9.1, 1.7.0, 0.3.3, 2.5.0, 5.4.2, 0.5.4, 2.5.0, 0.4.1, 2.6.1, 0.2.0, 1.2.2, 2.14.2, 0.13.1, 0.1.1, 0.2.0, 2.2.1, 0.6.1, 1.1.13, 1.1.13, 0.2.17, 3.11.0, 1.15.0, 0.2.8, 1.1.0, 2.1.3, 1.2.0, 0.2.10, 1.0.4, 1.29.0, 0.1.0, 0.1.0, 0.2.1, 0.3.3 | Apache-2.0 OR MIT | the window's crates above |
| linux-raw-sys, linux-raw-sys, rustix, rustix, wasip2, wit-bindgen | 0.12.1, 0.4.15, 0.38.44, 1.1.6, 1.0.4+wasi-0.2.12, 0.57.1 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | the window's crates above |
| arrayref | 0.3.9 | BSD-2-Clause | the window's crates above |
| zerocopy, zerocopy-derive | 0.8.63, 0.8.63 | BSD-2-Clause OR Apache-2.0 OR MIT | the window's crates above |
| tiny-skia-path | 0.12.0 | BSD-3-Clause | the window's crates above |
| moxcms, pxfm | 0.8.1, 0.1.30 | BSD-3-Clause OR Apache-2.0 | the window's crates above |
| num_enum, num_enum_derive | 0.7.6, 0.7.6 | BSD-3-Clause OR MIT OR Apache-2.0 | the window's crates above |
| clipboard-win, error-code | 5.4.3, 3.4.0 | BSL-1.0 | the window's crates above |
| libloading | 0.8.9 | ISC | the window's crates above |
| android-properties, block2, block2, bytes, calloop, calloop, calloop-wayland-source, calloop-wayland-source, cfg_aliases, color_quant, combine, core_maths, crunchy, dispatch, dlib, endi, fax, float-cmp, fontconfig-parser, fontdb, glutin-winit, harfrust, harfrust, imagesize, libm, libredox, memoffset, objc-sys, objc2, objc2, objc2-app-kit, objc2-cloud-kit, objc2-contacts, objc2-core-data, objc2-core-image, objc2-core-location, objc2-encode, objc2-foundation, objc2-foundation, objc2-link-presentation, objc2-metal, objc2-quartz-core, objc2-symbols, objc2-ui-kit, objc2-uniform-type-identifiers, objc2-user-notifications, orbclient, ordered-float, phf, phf_generator, phf_macros, phf_shared, quick-xml, redox_syscall, redox_syscall, redox_syscall, rustybuzz, simd-adler32, slab, smithay-client-toolkit, smithay-client-toolkit, smithay-clipboard, strict-num, synstructure, tiff, tokio, tracing, tracing-attributes, tracing-core, uds_windows, wayland-backend, wayland-client, wayland-csd-frame, wayland-cursor, wayland-protocols, wayland-protocols-experimental, wayland-protocols-misc, wayland-protocols-plasma, wayland-protocols-wlr, wayland-scanner, wayland-sys, winnow, x11-dl, xcursor, xkbcommon-dl, xml-rs, zbus, zbus-lockstep, zbus-lockstep-macros, zbus_macros, zbus_names, zbus_xml, zcheapstr, zvariant, zvariant_derive, zvariant_utils | 0.2.2, 0.5.1, 0.6.2, 1.12.1, 0.13.0, 0.14.5, 0.3.0, 0.4.1, 0.2.2, 1.1.0, 4.6.9, 0.1.1, 0.2.4, 0.2.0, 0.5.3, 1.1.1, 0.2.7, 0.9.0, 0.5.8, 0.23.0, 0.5.0, 0.12.0, 0.5.2, 0.14.0, 0.2.16, 0.1.25, 0.9.1, 0.3.5, 0.5.3, 0.6.5, 0.2.2, 0.2.2, 0.2.2, 0.2.2, 0.2.2, 0.2.2, 4.1.0, 0.2.2, 0.3.2, 0.2.2, 0.2.2, 0.2.2, 0.2.2, 0.2.2, 0.2.2, 0.2.2, 0.3.55, 5.5.1, 0.13.1, 0.13.1, 0.13.1, 0.13.1, 0.41.0, 0.4.1, 0.5.18, 0.9.5, 0.20.1, 0.3.10, 0.4.12, 0.19.2, 0.20.0, 0.7.3, 0.1.1, 0.14.0, 0.11.3, 1.54.1, 0.1.44, 0.1.31, 0.1.36, 1.2.1, 0.3.17, 0.31.15, 0.3.0, 0.31.14, 0.32.13, 20250721.0.1, 0.3.12, 0.3.12, 0.3.12, 0.31.11, 0.31.11, 1.0.4, 2.21.0, 0.3.11, 0.4.2, 0.8.29, 5.19.0, 0.5.2, 0.5.2, 5.19.0, 4.3.5, 5.2.1, 1.1.0, 5.15.1, 5.15.1, 4.2.0 | MIT | the window's crates above |
| accesskit, accesskit_atspi_common, accesskit_consumer, accesskit_consumer, accesskit_consumer, accesskit_macos, accesskit_unix, accesskit_windows, ahash, android-activity, android_system_properties, arrayvec, as-raw-xcb-connection, ash, async-broadcast, async-recursion, async-trait, base64, bitflags, bitflags, bumpalo, cgl, core-foundation, core-foundation-sys, core-graphics, core-graphics-types, crc32fast, crossbeam-utils, displaydoc, document-features, downcast-rs, duplicate, ecolor, egui-wgpu, egui-winit, egui_glow, either, emath, enum-map, enum-map-derive, enumflags2, enumflags2_derive, epaint, errno, euclid, fdeflate, flate2, font-types, font-types, foreign-types, foreign-types-macros, foreign-types-shared, form_urlencoded, futures-core, futures-io, futures-macro, futures-task, futures-util, getrandom, getrandom, gif, gpu-allocator, guillotiere, half, hashbrown, heck, hermit-abi, hex, idna, image-webp, itertools, itoa, jni, jni-macros, jni-sys, jni-sys, jni-sys-macros, jobserver, js-sys, khronos-egl, litrs, lock_api, log, memmap2, naga, naga-types, ndk, ndk-context, ndk-sys, once_cell, ordered-stream, parking_lot, parking_lot_core, pdf-writer, percent-encoding, piper, pkg-config, plain, polycool, presser, proc-macro-crate, proc-macro2-diagnostics, profiling, quick-error, range-alloc, rangemap, raw-window-metal, read-fonts, read-fonts, read-fonts, renderdoc-sys, roxmltree, rustversion, scoped-tls, scopeguard, serde_repr, signal-hook-registry, simdutf8, siphasher, skrifa, skrifa, skrifa, smol_str, smol_str, stable_deref_trait, static_assertions, subsetter, sys-locale, tempfile, thiserror, thiserror, thiserror-impl, thiserror-impl, toml_datetime, toml_edit, toml_parser, ttf-parser, type-map, unicode-bidi, unicode-bidi-mirroring, unicode-ccc, unicode-properties, unicode-script, unicode-segmentation, unicode-width, url, wasm-bindgen, wasm-bindgen-futures, wasm-bindgen-macro, wasm-bindgen-macro-support, wasm-bindgen-shared, web-sys, web-time, webbrowser, weezl, wgpu-core, wgpu-core-deps-apple, wgpu-core-deps-emscripten, wgpu-core-deps-wasm, wgpu-core-deps-windows-linux-android, wgpu-hal, wgpu-naga-bridge, wgpu-types, windows, windows-collections, windows-core, windows-future, windows-implement, windows-interface, windows-numerics, windows-result, windows-strings, windows-sys, windows-sys, windows-sys, windows-targets, windows-threading, windows_aarch64_gnullvm, windows_aarch64_msvc, windows_i686_gnu, windows_i686_gnullvm, windows_i686_msvc, windows_x86_64_gnu, windows_x86_64_gnullvm, windows_x86_64_msvc, write-fonts, x11rb, x11rb-protocol, xmp-writer | 0.24.1, 0.18.1, 0.35.0, 0.36.0, 0.38.0, 0.26.3, 0.21.1, 0.32.1, 0.8.12, 0.6.1, 0.1.6, 0.7.8, 1.0.1, 0.38.0+1.3.281, 0.7.2, 1.2.0, 0.1.92, 0.22.1, 1.3.2, 2.13.2, 3.20.3, 0.3.2, 0.9.4, 0.8.7, 0.23.2, 0.1.3, 1.5.3, 0.8.23, 0.2.7, 0.2.12, 1.2.1, 2.0.1, 0.36.2, 0.36.2, 0.36.2, 0.36.2, 1.19.0, 0.36.2, 2.7.3, 0.17.0, 0.7.12, 0.7.12, 0.36.2, 0.3.14, 0.22.14, 0.3.7, 1.1.10, 0.11.3, 0.12.7, 0.5.0, 0.2.4, 0.3.1, 1.2.2, 0.3.34, 0.3.34, 0.3.34, 0.3.34, 0.3.34, 0.3.4, 0.4.3, 0.14.2, 0.28.0, 0.7.0, 2.7.1, 0.17.1, 0.5.0, 0.5.3, 0.4.3, 1.1.0, 0.2.4, 0.15.0, 1.0.18, 0.22.4, 0.22.4, 0.3.1, 0.4.1, 0.4.1, 0.1.35, 0.3.106, 6.0.0, 1.0.1, 0.4.14, 0.4.34, 0.9.12, 30.0.1, 30.0.1, 0.9.0, 0.1.1, 0.6.0+11769913, 1.21.4, 0.2.0, 0.12.5, 0.9.12, 0.15.0, 2.3.2, 0.2.5, 0.3.34, 0.2.3, 0.4.0, 0.3.1, 3.5.0, 0.10.1, 1.0.18, 2.0.1, 0.1.5, 1.8.0, 1.1.0, 0.37.0, 0.39.2, 0.41.0, 1.1.0, 0.20.0, 1.0.23, 1.0.1, 1.2.0, 0.1.21, 1.4.8, 0.1.5, 1.0.4, 0.40.0, 0.42.1, 0.44.0, 0.2.2, 0.3.7, 1.2.1, 1.1.0, 0.2.6, 0.3.2, 3.27.0, 1.0.69, 2.0.21, 1.0.69, 2.0.21, 1.1.2+spec-1.1.0, 0.25.17+spec-1.1.0, 1.1.5+spec-1.1.0, 0.25.1, 0.5.1, 0.3.18, 0.4.0, 0.4.0, 0.1.4, 0.5.8, 1.13.3, 0.2.3, 2.5.8, 0.2.129, 0.4.79, 0.2.129, 0.2.129, 0.2.129, 0.3.106, 1.1.0, 1.2.5, 0.1.12, 30.0.1, 30.0.1, 30.0.1, 30.0.1, 30.0.1, 30.0.1, 30.0.1, 30.0.1, 0.62.2, 0.3.2, 0.62.2, 0.3.2, 0.60.2, 0.59.3, 0.3.1, 0.4.1, 0.5.1, 0.52.0, 0.59.0, 0.61.2, 0.52.6, 0.2.1, 0.52.6, 0.52.6, 0.52.6, 0.52.6, 0.52.6, 0.52.6, 0.52.6, 0.52.6, 0.48.1, 0.13.2, 0.13.2, 0.3.3 | MIT OR Apache-2.0 | the window's crates above |
| r-efi, r-efi | 5.3.0, 6.0.0 | MIT OR Apache-2.0 OR LGPL-2.1-or-later | the window's crates above |
| cursor-icon, glow, raw-window-handle, xkeysym, zune-core, zune-jpeg | 1.2.0, 0.17.0, 0.6.2, 0.2.1, 0.5.3, 0.5.15 | MIT OR Apache-2.0 OR Zlib | the window's crates above |
| miniz_oxide | 0.9.1 | MIT OR Zlib OR Apache-2.0 | the window's crates above |
| icu_collections, icu_locale_core, icu_normalizer, icu_normalizer_data, icu_properties, icu_properties_data, icu_provider, litemap, potential_utf, tinystr, writeable, yoke, yoke-derive, zerofrom, zerofrom-derive, zerotrie, zerovec, zerovec-derive | 2.4.0, 2.4.0, 2.4.0, 2.4.0, 2.4.0, 2.4.0, 2.4.0, 0.8.3, 0.1.6, 0.8.5, 0.6.4, 0.8.4, 0.8.4, 0.1.8, 0.1.8, 0.2.5, 0.11.9, 0.11.7 | Unicode-3.0 | the window's crates above |
| byteorder-lite, memchr, same-file, termcolor, walkdir, winapi-util | 0.1.0, 2.8.3, 1.0.6, 1.4.1, 2.5.0, 0.1.11 | Unlicense OR MIT | the window's crates above |
| slotmap, zlib-rs | 1.1.1, 0.6.8 | Zlib | the window's crates above |
| bytemuck_derive, dispatch2, objc2-app-kit, objc2-core-foundation, objc2-core-graphics, objc2-io-surface, objc2-metal, objc2-quartz-core, objc2-ui-kit, tinyvec | 1.12.1, 0.3.1, 0.3.2, 0.3.2, 0.3.2, 0.3.2, 0.3.2, 0.3.2, 0.3.2, 1.13.3 | Zlib OR Apache-2.0 OR MIT | the window's crates above |

Every licence named is compatible with GPL-3.0-or-later: Apache-2.0 is
compatible with GPL version 3, and MIT, BSD-2-Clause, BSD-3-Clause, ISC,
Zlib, 0BSD, BSL-1.0, the Unlicense and Unicode-3.0 are permissive. Where a
crate offers a choice, Katana takes it under MIT or Apache-2.0 - including
the crates that also offer GPL-2.0-only or LGPL-2.1-or-later, which are
not taken.

**The window's crates** (step 7, from rust-port's prelude of 2026-11-11):
the rows from egui on. Their licences are crates.io's `license` field for
each locked version, read on 2026-11-11 (recorded). Every one was published
on or before 2026-10-28, two weeks before it was locked; the newer releases
cargo first chose were pinned back, and `tools/build_visible.sh` builds
`--locked`. The lock is resolved for every target, so the rows include
crates compiled only for Windows, macOS, Android or WebAssembly, and
build-time crates (proc macros and build scripts); none of them is compiled
into a Linux build that does not need it. egui's embedded fonts
(`epaint_default_fonts`) are font data under the SIL Open Font Licence 1.1
and the Ubuntu Font Licence 1.0: whether the GPL-3.0 rule applies to them,
and which font the window carries, waits on the owner (docs/rust.md, "Not
done"); nothing ships before step 8.

**Code translated into the Rust port.** The port reproduces the C++
program's behaviour exactly, and in a few places that meant translating a
third party's code into Rust rather than calling it: the translation is a
copy of that code in another language, so it keeps that code's copyright and
licence. Each file names its source at the top; this table lists them
(verified: the copyright lines were read in the installed headers of
MSYS2's packages on 2026-11-08, but fdlibm's, which are not installed and
are quoted as fdlibm distributes them).

| Translated code | Version | Licence | Rust file | What it asks of Katana |
|---|---|---|---|---|
| nlohmann/json: the lexer, parser, serializer, value rules and Grisu2 number text | 3.12.0 | MIT (Copyright (c) 2013-2025 Niels Lohmann; to_chars.hpp also Copyright (c) 2009 Florian Loitsch) | `rust/crates/katana_core/src/json.rs`, `json/sax.rs`, `json/serializer.rs`, `json/to_chars.rs` | Keep the copyright and permission notice (below). |
| fdlibm `e_remainder.c` (`__ieee754_remainder`) | as distributed in fdlibm 5.3 and its descendants | Sun's notice: permission to use, copy, modify and distribute, provided the notice is preserved | `rust/crates/katana_math/src/cpp.rs` (`remainder`) | Preserve the notice, which the file does word for word. |
| Eigen: ColPivHouseholderQR, Householder, HouseholderSequence | 3.4 | MPL-2.0 (Copyright (C) 2008-2010 Gael Guennebaud and Benoit Jacob) | `rust/crates/katana_survey/src/lsq_core.rs` | The translated file's source stays available under MPL-2.0, with its notice. Eigen's files carry no "Incompatible With Secondary Licenses" notice, so MPL-2.0 section 3.3 lets the larger work be distributed under the GPL. Recorded; whether a translation counts as a Modification of the covered files is unverified (section 7). |
| CGAL: Constrained_triangulation_plus_2, Constrained_Delaunay_triangulation_2, Triangulation_2 (the perturbed in-circle test) | 6.2 | GPL-3.0-or-later (Copyright (c) 1997-2012 INRIA Sophia-Antipolis) | `rust/crates/katana_terrain/src/cdt_backend.rs` | The same licence as Katana's; keep the notices. |
| CGAL: Intersections_2/Segment_2_Segment_2 (the crossing point's construction) | 6.2 | LGPL-3.0-or-later (Copyright (c) 2000 Utrecht University, ETH Zurich, INRIA Sophia-Antipolis and others) | `rust/crates/katana_terrain/src/cdt_backend.rs` | Compatible with GPL-3.0-or-later; keep the notice. |
| libstdc++: `uniform_int_distribution`, `generate_canonical`, `std::clamp` | GCC 16.2 | GPL-3.0-or-later with the GCC Runtime Library Exception (Copyright (C) Free Software Foundation, Inc.) | the tests' random engines (`tests/support` of geometry, render, entity, dxf, surveyio, terrain and others) and `katana_math::cpp::clamp` | The same licence as Katana's. |

nlohmann/json's copyright and permission notice, as its `LICENSE.MIT` gives it:

```text
MIT License 

Copyright (c) 2013-2025 Niels Lohmann

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## 6. The Linux, macOS and Windows ARM64 packages

Not audited. The Linux and macOS packages are built against the conda-forge
packages named in `tools/setup_linux_toolchain.py` (`qt6-main`, `cgal-cpp`,
`eigen` 3.4, `proj`, `gdal`, `pdal`, `nlohmann_json`, `openssl`, `zlib`,
`libsqlite`, `gmp`, `mpfr`, `libboost-headers`), and the libraries
`KatanaDeployUnix.cmake.in` copies beside the programs are those packages'
dependencies, so the list differs from section 3. The Windows ARM64 package
is built from MSYS2's CLANGARM64 packages. Section 1 holds for all
platforms, because it comes from Katana's sources. For the others, each
package's licence is in its `info/licenses` folder and
`conda-meta/<package>.json` inside the toolchain prefix; the packaging step
should generate this table from them for each platform and add it to the
package.

## 7. Not verified

* The versions and licences in section 3 are MSYS2's records, not read from each
  source. The headers of x264, x265, poppler, librttopo, libheif, GMP, MPFR and
  Qt were read, as the notes say.
* Whether the Rust translation of Eigen's QR (section 5, "Code translated into the
  Rust port") is a Modification of Eigen's covered files under MPL-2.0, and so
  must itself stay available under MPL-2.0; the notice is kept either way.
* The Windows-only Rust crates of section 5 (`windows-sys` and its target crates)
  are not yet downloaded, because nothing of the port is built on Windows before
  its step 8: their licences are not yet read from the crates.
* `jbigkit`: read in the 2.1 sources of Debian and a mirror, not in the MSYS2
  archive (see the table).
* The EPSG registry terms behind `proj.db`, and which licence covers the
  certificate bundle file.
* Everything in section 6.
* Qt's own licensing pages (doc.qt.io and qt.io) refused automated access
  (HTTP 403); the Qt licence is taken from the 6.11.2 headers on the build
  machine.
* Whether the packages carry the licence texts, the notices and the written offer
  of source that sections 1 and 2 call for. They do not yet: `LICENSE` is the only
  file the install rules copy. This is for packaging to do; `LICENSING.md`,
  "What is still to do", lists it.
