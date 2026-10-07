# Licensing

Katana is free software. It is licensed under the GNU General Public License
version 3 or, at your option, any later version (SPDX `GPL-3.0-or-later`), with
one short additional term about attribution. Copyright (c) 2026 Jarada.
This page says what that means in plain words and what the licence does not
cover. It is a guide, not legal advice and not part of the licence: the binding
texts are `LICENSE` and `ADDITIONAL_TERMS.md`.

| File | What it holds |
|---|---|
| `LICENSE` | the GNU General Public License, version 3, unmodified, as the Free Software Foundation publishes it |
| `ADDITIONAL_TERMS.md` | the attribution term, added under section 7 of that licence, and the notice for source files |
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

## The attribution term, in plain words

The GPL's section 7 lets a copyright holder add a few kinds of term. Katana
adds terms of those kinds only, all about credit and honesty
(`ADDITIONAL_TERMS.md` has the wording):

1. Keep the notice "Copyright (c) 2026 Jarada" and the licence notices on every
   copy, changed or not.
2. If your work includes or is based on Katana's code, say "Based on Katana by
   Jarada, https://github.com/kjarada/Katana" in its documentation, in its About
   dialog if it has a user interface (the GPL calls this the program's
   "Appropriate Legal Notices"), and in its startup banner if it prints one.
3. Mark a changed version as changed, and do not present it as the original
   Katana.
4. Do not use the names Katana or Jarada to suggest that Jarada endorses your
   work.

Merely using Katana is not covered by any of this.

### Three examples

* **A fork, renamed and sold as a product.** The seller keeps the copyright and
  licence notices, shows "Based on Katana by Jarada ..." in the About box and
  the documentation, marks the product as modified, does not call it Katana or
  suggest that Jarada backs it, and gives every buyer the full source under the
  GPL. Selling it is allowed.
* **A company using Katana internally to produce drawings.** Nothing is
  required: no attribution on the drawings, nothing to publish. Copies passed
  between colleagues at work are not redistribution.
* **A library extracted from Katana and used in another program.** That program
  is now a work based on Katana's code: it must itself be offered under
  GPL-3.0-or-later with its source, say "Based on Katana by Jarada ..." in its
  documentation and in its About box if it has one, and keep the copyright
  notices in the files it took.

## Why version 3, and not the Linux kernel's version 2

Katana is built on CGAL's 2D triangulation code, which is GPL-3.0-or-later, and
ships with Apache-2.0 libraries (OpenSSL 3 and others); neither can be combined
with the kernel's `GPL-2.0-only`, while both fit version 3. Section 7, which
allows the attribution term, exists only in version 3. (`THIRD_PARTY_NOTICES.md` has the
audit; the kernel's licence is the same family, an earlier version.)

## What the licence does not cover

The project licence applies to what the owner has the right to license. These
are in the repository or its packages and are **not** covered by it:

1. **`resources/customisation/nsw.customisation.json`**, the built-in
   customisation (linestyles, symbols and survey codes). It was converted from
   third-party library files that came with a notice saying they may not be
   copied or used without their authors' authorisation. The owner decided to
   include the converted file and removed that notice from it, but cannot grant
   rights over material that belongs to someone else. No permission from the
   authors is recorded in this repository, and none is claimed. The file is
   compiled into the programs. Anyone who cannot rely on it can leave it out:
   build with `-DKATANA_BUILTIN_CUSTOMISATION=<another file>`
   (`docs/building.md`).
2. **`samples/`**, installed in every package. Where each file came from is not
   recorded in this repository, so no licence is granted for them here. What
   can be verified:

   | Folder | What the history and the files show |
   |---|---|
   | `site_plan`, `site_plan.kcs` | a drawing script and the project Katana made from it; in the repository since the first commit (2026-09-20) |
   | `gis` | four small files: parcels, a terrain grid, a point cloud; the point cloud's header names PDAL 2.9.3 as its writer, 2026-09-20; the source of the data is not recorded |
   | `ifc` | two text files added with the IFC export (2026-09-25): a scenario script and classification rules |
   | `utilities` | four small files added with the utility tools (2026-09-25 and 26); one names its columns as a published government schema does (names only) |
   | `Survey_File` | six large files added on 2026-10-08: field-book and data-collector files from survey instruments, and one text file that describes a survey software's file format in its vendor's words. Who made the data, who owns it and whether it may be published is not recorded here |

3. **`third_party/`**, which is not committed: the build downloads GoogleTest and
   Google Benchmark into `third_party/_cache`. They are used for tests only, are
   not shipped, and keep their own licences, as does every library in
   `THIRD_PARTY_NOTICES.md`.
4. **The utility schema workbook**, a government agency's document that the
   utility tools can read. It is never in the repository; its own terms limit
   who may use it, and each user brings their own copy.

## Contributing

* **Inbound is outbound.** You contribute under the licence the project is under:
  GPL-3.0-or-later and the additional terms in `ADDITIONAL_TERMS.md`. You keep
  your copyright.
* **Sign off every commit** (`git commit -s`), which adds a `Signed-off-by:`
  line. That is the Developer Certificate of Origin, version 1.1
  (<https://developercertificate.org/>): you wrote the change or have the right
  to submit it under the project's licence. The Linux kernel uses the same.
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
  this page. Who exactly "Jarada" is, as a legal person, is for the owner to
  confirm.
