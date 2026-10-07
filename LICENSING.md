# Licensing

Katana is free software. It is licensed under the GNU General Public License
version 3 or, at your option, any later version (SPDX `GPL-3.0-or-later`), with
additional terms about credit and honesty. Copyright (c) 2026 Jarada.
This page says what that means in plain words and what the licence does not
cover. It is a guide, not legal advice and not part of the licence: the binding
texts are `LICENSE` and `ADDITIONAL_TERMS.md`.

| File | What it holds |
|---|---|
| `LICENSE` | the GNU General Public License, version 3, unmodified, as the Free Software Foundation publishes it |
| `ADDITIONAL_TERMS.md` | the additional terms, added under section 7 of that licence, and the notice for source files |
| `THIRD_PARTY_NOTICES.md` | the libraries Katana is built with and ships with, and their licences |

## The licence in a paragraph

The GPL lets everyone use Katana for any purpose, commercial use included;
study and change it; and copy and redistribute it, changed or not, free or for
a fee. In return, whoever passes on Katana, or a work based on it, must pass it
on under the same licence, with the source code (or a written offer of it), and
must keep the copyright and licence notices. A work based on Katana therefore
stays open when it is distributed. There is no warranty. Running Katana asks
nothing of you, and Katana claims no rights in the drawings and data you make
with it (where it copies a piece of its own built-in material into a file you
make, that piece keeps the licence it came with).

## The additional terms, in plain words

The GPL's section 7 lets a copyright holder add a few kinds of term. Katana
adds terms of those kinds only, all about credit and honesty
(`ADDITIONAL_TERMS.md` has the wording):

1. **Keep the notices.** Every file or document you take from Katana keeps its
   copyright and licence notices. If your work contains any part of Katana and
   shows Appropriate Legal Notices (the GPL's name for the About-box feature:
   copyright, no warranty, the licence and how to read it), put this line in
   them: "Katana, copyright (c) 2026 Jarada, https://github.com/kjarada/Katana".
2. **Mark a changed version as changed**, and do not present it as the original
   Katana.
3. **Do not use the name Jarada** to suggest that Jarada endorses or promotes
   your work, without written permission. No trademark rights in the name
   "Katana" are granted.
4. Merely using Katana is not covered by any of this.

### What the GPL does not let this ask for

