# The Rust port

On 2026-11-05 the owner decided that Katana moves to Rust: the whole of it,
the window included. This document is the port's record - the decisions and
their reasons, the workspace and its two toolchains, the rules every port
follows, what is ported, the measurements and what is not done. It is updated
in the commit that changes any of them.

The C++ program stays the product while the port is made, and it is the
oracle the port is checked against. The rewrite runs beside it, bottom-up by
layer, and the programs are cut over one at a time: no C++ is deleted until
the Rust that replaces it has passed its gate and shipped.

## The owner's decisions (2026-11-05)

| Question | Decision | What follows |
|---|---|---|
| What the window becomes | Rebuilt in Rust with egui and wgpu | No C++ ever calls Rust, so nothing bridges them that way. Printing becomes a PDF printed by the operating system. |
| C++ while the port runs | Rust first for new work | C++ takes fixes only. A fix to a layer already ported lands in its Rust twin in the same commit (`rust_parity` refuses it otherwise); a new feature waits for its layer and is then written in Rust alone. |
| How the work is run | Parallel agents, one per crate | Each agent ports one crate in a worktree of its own; one integration build and whole-suite run per step. |
| Where Rust is built | In WSL only, until the port is a fully working version; then it is tested on Windows | Building, testing and the comparison with the C++ all run in WSL. Nothing is run on Windows meanwhile - and so nothing ships, since Windows is where Katana ships first and where the Windows-only code (the updater's swap, Credential Manager, wide paths) can only be tested. The owner first ruled "WSL every day, Windows at every gate", and narrowed it the same day: "i dont want to run anything on windows at this stage just build for wsl then once we have a fully working version we test on windows". |

Rejected for the window: keeping the Qt window in C++ over a Rust engine (not
a port of the whole), Slint (weak for docking and a large custom canvas), and
Qt Quick through cxx-qt (still Qt and a C++ toolchain). Rejected for the
build: the MSVC Rust installed on the owner's machine (it cannot link the
MSYS2 builds of GDAL, PDAL and PROJ the Windows build uses), rustup's GNU
target (its MinGW runtime is msvcrt, which must not be mixed with UCRT) and
cross-compiling Windows programs from WSL (the same mismatch).

## The order of the work

Bottom-up by layer, because a layer's tests can only run once what it stands
on exists; programs cut over one at a time, each behind evidence that it
answers as the C++ does.

| Step | Crates | C++ test cases | Its gate, in WSL |
|---|---|---|---|
| 0 | the workspace, the toolchain, the checks | - | the C++ Linux suite and the cargo stage green, the GDAL spike passes |
| 1 | core, math, geometry | 398 | the cases twinned, the number formats equal the C++'s on random doubles |
| 2 | geodesy, survey, terrain, render, entity, commands, update, updater | 2,094 | the cases twinned; the updater's end-to-end tests on Linux |
| 3 | storage, archive12d, dxf, ifc, surveyio | 1,124 | every file of the test data and samples read and written by both, each reading the other's |
| 4 | cad | 3,011 | the cases twinned; every verb and alias accepted |
| 5 | gis, pointcloud, interop | 744 | the cases twinned, the online tests too |
| 6 | app | 241 | the 375 cli tests pass against the Rust katana_cli; the replies and saved projects of both programs compared |
| 7 | window, gpu | 1,609 | the 285 headless window tests pass against the Rust window |
| 8 | Windows | - | the whole of the above on Windows, natively; the updater applies, rolls back and recovers a real update there |
| 9 | - | - | a release on the Rust programs, one release cycle, then the C++ is removed |

Nothing ships before step 8: the owner's ruling keeps the port off Windows
until it works, and Windows is where Katana ships first. The case counts are
what `tools/check_rust_parity.py --list` printed on 2026-11-05 (9,236 in all);
the step's own run is the evidence, not this table.

## Where it stands

