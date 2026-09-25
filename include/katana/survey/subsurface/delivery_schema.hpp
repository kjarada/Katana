#pragma once

// Checking a utility schedule against a client's delivery schema: which
// attributes each asset must carry and which values each may take. The TfNSW
// Utility Schema and Specification (DMS-FT-493) is the one this was written
// against; nothing here is specific to it.
//
// WHY THE SCHEMA IS DATA AND NOT CODE. A delivery schema is a client's
// document, revised on the client's timetable and under the client's licence -
// TfNSW's may be used only by those delivering to a NSW Government agency, and
// is not ours to copy into this repository. So the schema is read at run time
// from a file its user makes from their own copy
// (tools/utility_schema_domains.py), and a new revision needs a new file, not
// a new build. The format is plain CSV so that a schema from any other client
// can be written by hand:
//
//   kind,attribute,value,detail,label
//   schema,<title>,<version>,,
//   field,<attribute>,<field type>,Yes|No|Conditional,<field name>
//   domain,<attribute>,<value>,<parent value or empty>,
//   open,<attribute>,,,          the domain lists examples: sizes, arrangements
//   parent,<attribute>,,,        the attribute a domain's parent values belong to
//   identifier,<attribute>,,,    the attribute that names an asset
//   prefix,<attribute>,<other>,, <attribute>'s value should start with <other>'s
//
// A field type starting "Domain List" is checked against its domain; "Date"
// against YYYY/MM/DD; "Real Number" or "Integer" as a number; anything else
// is free text.

#include <cstddef>
#include <map>
#include <string>
#include <string_view>
#include <vector>

#include "katana/core/error.hpp"

namespace katana::survey::subsurface {

enum class Requirement {
    Mandatory,
    Conditional, // required when something else holds; not checked, only counted
    Optional,
};

struct SchemaField {
    std::string attribute; // the name a deliverable's column carries: "AssetOwner"
    std::string label;     // the name a person reads: "Asset Owner"
    std::string type;      // "Domain List: Asset Owner", "Date (YYYY/MM/DD)" ...
    Requirement requirement = Requirement::Optional;
};

struct SchemaValue {
    std::string value;
    std::string parent; // the parent attribute's value this one belongs under; empty for any
};

struct SchemaDomain {
    std::vector<SchemaValue> values;
    // The listed values are examples of an open set: a value not listed is
    // still accepted when it is a number or a "number x number" arrangement.
    bool open = false;
};

struct DeliverySchema {
    std::string title;
    std::string version;
    std::vector<SchemaField> fields;
    std::map<std::string, SchemaDomain> domains; // by attribute
    std::string parentAttribute;                 // e.g. "AssetTypeCode"
    std::string identifierAttribute;             // e.g. "AssetIdentifier"
    std::map<std::string, std::string> prefixes; // attribute -> the attribute it starts with
};

// ParseFailure, naming the line, for an unknown kind, a field or domain with
// no attribute, a domain for an attribute that is not a field, or a schema
// with no fields.
[[nodiscard]] core::Result<DeliverySchema> parseDeliverySchema(std::string_view text);

enum class FindingSeverity {
    Error,   // the deliverable does not meet the schema
    Warning, // it may not: a spelling that differs only in case, a prefix
};

struct SchemaFinding {
    FindingSeverity severity = FindingSeverity::Error;
    std::string attribute;
    // What is wrong, the same words for every row it is wrong on, so that
    // findings group: "is empty", "\"Inservice\" is not in the domain" ...
    std::string problem;
    std::size_t lineNumber = 0; // 0 for a finding about the file as a whole
};

struct SchemaCheck {
    std::size_t rows = 0;
    std::size_t assets = 0;               // distinct identifiers, when the schema names one
    std::vector<std::string> notInSchema; // columns the schema does not define (coordinates ...)
    std::size_t conditionalNotChecked =
        0; // conditional fields present, whose condition is not evaluated
    std::vector<SchemaFinding> findings; // in file order, file-wide ones first

    [[nodiscard]] std::size_t count(FindingSeverity severity) const;
};

// Every row of the schedule `text` (the delimited format of utility_csv.hpp:
// a header row, commas, optional quotes) against `schema`. Columns are
// matched to attributes by attribute name or label, ignoring case, blanks,
// '-' and '_'. Values are matched EXACTLY - a delivery schema asks for its
// values "using the exact field values" - and a value that matches only when
// case and spacing are ignored is reported as a warning naming the listed
// spelling.
//
// ParseFailure only when the text cannot be read as a table at all.
[[nodiscard]] core::Result<SchemaCheck> checkDelivery(std::string_view text,
                                                      const DeliverySchema& schema);

// The check as text: counts, then each distinct finding once with how many
// rows it is on and the first few line numbers.
[[nodiscard]] std::string renderSchemaCheck(const SchemaCheck& check, const DeliverySchema& schema);

} // namespace katana::survey::subsurface
