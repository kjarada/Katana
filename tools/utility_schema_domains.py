#!/usr/bin/env python3
# tools/utility_schema_domains.py <Utility-Schema-and-Specification.xlsx> <out.csv>
#
# Makes the delivery-schema file `UTILITY CHECK ... SCHEMA <file>` reads
# (include/katana/survey/subsurface/delivery_schema.hpp) from YOUR copy of the
# TfNSW Utility Schema and Specification workbook (DMS-FT-493).
#
# Why a tool and not a file in the repository: the workbook is TfNSW's, under
# its licence - usable by those delivering to a NSW Government agency with
# its authority - and not ours to redistribute. So the repository carries the
# means of reading it, and each user reads their own copy; a new revision of
# the workbook needs a re-run, not a new Katana. The output is as much the
# client's as the workbook: keep it beside the project, not in a repository.
#
# What is read (verified against v1.2, December 2022):
#   "Utility Configuration Spec": one row per attribute - Field Name, Field
#       Type, Mandatory Data, Attribute Name - which become `field` rows;
#   "Utility PDS": the domain lists, one column per field, row-aligned with
#       an "Asset Type Code" column in the same field group where the values
#       belong under an asset type (subtypes, features, capacities), which
#       becomes each value's parent. A list containing "etc." is a list of
#       examples ("open": sizes, arrangements).
# The workbook names the asset identifier "AssetIdentifier" and says it
# "should be prefixed with the Asset Type Code"; those become the identifier
# and prefix rows.
#
# Only the standard library: an .xlsx is a zip of XML.

import csv
import re
import sys
import zipfile
import xml.etree.ElementTree as ET

NS = {
    "m": "http://schemas.openxmlformats.org/spreadsheetml/2006/main",
    "r": "http://schemas.openxmlformats.org/officeDocument/2006/relationships",
    "rel": "http://schemas.openxmlformats.org/package/2006/relationships",
}

CONFIG_SHEET = "Utility Configuration Spec"
DOMAIN_SHEET = "Utility PDS"
PARENT_FIELD = "Asset Type Code"

# Where the domain sheet's column heading differs from the configuration
# sheet's Field Name for the same field.
HEADING_TO_FIELD = {
    "co-ordinate systems": "co-ordinate system",
}

# v1.2 labels its two organisation attributes the wrong way round: attribute
# TfNSW_ContractOrgCode is "Originator Name" (Domain List: Organisation Name)
# and TfNSW_ContractOrgName is "Originator Code". A deliverable's columns
# carry the ATTRIBUTE names, so the codes go to ...Code and the names to
# ...Name, whatever the labels say, and those two labels are not written
# (a column headed "Originator Name" would otherwise be checked for codes).
HEADING_TO_ATTRIBUTE = {
    "contracted organisation code": "TfNSW_ContractOrgCode",
    "contracted organisation name": "TfNSW_ContractOrgName",
}


def key(text):
    return re.sub(r"\s+", " ", str(text)).strip().lower()


def column_index(reference):
    letters = re.match(r"[A-Z]+", reference).group(0)
    index = 0
    for letter in letters:
        index = index * 26 + (ord(letter) - ord("A") + 1)
    return index - 1


def cell_text(cell, shared):
    kind = cell.get("t")
    if kind == "inlineStr":
        return "".join(t.text or "" for t in cell.iter("{%s}t" % NS["m"]))
    value = cell.find("m:v", NS)
    if value is None or value.text is None:
        return ""
    if kind == "s":
        return shared[int(value.text)]
    if kind in ("str", "e"):
        return value.text
    if kind == "b":
        return "TRUE" if value.text == "1" else "FALSE"
    number = float(value.text)
    return str(int(number)) if number.is_integer() else value.text


def read_workbook(path):
    """{sheet name: list of rows, each a list of cell texts}"""
    with zipfile.ZipFile(path) as archive:
        shared = []
        if "xl/sharedStrings.xml" in archive.namelist():
            root = ET.fromstring(archive.read("xl/sharedStrings.xml"))
            for item in root.findall("m:si", NS):
                shared.append("".join(t.text or "" for t in item.iter("{%s}t" % NS["m"])))
        workbook = ET.fromstring(archive.read("xl/workbook.xml"))
        relations = ET.fromstring(archive.read("xl/_rels/workbook.xml.rels"))
        targets = {rel.get("Id"): rel.get("Target") for rel in relations.findall("rel:Relationship", NS)}
        sheets = {}
        for sheet in workbook.find("m:sheets", NS).findall("m:sheet", NS):
            target = targets[sheet.get("{%s}id" % NS["r"])].lstrip("/")
            if not target.startswith("xl/"):
                target = "xl/" + target
            root = ET.fromstring(archive.read(target))
            rows = []
            for row in root.find("m:sheetData", NS).findall("m:row", NS):
                number = int(row.get("r")) - 1
                while len(rows) <= number:
                    rows.append([])
                cells = rows[number]
                for cell in row.findall("m:c", NS):
                    index = column_index(cell.get("r"))
                    while len(cells) <= index:
                        cells.append("")
                    cells[index] = cell_text(cell, shared).strip()
            sheets[sheet.get("name")] = rows
        return sheets