| Step | State |
|---|---|
| 0 | done: the workspace of 23 empty crates, the WSL toolchain, `rust_layering`, `rust_parity`, the visible build's cargo stage and WSL mode, the GDAL spike |
| 1 | done in WSL: core, math and geometry, all 398 C++ cases twinned, plus core's 11 task-pool cases from the render suite; the Release build, Clippy, numeric oracle and whole suite pass |
| 2 | in progress: wave A done in WSL - geodesy and render whole, the entity model, the terrain TIN core on spade, the survey core and its subsurface tools, the update layer's leaves and machine seams (902 of the step's 2,094 cases, 869 run on Linux); waves B and C to come |
| 3-9 | not started |

### Step 1: the foundation crates

The port keeps the C++ header boundaries as Rust modules, with the public
types re-exported at each crate's root. Native `Result` and `Option` express
failure and absence; matrices retain their row-major storage and column-vector
composition order. The three-argument vector length uses libstdc++'s scaled
calculation, not chained two-argument hypot calls, because the rounding order
is observable. The existing AVX2 and NEON text, point and splat kernels have
Rust implementations with runtime dispatch and the scalar comparison tests.

`rust/parity.toml` now holds `core`, `math` and `geometry` to their 126, 55 and
217 cases respectively. The 11 task-pool cases remain owned by the C++ render
test directory but have their Rust twins in core, where the pool lives. No
whole case is exempted. Test values and tolerances come from the C++ tests;
additional tests exercise Rust ownership and panic cleanup. In particular,
workers retain surplus panic payloads until completion: dropping a payload
whose destructor also panics must not kill a worker and strand a waiting job.
Restoring worker-side disposal made the regression hit its ten-second
deadlock watchdog; restoring the repair made it pass and reuse the workers.
The property-test generators follow GCC 16's integer sample assembly and
redraw a fraction rounded to one, rather than clamping it. A forced
maximum-integer sample proves that branch is reached and rejects the old
clamp.

The number-format gate is `rust/crates/katana_core/tests/number_oracle.rs`,
against the test-only C++ executable built from
`tests/core/number_format_oracle.cpp`. Its 28,942 bit patterns cover both
signs of every binary exponent, boundary mantissas, half-integer rounding,
signed zero, infinities, NaNs and 16,384 deterministic random samples. The
random sequence is the xorshift sequence and seed already used by the C++
point-batch tests. Each input compares exact text, twelve significant digits,
and six fixed precisions including both sides of the precision clamp: 231,536
byte-for-byte comparisons. This is an oracle comparison, not a Rust
formatter's output saved as an expectation.

The gate found a shortest-decimal half tie: binary bits `3e60000000000000`
were written as `2.9802322387695313e-08` by Rust and
`2.9802322387695312e-08` by C++. The exact formatter now detects equal-distance
decimal candidates using integer factors of two and five, chooses the even
final digit, and checks that the adjusted text still round-trips. Fixed
integers retain every exact digit; using ordinary Rust display for every
value was rejected because it does not have this wire-format contract.

The visible build sets `KATANA_NUMBER_FORMAT_ORACLE` to the helper in the
C++ build's `bin/tests` directory. A missing helper fails the Rust test;
`wsl cargo` therefore needs a preceding full WSL build. It never silently
skips the cross-language gate. `rust/clippy.toml` and the workspace lint
reject direct `to_string` calls.
Because that method is generic, the guard deliberately rejects integer and
string calls too; integers use explicit formatting and strings use owned
copies. The cargo stage proves the rule on
`tests/rust_check_fixtures/float_to_string`, requiring the named diagnostic
and a nonzero exit. Formatting macros still require the same review as C++
streams; this guard targets direct `to_string`, as its C++ counterpart does.

Geometry explicitly allows Clippy's negated-partial-comparison lint so that
the C++ conditions continue to reject NaNs; replacing `!(value > 0)` with
`value <= 0` would change behaviour. Oracle decimal literals and test-suite
module names likewise retain their spelling with narrow, reasoned lint
exceptions. Fresh child processes test scalar, detected-native and invalid
SIMD environment requests. A kernel-parity run requires a real kernel:
scalar-against-scalar is not credited as native coverage. Unlike gtest's
runtime skip, the Rust parity gate fails on a scalar-only test host; the
library itself still supports scalar execution.

This is library work, not a program cut-over. The window, CLI and MCP still
run the C++ implementation; no Rust drawing operation is exposed yet.

