# Additional terms

Katana is licensed under the GNU General Public License, version 3 or, at
your option, any later version (`LICENSE`; SPDX `GPL-3.0-or-later`). Section 7
of that licence lets a copyright holder add terms of a few listed kinds. This
file states the ones that apply to Katana's own source code and resources,
including contributions made under these terms. They supplement `LICENSE`;
they do not replace or change it. `LICENSING.md` explains them in plain words,
with examples.

They do not apply to the third-party components listed in
`THIRD_PARTY_NOTICES.md`, which keep their own licences, nor to the files
`LICENSING.md` lists as not covered.

## The terms

For Katana, copyright (c) 2026 Jarada, under GNU GPL version 3, section 7:

1. **Attribution (section 7(b)).** Katana's source files carry the notice
   "Copyright (c) 2026 Jarada" with the licence notice, and Katana's README
   and its Appropriate Legal Notices carry the line "Katana, copyright (c) 2026
   Jarada, https://github.com/kjarada/Katana". A work that contains any part
   of Katana, in source or object form, modified or not, must preserve those
   notices in the material it takes from Katana, and must include the line in
   the Appropriate Legal Notices that the work displays.
2. **Origin (section 7(c)).** A modified version must be marked in reasonable
   ways as different from the original, and must not be presented as the
   original Katana.
3. **Names (section 7(d)).** The name "Jarada" must not be used for publicity
   purposes to suggest that Jarada endorses or promotes a work based on
   Katana, without Jarada's written permission.
4. **Trademark (section 7(e)).** No rights under trademark law are granted
   for the name "Katana".
5. **Use.** Running Katana, and creating drawings or data with it, requires no
   attribution.

Each term stands alone: if one is held invalid, or is removed as section 7
allows, the others remain.

## Before these terms can be relied on

Two things are still to be done in the code, and the terms are weaker until
they are. They are listed in `LICENSING.md`, "What is still to do":

* Katana's own About dialog (and the notice of `katana_cli` and `katana_mcp`)
  must show the line in term 1. Term 1 asks a work to preserve what Katana
  displays; Katana has to display it first.
* Every source file must carry the notice below. Section 7 requires "in the
  relevant source files, a statement of the additional terms that apply to
  those files, or a notice indicating where to find the applicable terms".

## The notice for source files

Katana's source files are to carry the standard notice from the end of the
GPL, with one added line:

```
<one line: the file's name and what it does>
Copyright (c) 2026 Jarada

This program is free software: you can redistribute it and/or modify
it under the terms of the GNU General Public License as published by
the Free Software Foundation, either version 3 of the License, or
(at your option) any later version.

This program is distributed in the hope that it will be useful,
but WITHOUT ANY WARRANTY; without even the implied warranty of
MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
GNU General Public License for more details.

You should have received a copy of the GNU General Public License
along with this program.  If not, see <https://www.gnu.org/licenses/>.

Additional terms under GNU GPL version 3, section 7, apply: see
ADDITIONAL_TERMS.md in https://github.com/kjarada/Katana
```

A short form, for a file where the long one would be out of place (a data
file, a small script), keeps the same facts:

```
SPDX-License-Identifier: GPL-3.0-or-later
Copyright (c) 2026 Jarada. Additional terms under GNU GPL version 3, section 7,
apply: see ADDITIONAL_TERMS.md in https://github.com/kjarada/Katana
```

The SPDX line alone does not carry the additional terms: a tool that reads only
it reports plain `GPL-3.0-or-later`. The second line is part of the notice and
stays with it.

## What these terms are, and are not

* They are about credit and honesty, not about use. The GPL itself gives
  everyone the right to use Katana for any purpose, commercial use included,
  and these terms take none of it away.
* They do not require a derivative to say "Based on Katana" in its own
  documentation or in a banner. Section 7(b) allows only the preservation of
  specified notices in the material they belong to, or in the Appropriate
  Legal Notices that a work containing that material displays. A requirement
  to add a new credit elsewhere would be a "further restriction", which a
  recipient may remove (section 7, closing paragraphs; section 10), and the
  Free Software Foundation's FAQ says the same about citation requirements
  (<https://www.gnu.org/licenses/gpl-faq.html#RequireCitation>). `LICENSING.md`
  has what is asked as a courtesy, and what the owner could choose instead.
* "Appropriate Legal Notices" is the GPL's own term (section 0): the feature of
  an interactive user interface that shows a copyright notice, says there is
  no warranty, that the work may be conveyed under the licence, and how to
  read it. A menu item meets it. A startup banner does so only if it is that
  feature. The GPL's section 5(d) requires a work to show them only if the
  program it is based on does.
* Section 7 allows only the kinds of term it lists: notices and attributions
  (b), marking and no misrepresentation of origin (c), no use of the names of
  authors for publicity (d), and declining trademark rights (e). Any other
  added restriction is a "further restriction" that a recipient may remove.
  These terms were written to stay within (b), (c), (d) and (e). No lawyer has
  reviewed them yet.