def at(row, index):
    return row[index] if index < len(row) else ""


def main():
    if len(sys.argv) != 3:
        print("usage: utility_schema_domains.py <workbook.xlsx> <out.csv>", file=sys.stderr)
        return 2
    sheets = read_workbook(sys.argv[1])
    for name in (CONFIG_SHEET, DOMAIN_SHEET):
        if name not in sheets:
            print(f"error: the workbook has no sheet \"{name}\"; is it the Utility Schema "
                  "and Specification?", file=sys.stderr)
            return 1

    # Title and version from the cover.
    version = ""
    for row in sheets.get("Cover page", []):
        for i, text in enumerate(row):
            if key(text) == "version:":
                version = next((t for t in row[i + 1:] if t), "")

    # The fields.
    config = sheets[CONFIG_SHEET]
    heading = [key(t) for t in config[0]]
    try:
        c_name = heading.index("field name")
        c_type = heading.index("field type")
        c_required = heading.index("mandatory data")
        c_attribute = heading.index("attribute name")
    except ValueError as missing:
        print(f"error: {CONFIG_SHEET} has no column {missing}", file=sys.stderr)
        return 1
    fields = []  # (attribute, type, requirement, label)
    by_label = {}
    for row in config[1:]:
        attribute = at(row, c_attribute)
        if not attribute:
            continue
        field = (attribute, re.sub(r"\s+", " ", at(row, c_type)),
                 at(row, c_required), at(row, c_name).strip())
        fields.append(field)
        by_label[key(field[3])] = field

    # The domains.
    pds = sheets[DOMAIN_SHEET]
    groups, headings = pds[0], pds[1]
    group_starts = [i for i, text in enumerate(groups) if text and i > 0] + [len(headings)]

    def group_of(index):
        for start, end in zip(group_starts, group_starts[1:]):
            if start <= index < end:
                return range(start, end)
        return range(index, index + 1)

    domains = {}  # attribute -> [(value, parent)]
    open_domains = set()
    unmatched = []
    for j, text in enumerate(headings):
        if j == 0 or not text:
            continue
        label = HEADING_TO_FIELD.get(key(text), key(text))
        field = by_label.get(label)
        if key(text) in HEADING_TO_ATTRIBUTE:
            field = next((f for f in fields if f[0] == HEADING_TO_ATTRIBUTE[key(text)]), None)
        if field is None:
            unmatched.append(text)
            continue
        if not key(field[1]).startswith("domain list"):
            continue
        parent_column = None
        if key(text) != key(PARENT_FIELD):
            parent_column = next((k for k in group_of(j) if key(at(headings, k)) == key(PARENT_FIELD)), None)
        values = domains.setdefault(field[0], [])
        for row in pds[2:]:
            value = at(row, j)
            if not value:
                continue
            if key(value) == "etc.":
                open_domains.add(field[0])
                continue
            parent = at(row, parent_column) if parent_column is not None else ""
            if (value, parent) not in values:
                values.append((value, parent))

    parent_attribute = by_label[key(PARENT_FIELD)][0]
    identifier = by_label.get("asset identifier")

    with open(sys.argv[2], "w", newline="", encoding="utf-8") as out:
        writer = csv.writer(out, lineterminator="\n")
        writer.writerow(["kind", "attribute", "value", "detail", "label"])
        writer.writerow(["schema", "TfNSW Utility Schema and Specification", version, "", ""])
        writer.writerow(["parent", parent_attribute, "", "", ""])
        if identifier:
            writer.writerow(["identifier", identifier[0], "", "", ""])
            writer.writerow(["prefix", identifier[0], parent_attribute, "", ""])
        swapped = set(HEADING_TO_ATTRIBUTE.values())
        for attribute, field_type, requirement, label in fields:
            if attribute in swapped:
                field_type, label = "Domain List", ""
            writer.writerow(["field", attribute, field_type, requirement, label])
        for attribute, values in domains.items():
            if attribute in open_domains:
                writer.writerow(["open", attribute, "", "", ""])
            for value, parent in values:
                writer.writerow(["domain", attribute, value, parent, ""])

    total = sum(len(v) for v in domains.values())
    print(f"{sys.argv[2]}: {len(fields)} fields, {len(domains)} domains, {total} values"
          f" (schema version {version or 'not stated'})")
    if unmatched:
        print("domain columns with no matching field (not written): " + ", ".join(unmatched))
    return 0


if __name__ == "__main__":
    sys.exit(main())