Final integration evidence on 2026-11-05: Release C++ build and Rust Clippy,
build and tests in Ubuntu-24.04; the complete visible test phase exited 0.
CTest had 9,914 entries: 9,874 passed, 40 skipped, none failed. Its skips
cover unavailable reference files, hardware/display and online cases, and
permission checks unavailable to the root user. The cargo-stage aggregate
was 430 passed, zero failed and zero ignored, including child-process test
summaries. All 231,536 number-format comparisons passed. No Windows run or
program cut-over is claimed by this evidence.

### Step 2: the domain kernels

Step 2 is run in waves, split by what each module and each C++ test file
includes: an agent may build only on what is already integrated, because
under the owner's no-build rule it cannot compile its code, and code written
against another agent's unbuilt API is a guess. Each wave is merged, built
once, fixed and committed before the next begins.

| Wave | What | Cases |
|---|---|---|
| A | geodesy; render; the entity model (all but the customisation libraries); terrain's TIN core (all but the volumes); survey's core and its subsurface tools (all but the reduction); update's version, ranges, inflate, paths, credential, file operations, fault injection, registry and process seams | 902 |
| B | entity's customisation libraries; commands; terrain's overlay and volumes; survey's reduction; update's install, journal, state, feed, manifest, signature, trust, plan and check | - |
| C | update's stage, apply, recover and contract, with the tests that need the apply harness; the updater | - |

Wave A's seven agents wrote 73,540 lines no one had compiled: 44,357 of
library and 29,099 of tests. The
first build of the merged tree type-checked all six libraries; what failed
was 18 Clippy findings, then 27 `.err().expect()` calls, two names a test
module shadowed and an untyped float in the tests. The first test run
passed: 1,302 passed, none failed (430 from step 1, and 872 new - the 902
twins less the 33 Windows-only credential cases, plus three extra tests).
Two of Clippy's suggestions were not taken because they would change
behaviour: eager construction of a tile's 4,096-byte mask for every point,
and an auto-deref that would borrow through `&mut` where the C++'s copy of
the reference is meant.

**CGAL to spade: decided for spade.** The plan's rule was spade if the
terrain tests pass unchanged, else a C++ shim of the CGAL backend. They pass
unchanged: all 185 TIN-core cases, among them the 89 TIN-edit cases. What
made it possible: CGAL's constrained Delaunay triangulation flips with a
symbolically perturbed in-circle test, so its result for a given set of
vertices and constraints is unique and independent of insertion order - the
spatial sort before insertion only makes it faster. The Rust
(`katana_terrain`'s `cdt_backend.rs`) splits crossing constraints as
`Constrained_triangulation_plus_2::insert_subconstraint` does, with CGAL's
own construction of the crossing point and its four-ulp snap; triangulates
with spade; and flips with CGAL's perturbed predicate, ported line for line,
to CGAL's unique triangulation. Not reproduced: CGAL's check that a crossing
it computed lies in a triangle beside the crossed edge, with its exact
recomputation when it does not - it can only matter for crossings within
rounding of an end. The overlay and volume cases of wave B test the kernel
further.

**Text in the model is validated UTF-8.** The entity layer holds `String`,
by the rule "bytes up to the decoder": a C++ case that stores non-UTF-8 text
in the model and refuses it later keeps its UTF-8 half as its twin, and the
readers of stored bytes - the geometry blob - still refuse such bytes. Two
cases whose only subject is that refusal are exempt (`rust/parity.toml`), as
are a moved-from transformer and a change of the floating-point rounding
mode, which Rust cannot express.

Wave A's integration evidence, 2026-11-07, `tools/build_visible.sh wsl` on
the merged tree: exit 0; the Release C++ build, Clippy with `-D warnings`
and the float-to-text guard clean; CTest 9,914 entries, 9,874 passed, 40
skipped as before, none failed, `rust_parity` holding geodesy and render as
ported; cargo test 1,302 passed, none failed, none ignored. No Windows run is
claimed: the update layer's Windows code is written and not compiled.

## The workspace

`rust/Cargo.toml` is one Cargo workspace, with one crate per layer of
`tools/check_layering.cmake` in `rust/crates/`, named `katana_<layer>`. Two
names differ: the qt layer's crate is `katana_window` (the window is not Qt
in the port), and the helper program `katana_updater` is a crate of the update
layer, as `src/katana_updater` is.