The owner asked that anyone who uses Katana's code or passes it on must credit
the author. The GPL allows less than that, and the terms stop where it stops.
Section 7(b) lets a copyright holder require that specified notices be
preserved **in the material they belong to, or in the Appropriate Legal Notices
that a work containing it displays**. It does not let the holder require a new
sentence ("Based on Katana by ...") in a derivative's own documentation or
startup banner: that is a "further restriction", which any recipient may remove
(section 7, closing paragraphs; section 10), and the Free Software Foundation's
FAQ says so of citation requirements
(<https://www.gnu.org/licenses/gpl-faq.html#RequireCitation>). So:

* What the terms secure: the credit travels with every file and document taken
  from Katana, and appears in the About box of any work that contains Katana's
  code and has one. A fork that is renamed and sold still carries it there.
* What they cannot secure: a credit line in a derivative's documentation, or a
  derivative's command-line banner. Jarada can ask for it as a courtesy ("Based
  on Katana by Jarada, https://github.com/kjarada/Katana"); the licence does
  not require it.
* Anything stronger is a different licence model, not a different wording: the
  programs include CGAL's GPL code, so a requirement the GPL forbids would stop
  them being passed on at all, unless the owner bought CGAL's commercial licence
  (which would end the obligation to be open). Whether to go there is the
  owner's decision, with a lawyer.

### Three examples

* **A fork, renamed and sold as a product.** The seller keeps the copyright and
  licence notices in every file it took, shows "Katana, copyright (c) 2026
  Jarada, ..." among the legal notices of its About box, marks the product as
  modified, does not call it Katana or suggest that Jarada backs it, and gives
  every buyer the full source under the GPL. Selling it is allowed.
* **A company using Katana internally to produce drawings.** Nothing is
  required: no attribution on the drawings, nothing to publish. Copies passed
  between colleagues at work are not redistribution.
* **A library extracted from Katana and used in another program.** That program
  is now a work containing Katana's code: it must itself be offered under
  GPL-3.0-or-later with its source, keep the copyright notices in the files it
  took, and, if it has an About box, show the line above in it.

## Why version 3, and not the Linux kernel's version 2

The kernel's licence is `GPL-2.0-only`. Katana cannot use it, for one reason
that is enough: CGAL's 2D Triangulations package is licensed
`GPL-3.0-or-later` (or commercially, and Katana holds no commercial licence).
One translation unit, `src/katana_terrain/cdt_backend.cpp`, includes it, and it
is compiled into all three programs. Code that is "version 3 or later" cannot
be combined into a work that is "version 2 only". Section 7, which allows the
additional terms, also exists only in version 3, and the programs ship with
Apache-2.0 libraries (OpenSSL 3 and others), which the Free Software
Foundation lists as compatible with version 3 and not with version 2.
`THIRD_PARTY_NOTICES.md` has the audit.

"Or later" is a separate choice, not forced by any library: `GPL-3.0-only`
would also work with every library audited. "Or later" lets the project follow
a future version, and also lets a future version's section 7 differ from
today's, which could affect these terms. The kernel says "only". The owner
decides.

## What the licence does not cover

The project licence applies to what the owner has the right to license. These
are in the repository or its packages and are **not** covered by it, or are not
known to be:

1. **`resources/customisation/nsw.customisation.json`**, the built-in
   customisation (linestyles, symbols and survey codes). It was converted from
   third-party library files that came with a notice saying they may not be
   copied or used without their authors' authorisation. The owner states that
   the information in the file is public, decided on 2026-10-08 to include the
   converted file in the repository and in releases, and removed that notice
   from it. The owner does not claim to have written it, so the project licence
   does not extend to it. No document from the authors is recorded in this
   repository, and no permission is claimed.
   * The file is compiled into `katana_cad`, so into every program, and the
     release workflow ships it.
   * The GPL (section 5(c)) wants the whole work, as passed on, licensed under
     it. Whether that is satisfied for a program containing this file rests on
     the owner's statement that the information is public; this repository
     cannot settle it, and a lawyer's confirmation is advised.
   * Making the repository public also publishes the file.
   * Anyone who cannot rely on that statement can build without the file: a
     configure option whose path names no file makes a program with no
     built-in (`-DKATANA_BUILTIN_CUSTOMISATION=<a path with no file>`;
     `src/katana_cad/CMakeLists.txt`, `docs/building.md`).
2. **`samples/`**, installed in every package (`cmake/KatanaPackaging.cmake`).
   Where each file came from is not recorded in this repository, so no licence
   is granted for them here. What can be verified:

   | Folder | What the history and the files show |
   |---|---|
   | `site_plan`, `site_plan.kcs` | a drawing script and the project Katana made from it; in the repository since the first commit (2026-09-20) |
   | `gis` | four small files: parcels, a terrain grid, a point cloud; the point cloud's header names PDAL 2.9.3 as its writer, 2026-09-20; the source of the data is not recorded |
   | `ifc` | two text files added with the IFC export (2026-09-25): a scenario script and classification rules |
   | `utilities` | four small files added with the utility tools (2026-09-25 and 26); one names its columns as a published government schema does (names only) |
   | `Survey_File` | six large files added on 2026-10-08: three `.fld` field files, one data-collector `.sdr` file and one raw instrument `.txt` file, with coordinates that look like a real site's, and **one text file that is a survey software vendor's documentation of its file format** (its title begins "Structure of the"), in the vendor's words. Who made the data, who owns it and whether it may be published is not recorded; the vendor's document is very probably not the owner's to license. `docs/survey.md` says of the project's own fixtures: "No proprietary sample files are used". |

   `Survey_File` should leave the repository and the packages, or its owners'
   permission should be recorded, before the repository is made public.
3. **`tests/`**, which is not installed but will be published with the
   repository. `docs/survey.md` says the survey fixtures were constructed from
   specifications, and `docs/dxf.md` that the two DXF drawings were written by
   hand. For the rest the origin is not recorded: the archive-format fixtures
   in `tests/archive12d/data/` (one carries a comment that it is hand-written;
   it names a viewer, not Katana), the files in `tests/geo/data/`, and the
   two instrument files in `tests/qt_widgets/survey/data/`.
4. **`resources/` and `docs/images/`.** The icons (`katana.ico`, `katana.png`,
   `icon_sheet.png`) are rendered by the project's own tool
   (`katana_make_icons`, `docs/desktop.md`). The plot frame was, by its commit
   message (`bbb26aac`), measured from the owner's own application. The pictures in `docs/images/` are screenshots
   of Katana on sample data. The origin of `resources/screenshot.png` is not
   recorded.
5. **`third_party/`**, which is not committed: the build downloads GoogleTest
   and Google Benchmark into `third_party/_cache`. They are used for tests
   only, are not shipped, and keep their own licences, as does every library in
   `THIRD_PARTY_NOTICES.md`.
6. **The utility schema workbook**, a government agency's document that the
   utility tools can read. It is never in the repository; its own terms limit
   who may use it, and each user brings their own copy.

## What is still to do

None of this is in the code yet, because it is for the people who own the
window, the packaging and the release workflow. Until it is done the terms are
open to challenge: section 7 asks for the notice in the source files, and term 1
asks a work to preserve what Katana's own About box shows, which does not yet
show it.

* **The About dialog** shows the Appropriate Legal Notices: the copyright
  line, that there is no warranty, that the work may be passed on under the
  GPL and where to read it, the line in term 1, and Qt's copyright with a
  pointer to the LGPL (LGPL-3.0 section 4).
* **`katana_cli`** prints a short notice at start-up or has a command that
  prints it. **`katana_mcp`** shows it on stderr or as a resource, never on
  stdout, which is the protocol.
* **Every source file** gets the notice in `ADDITIONAL_TERMS.md`. A script,
  `tools/add_licence_notice.py`, adds it; run it once on the merged tree, so
  that it does not conflict with work in progress.
* **Packages** install `LICENSE`, `ADDITIONAL_TERMS.md`, `LICENSING.md` and
  `THIRD_PARTY_NOTICES.md` (only `LICENSE` is installed now) and the licence
  text of each bundled library. The Windows installer shows the licence. A
  written offer of source names each GPL and LGPL library it ships (poppler,
  librttopo, x264, x265, GEOS, GMP, MPFR, libheif and the others in
  `THIRD_PARTY_NOTICES.md`) with the exact URL of its source, and Eigen's
  (MPL-2.0 section 3.2 asks for the way to its source). The LGPL libraries stay
  replaceable DLLs.
* **The third-party table** is generated per platform: the Linux, macOS and
  Windows ARM64 packages were not audited.
* **`src/katana_qt/katana.rc.in`** says "Copyright (C) Jarada" with no year;
  the notice is "Copyright (c) 2026 Jarada".
* **The release workflow** leaves out the built-in customisation and
  `samples/Survey_File` until their rights are settled (above).
* **`https://github.com/kjarada/Katana`**, the address in the notice, returned
  "not found" on 2026-10-08, because the repository is not public yet. The line
  points nowhere until it is. A way to ask for written permission (term 3)
  needs a contact there.

## Contributing

* **Inbound is outbound.** You contribute under the licence the project is under:
  GPL-3.0-or-later and the additional terms in `ADDITIONAL_TERMS.md`. You keep
  your copyright.
* **Sign off every commit** (`git commit -s`), which adds a `Signed-off-by:`
  line. That is the Developer Certificate of Origin, version 1.1
  (<https://developercertificate.org/>): you wrote the change or have the right
  to submit it under the project's licence. The Linux kernel uses the same
  (<https://docs.kernel.org/process/submitting-patches.html>).
* **Why not a CLA.** A contributor licence agreement or copyright assignment
  would let the owner relicense contributions later, even as closed software.
  That is more than an open project needs from each contributor, and the
  paperwork keeps people away. The cost of the sign-off alone is that changing
  the licence later needs the agreement of every contributor whose code is
  affected. The owner may decide otherwise.
* **Never submit what you cannot license:** code copied from a project with an
  incompatible licence, or files that belong to someone else.
* **AI-assisted changes** say so, with a `Co-Authored-By` line, and the person
  submitting is still the one certifying the right to submit.

## Two cautions

* Parts of Katana were written with the help of AI assistants. Whether, and to
  what extent, copyright subsists in AI-generated code is unsettled in many
  jurisdictions, and so is what that means for licensing it. The owner should
  take legal advice.
* No lawyer has reviewed how this licence and the additional terms are used, or
  this page. Who exactly "Jarada" is, as a legal person, and whether
  contributors' copyright should be named in the notice, are for the owner to
  confirm.