A crate's `katana_*` dependencies are exactly the layers the table allows it,
all of them, declared now. A crate cannot name a crate it does not depend on,
so the layering rule holds by construction; and with every allowed dependency
declared up front, an agent porting one crate never edits another's
dependency list. Unused path dependencies cost nothing.
`tools/check_rust_layering.py` (the `rust_layering` test) keeps the crates and
the table in step while both exist: every layer has a crate, no crate depends
on a layer the table does not allow it, and no crate re-exports a third-party
crate (`pub use`), which would put a third-party type in its public
interface - the table's second rule. Dev-dependencies are not checked, as
`tests/` is not in the C++ check.

Edition 2024; `rust-version` 1.98, the older of the two compilers. `unsafe`
is denied in the workspace's lints and allowed only where a module must be
unsafe - an FFI adapter, an OS call, a SIMD kernel, or the one module that
hands the task pool's tasks disjoint parts of one buffer
(`katana_render`'s `disjoint.rs`: tiles of a framebuffer have no safe slice
form, and every step rests on the pool's contract that each index runs once
and no range overlaps another) - each saying why. Clippy
runs with `-D warnings`, as `-Werror` does for the C++.

## The toolchains

### WSL, where the port is built

The distribution is **Ubuntu-24.04**, the release job's Linux runner. WSL
3.0.3 on the owner's machine had no distribution until 2026-11-05; it now has
two: `Ubuntu-24.04`, installed for the port, and `Ubuntu` (26.04.1, the
default), which the port does not use. The distribution has no Linux user:
it was provisioned as root, non-interactively, and the builds run as root. A
user can be added at any time and becomes the default; the toolchain prefix
under `/opt` is readable by every user.

Set up, as root (`wsl -d Ubuntu-24.04 -u root`):

```sh
apt-get install -y --no-install-recommends python3-venv libegl1 libgl1 libopengl0 \
    libfontconfig1 xvfb mesa-vulkan-drivers libxkbcommon-x11-0 libxcb-cursor0 \
    libxcb-icccm4 libxcb-keysyms1 libxcb-shape0 libxcb-xinerama0 libxcb-randr0 \
    libxcb-render-util0 libxcb-image0          # what the Linux release job installs
python3 -m venv /opt/katana-venv              # Ubuntu 24.04's Python is externally managed
cd /mnt/d/<checkout> && /opt/katana-venv/bin/python tools/setup_linux_toolchain.py
cd /opt/katana-toolchain && mkdir -p bin-tools && for t in cmake ctest cpack ninja \
    cargo rustc rustdoc cargo-clippy clippy-driver rustfmt cargo-fmt; do
    ln -sf ../bin/$t bin-tools/$t; done
```

The compiler comes from the toolchain's own conda-forge snapshot: `rust` is
one of the specs of `tools/setup_linux_toolchain.py`, so WSL, the Linux and
macOS release jobs and a cloud session - whose egress reaches conda-forge
and not the Rust project's own servers - build with the same Rust, as they
build with the same GCC. On 2026-11-05 the snapshot gave rustc 1.98.1 with
cargo, clippy and rustfmt, beside GCC 16.2.0 and GDAL 3.13.2: 291 packages,
5.9 GB in `/opt/katana-toolchain`. Only the build tools are on `PATH`, linked
into `bin-tools/`: the prefix's `bin/` also holds a python3 that would shadow
the system's (the release job does the same for CMake's tools).

The sources stay in the worktree on `D:`, where git and the IDE work. Git is
never run inside WSL: a worktree made by Windows git names its repository by
a `D:/` path that Linux cannot open, and nothing in the build calls git. The
builds are on WSL's own disk, in `~/katana-build/<worktree>/` - the C++ in
`linux-release/`, cargo's target in `cargo/` - because a build on the
Windows drive, through `/mnt/d`, is slow.

Fixture tests that need POSIX modes must recreate their input in a native
temporary directory too. The first full step-1 run passed the Rust stage
but failed `update_tools_make_manifest`: Git's non-executable fixture files
appeared executable on the Windows mount. Its existing temporary-tree helper
now supplies the same bytes with native modes; the manifest expectations and
producer are unchanged (`docs/update.md`, "The pieces").

cargo links with the prefix's GCC and its sysroot (glibc 2.28), as CMake
builds with them, and writes the prefix's `lib/` into each program's run
path, so a test finds GDAL without `LD_LIBRARY_PATH`. `tools/build_visible.sh`
sets both; a crate that links a C library names the prefix as a search path
from `KATANA_TOOLCHAIN` (as `rust/crates/katana_gis/build.rs` does).

The build in WSL has a `PATH` of its own: the prefix's build tools, then the
system's directories, and nothing of Windows. WSL appends the Windows `PATH`
to Linux's, and CMake's `find_package` searches the prefixes of `PATH`; the
first configure there, on 2026-11-05, took MSYS2's Windows CURL package
through `/mnt/c/msys64/ucrt64` and failed on its missing Brotli. A configure
that has failed so keeps the Windows path in its cache (`CURL_DIR`), so the
cure includes deleting that build directory, as was done.

### Windows, once the port works

MSYS2's `mingw-w64-ucrt-x86_64-rust`, **1.99.0-1**, built against LLVM 22.
The newest build of the same version, 1.99.0-2, was made against LLVM 23 and
does not start on the owner's toolchain, whose `llvm-libs` is 22 - and
`llvm-libs` is also what MSYS2's Arrow, which GDAL links, was built against,
so moving it alone would leave GDAL unable to load Arrow. A partial upgrade of
MSYS2 was rejected; the next whole `pacman -Syu` moves Rust and LLVM together.
The package came from an MSYS2 mirror (`repo.msys2.org` reset the
connection); its signature, by an MSYS2 development key, was checked against
pacman's keyring with gpg before it was installed with `pacman -U`.

It targets `x86_64-pc-windows-gnu` against UCRT: a program it builds imports
only `api-ms-win-crt-*` and system DLLs, never msvcrt. Its cargo is the first
on `PATH` in a build window, because `/c/msys64/ucrt64/bin` is.

It was installed, and the empty workspace's cargo stage with the GDAL spike
ran green in it once (2026-11-05), before the owner's ruling kept the port
off Windows; nothing has been run on Windows since, and nothing will be until
the port is a fully working version (step 8). The Windows ARM64 release job
will take `mingw-w64-clang-aarch64-rust` from its CLANGARM64 environment, as
it takes clang ("Not done", below).

## Building and testing

Every build goes through `tools/build_visible.sh`, in a window the owner
watches, and the Rust is part of each run. Until step 8 every run is a `wsl`
one:

```sh
bash tools/build_visible.sh wsl        # linux-release, its suite under xvfb-run, clippy, cargo build and test
bash tools/build_visible.sh wsl cargo  # the Rust workspace alone
bash tools/build_visible.sh wsl test -R Docs   # ctest's arguments, as on Windows
```

The same phases without `wsl` are the Windows run, for step 8.

After the C++ build come clippy and the workspace's build with warnings as
errors; after ctest, `cargo test`, whose counts the log sums into one line
(`== cargo test: N passed, M failed, K ignored`). A narrow run - `build` with
targets, `test` with ctest's arguments - leaves the Rust out, as it leaves the
rest of the C++ out. A WSL run's window, log and status file are on the
Windows side, in `build/wsl/`, and are waited on and read as any other.

## Tests are the specification

Every C++ test case of a layer is ported with the layer, and its expected
values are copied, not recomputed: a ported test that fails means the port is
wrong, until shown otherwise from a source outside both programs (the
cardinal rule of testing). The twin of the gtest case `Suite.CaseName` is a
Rust test function `case_name` in a module `suite` - the names in snake case,
the module usually a `mod` block in a file under the crate's `tests/`. Test
data stays where the C++ reads it, under `tests/`, and is read from there.

`tools/check_rust_parity.py` (the `rust_parity` test) holds this. A test
directory listed as ported in `rust/parity.toml` must have a twin for every
case, a case added later by a fix in C++ included, unless the file exempts the
case with its reason (a misuse that the types now refuse to compile, a reader
the owner chose not to port); an exemption naming a case that does not exist
fails too. `--list` prints each directory's count. The Rust is read rather
than built, stepping over comments, string, raw string and character literals,
so that a brace inside a test's text cannot move a test into another module.

Both checks are shown to see what they claim, on fixtures in
`tests/rust_check_fixtures/` that break each rule: a crate depending on a
layer it may not, one re-exporting `rusqlite`, one that is no layer, a layer
with no crate, a case with no twin, a stale exemption. One fixture must pass:
`rust_parity_skips_literals`, whose braces are all inside literals and
comments. It was checked on 2026-11-05 to fail with each of the four kinds of
literal not stepped over.

## Porting rules

- **Behaviour is ported as it is.** A port that also fixes or redesigns loses
  its oracle. A defect found while porting goes under "Not done" here and is
  fixed after the gate, in both programs, or in Rust alone after the cut-over.
- **Errors.** One `Error` in `katana_core` with the C++'s code, message and
  context and the same `describe()` text; `Result<T>` is
  `std::result::Result<T, Error>` and a status is `Result<()>`. The C++'s
  `BadResultAccess` has no custom twin. Its success and structured-failure
  cases retain their ordinary assertions; misuse is governed by Rust's
  native result API rather than a second result wrapper or a case exemption.
- **Text and numbers.** Bytes up to the decoder, validated UTF-8 after it.
  Each of the reply's number formats must give the C++'s text exactly:
  `core::formatExactReal` (the shortest round-trip text, as `std::to_chars`
  writes it - `1e+20`, exact digits for large integers), `core::formatFixed`
  and the twelve significant digits of LIST and INFO. `parseFiniteDouble`
  refuses an underflow such as `1e-999`, where Rust's parse returns zero.
- **Numerics.** Rust never fuses a multiply and an add, as
  `-ffp-contract=off` keeps the C++ from doing; three-argument `hypot` is
  ported as libstdc++'s (scaled by the largest component); output never
  depends on a hash map's order; `math::tolerance` stays the one source of
  tolerances.
- **Parallelism.** The task pool is ported with its contract - nested calls
  run inline, the first panic is rethrown, the result is the serial run's -
  and with the tests that prove it with one thread. rayon was rejected: its
  nesting and panics behave otherwise than the contract the tests hold.
- **SIMD.** The 23 kernel entries are ported with their layers, bit-identical
  to their scalar references, with `KATANA_SIMD` kept. `#[target_feature]`
  confines the AVX2 code to the kernel itself, so the object check the C++
  needs (`tools/check_simd_kernels.cmake`) has no twin.
- **Names.** The naming rule holds in the port: comments, identifiers and
  messages say "the archive format", "survey code file", "style library";
  the kept tokens keep their spelling (`.12da`, the `katana_archive12d`
  module, the persisted `12d.*` keys).

## Third-party libraries

Each stays confined to one crate, as each is to one module in C++, and is the
version the C++ build links, from the same toolchain.

| C++ | Rust | Crate |
|---|---|---|
| nlohmann_json | serde_json to parse (`preserve_order`, `float_roundtrip`); the text is written by a port of nlohmann's own `dump()` and its Grisu2 number text | entity, cad, app, update, interop |
| SQLite | rusqlite on the system library, every pragma kept | storage |
| Eigen (rank-revealing QR, Cholesky) | nalgebra for the dense storage and the Cholesky factor; the column-pivoted QR is the port's own, Eigen 3.4's algorithm step for step | survey |
| PROJ | hand-written declarations of the 57 functions used, on the same libproj | geodesy |
| CGAL (one 385-line file) | spade, if the terrain tests pass unchanged; otherwise that file stays as a C++ shim | terrain |
| GDAL, including its algorithm framework | gdal-sys, and the C algorithm API ("The GDAL algorithm spike") | gis |
| PDAL (C++ only) | a C shim of the one adapter file | pointcloud |
| libcurl, the handle GDAL configures | the curl crate on the same libcurl | gis |
| OpenSSL (Ed25519 verification with the RFC 8032 checks) | ed25519-dalek and curve25519-dalek | update |
| zlib (raw inflate) | miniz_oxide | update |
| Qt | eframe, egui, egui_dock, wgpu, rfd, tiny-skia, krilla, cosmic-text | window |

A crate is added to `THIRD_PARTY_NOTICES.md` in the commit that first uses it,
and must be GPL-3.0-compatible.

Three rows above differ from the plan of 2026-11-05, each for a reason found
in porting:

- **PROJ** is reached as GDAL is in the spike, through declarations of the
  functions the C++ calls and nothing else, with every `#[repr(C)]` layout
  checked by size and offset against PROJ 9.8's headers. Rejected: the
  proj-sys crate, a binding of the whole API made for a PROJ release of its
  own, where 57 declarations checked against the headers of the PROJ the
  build links pin exactly what is used.
- **Eigen's QR**: nalgebra's `ColPivQR` pivots on the column holding the
  largest entry, not the largest norm, and has no rank or threshold, so a
  rank-deficient network would be found so elsewhere than Eigen finds it.
  The port implements `ColPivHouseholderQR` as Eigen 3.4 does it (`lsq_core.rs`):
  norms and the first-largest pivot, the LAWN 176 norm downdate, the rank
  against the prescribed threshold, the solve. It is bit-identical to Eigen
  on systems of up to three equations; beyond that Eigen's SSE2 kernels sum
  in two lanes and the results differ in the last bit or two, inside every
  test's tolerance.
- **nlohmann's text**: serde_json writes numbers with ryu (`1e20`, `1`)
  where nlohmann writes `1e+20` and `1.0`, and sorts keys only when it is
  not built with `preserve_order`, which Cargo's feature unification turns
  on for every crate once one needs insertion order. The entity layer's
  private `json` module therefore keeps nlohmann's object order (sorted, as
  `std::map` holds it) and writes its text with a line-for-line port of
  nlohmann's `to_chars`; serde_json only parses.

## The GDAL algorithm spike

The C++ runs GDAL's algorithm framework through its C++ API
(`src/katana_io/geo/processing.cpp`, 102 allow-listed algorithms), which no
Rust binding wraps; it was the one part of the port with no known route.
GDAL 3.13 exposes the same framework in C (`gdalalgorithm_c.h`). The test
`rust/crates/katana_gis/tests/gdal_algorithm.rs` runs `gdal raster info` on
the geo suite's `tests/geo/data/plane.asc` through that API - the registry,
an algorithm from its path, the arguments parsed, the run, the output read
back - and checks the size it reports against the grid's own header (40
columns, 30 rows). It runs in every cargo stage, on both toolchains, which have
the same GDAL, 3.13.2. Phase 5 replaces its hand-written declarations with
the crate's binding.

## Measurements

| What | Where | Result |
|---|---|---|
| The toolchain prefix, solved and installed | WSL, 2026-11-05 | 291 packages, 5.9 GB |
| The empty workspace: clippy, build, test | WSL, 2026-11-05 | 11 s for the whole cargo stage; 14 s with the GDAL spike |
| The same with the GDAL spike, its first build | Windows, 2026-11-05, before the ruling | 147 s |

## Not done

- **No release job builds the Rust yet.** Nothing in `rust/` ships before
  step 8, and a change to `.github/workflows/release.yml` can only be proven
  by a release run; the cargo steps are added then, when that run is part of
  the gate anyway. The ARM64 job's Rust (CLANGARM64) and the macOS job's (the
  conda-forge snapshot) come with them.
- **Nothing of the port has been tested on Windows** beyond the one cargo
  stage of the empty workspace: the owner's ruling (above). Step 8 is where
  the Windows-only code - the updater's swap, Credential Manager, wide paths -
  is first proven.
- **ARM NEON execution waits for an ARM host.** The current WSL gate runs
  x86-64 and proves AVX2/scalar equivalence; NEON source has not been compiled
  or run in this gate.
- **Oracle edge cases are retained, not repaired during the port.** Static
  review found XML accepting trailing content and surrogate/NUL character
  references, logger fields permitting raw newlines, task-pool range
  arithmetic wrapping near the largest index, and spatial cell-count
  multiplication wrapping. Degenerate closed paths can retain too few
  vertices; extreme splat radii overflow C++ signed arithmetic. Ratio
  reduction has undefined zero/zero and minimum-integer inputs in C++,
  where the Rust port panics. These need separately justified fixes after
  the gate, in both implementations. The alignment prose also contradicts
  the implementation's acceptance of negative radius magnitudes.
- **The WSL distribution has no user of the owner's.** The builds run as root
  there; nothing depends on it.
- **Defects found in porting step 2, ported as they are** (fix after the gate,
  in both programs):
  - geodesy: `proj_info()` and `proj_context_create()` run unguarded in
    `projRuntimeInfo` and `ProjContext::create`'s fallback, though PROJ's
    default context is filled lazily and copied without its lock (the Rust
    serialises both behind one mutex, since its tests share a process);
    `CoordinateReferenceSystem::operator==` is not transitive.
  - render: the clipper's fixed 3+5-vertex buffers assume each plane adds at
    most one vertex, which a near-degenerate polygon rounded to float may not
    keep (an out-of-bounds write in C++, a panic in Rust; not shown to
    happen); `DrawLine::depthBias`'s comment says NDC units where the
    rasteriser applies pixel footprints of view distance.
  - entity: `rewriteLayerPrefix` with an empty `from` drops the name's first
    byte; `labelPiecesFor` and `labelAnchor` on an open polyline with no
    vertices wrap `size() - 1` and divide by zero, and `resolveAnchor` calls
    `front()` on an empty leader (undefined in C++; unreachable for a
    validated entity); `formatDms`/`formatDm` and nlohmann's integer reads
    convert an out-of-range double unchecked; `insidePoint` sorts crossings
    that may hold a NaN.
  - survey: `toString(SurveyTimestamp)` pads a "0" before negative seconds
    under 10; `observationKindName` calls a global GNSS position with
    neither part set "geodetic"; `millimetres` in the quality levels calls
    `std::llround` on a NaN uncertainty (glibc gives the minimum integer, which
    the Rust reproduces; the Windows runtime may not).
  - update: `removeTree` resets the depth for the entry it deferred, so a
    deep tree planted under the journal's name gets another 64 levels;
    `writeAll` loops for ever on a `WriteFile` that succeeds having written
    nothing; `spawn_win`'s `run` leaks the attribute list when
    `UpdateProcThreadAttribute` fails; `makeDirectories` on Windows counts an
    entry that vanished after `ERROR_ALREADY_EXISTS` as made by another
    process; `absoluteNormal` may strip a trailing dot or space before
    `endsInDotOrSpace` sees it (suspected, to be checked on Windows).
- **Where the Rust necessarily differs from the C++** (step 2):
  - Text the model holds is validated UTF-8 (`String`), by the rule "bytes
    up to the decoder": the C++ cases that store non-UTF-8 text in a model
    and refuse it later have their UTF-8 halves twinned, and the readers of
    stored bytes (the geometry blob) still refuse such bytes.
  - Non-UTF-8 bytes in a string PROJ returns become U+FFFD; the C++ keeps
    the bytes.
  - nlohmann's reader and serde_json's disagree at the edges: `-0` reads as
    the real -0.0 (nlohmann: the integer 0), `1e400` is refused as it is
    read (nlohmann reads infinity and the geometry check refuses it), and
    nesting deeper than 128 is refused. A parse error's context after
    nlohmann's prefix is serde_json's text.
  - The raw inflate is miniz_oxide's: a stream that is not deflate is
    refused with "inflate error: <reason>" where zlib gave "zlib error <n>:
    <msg>", and zlib's two internal failures have no counterpart.
  - `std::format` pads report columns by display width, Rust by characters:
    the subsurface reports differ for an id holding wide or combining
    characters.
  - Windows-only code of the update layer (the Credential Manager, the long
    path forms, the Windows file operations and process launch) is written,
    not compiled, until step 8; so is every NEON kernel.
