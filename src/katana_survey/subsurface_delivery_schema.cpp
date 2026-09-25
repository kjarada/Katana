#include "katana/survey/subsurface/delivery_schema.hpp"

#include <algorithm>
#include <format>
#include <optional>
#include <set>
#include <utility>

#include "katana/core/text.hpp"
#include "subsurface_table.hpp"

namespace katana::survey::subsurface {

namespace {

using core::ErrorCode;
using core::makeError;
using detail::at;
using detail::key;

bool startsWith(std::string_view text, std::string_view prefix)
{
    return text.substr(0, prefix.size()) == prefix;
}

bool isNumber(std::string_view text)
{
    return core::parseFiniteDouble(core::trimmed(text)).has_value();
}

// "2 x 1", "1200 x 900": numbers separated by 'x'.
bool isArrangement(std::string_view text)
{
    const std::string lower = core::lowered(text);
    std::size_t start = 0;
    int parts = 0;
    while (true) {
        const std::size_t cross = lower.find('x', start);
        const std::string_view part = std::string_view(lower).substr(
            start, cross == std::string::npos ? std::string::npos : cross - start);
        if (!isNumber(part)) {
            return false;
        }
        ++parts;
        if (cross == std::string::npos) {
            return parts >= 2;
        }
        start = cross + 1;
    }
}

// YYYY/MM/DD, a real calendar date.
bool isDate(std::string_view text)
{
    if (text.size() != 10 || text[4] != '/' || text[7] != '/') {
        return false;
    }
    const auto year = core::parseInteger(text.substr(0, 4));
    const auto month = core::parseInteger(text.substr(5, 2));
    const auto day = core::parseInteger(text.substr(8, 2));
    if (!year || !month || !day || *month < 1 || *month > 12 || *day < 1) {
        return false;
    }
    const bool leap = (*year % 4 == 0 && *year % 100 != 0) || *year % 400 == 0;
    constexpr int kDays[] = {31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31};
    return *day <= kDays[*month - 1] + (*month == 2 && leap ? 1 : 0);
}

std::string quoted(std::string_view text)
{
    return "\"" + std::string(text) + "\"";
}

} // namespace

std::size_t SchemaCheck::count(FindingSeverity severity) const
{
    return static_cast<std::size_t>(
        std::count_if(findings.begin(), findings.end(), [severity](const SchemaFinding& finding) {
            return finding.severity == severity;
        }));
}

core::Result<DeliverySchema> parseDeliverySchema(std::string_view text)
{
    auto table = detail::readRawTable(text);
    if (!table) {
        return table.error();
    }
    std::map<std::string, std::size_t> columns;
    for (std::size_t i = 0; i < table->header.size(); ++i) {
        columns.emplace(key(table->header[i]), i);
    }
    for (const char* required : {"kind", "attribute", "value", "detail"}) {
        if (!columns.contains(required)) {
            return makeError(ErrorCode::ParseFailure,
                             std::string("a schema file needs a \"") + required + "\" column",
                             at(table->headerLine));
        }
    }
    const auto cell = [&columns](const detail::RawRow& row, const char* name) -> std::string {
        const auto found = columns.find(name);
        return found == columns.end() ? std::string() : row.fields[found->second];
    };

    DeliverySchema schema;
    std::vector<std::pair<std::string, std::size_t>> domainLines; // to check once fields are known
    for (const detail::RawRow& row : table->rows) {
        const std::string kind = key(cell(row, "kind"));
        const std::string attribute = cell(row, "attribute");
        if (attribute.empty()) {
            return makeError(ErrorCode::ParseFailure, "no attribute", at(row.lineNumber));
        }
        if (kind == "schema") {
            schema.title = attribute;
            schema.version = cell(row, "value");
        } else if (kind == "field") {
            SchemaField field;
            field.attribute = attribute;
            field.type = cell(row, "value");
            field.label = cell(row, "label");
            const std::string requirement = key(cell(row, "detail"));
            if (requirement == "yes" || requirement == "mandatory") {
                field.requirement = Requirement::Mandatory;
            } else if (requirement == "conditional") {
                field.requirement = Requirement::Conditional;
            } else if (requirement == "no" || requirement == "optional" || requirement.empty()) {
                field.requirement = Requirement::Optional;
            } else {
                return makeError(ErrorCode::ParseFailure,
                                 "requirement " + quoted(cell(row, "detail")) +
                                     " is not Yes, No or Conditional",
                                 at(row.lineNumber));
            }
            schema.fields.push_back(std::move(field));
        } else if (kind == "domain") {
            schema.domains[attribute].values.push_back({cell(row, "value"), cell(row, "detail")});
            domainLines.emplace_back(attribute, row.lineNumber);
        } else if (kind == "open") {
            schema.domains[attribute].open = true;
            domainLines.emplace_back(attribute, row.lineNumber);
        } else if (kind == "parent") {
            schema.parentAttribute = attribute;
        } else if (kind == "identifier") {
            schema.identifierAttribute = attribute;
        } else if (kind == "prefix") {
            schema.prefixes[attribute] = cell(row, "value");
        } else {
            return makeError(ErrorCode::ParseFailure,
                             "kind " + quoted(cell(row, "kind")) +
                                 " is not schema, field, domain, open, parent, identifier or "
                                 "prefix",
                             at(row.lineNumber));
        }
    }
    if (schema.fields.empty()) {
        return makeError(ErrorCode::ParseFailure, "the schema defines no fields");
    }
    for (const auto& [attribute, lineNumber] : domainLines) {
        const bool defined = std::any_of(
            schema.fields.begin(), schema.fields.end(),
            [&attribute](const SchemaField& field) { return field.attribute == attribute; });
        if (!defined) {
            return makeError(ErrorCode::ParseFailure,
                             "a domain for " + quoted(attribute) + ", which is not a field",
                             at(lineNumber));
        }
    }
    return schema;
}

core::Result<SchemaCheck> checkDelivery(std::string_view text, const DeliverySchema& schema)
{
    auto table = detail::readRawTable(text);
    if (!table) {
        return table.error();
    }
    SchemaCheck check;
    check.rows = table->rows.size();

    // Column of each field, by attribute name or label.
    std::map<std::string, std::size_t> columnOf;
    std::set<std::size_t> used;
    for (const SchemaField& field : schema.fields) {
        for (std::size_t i = 0; i < table->header.size(); ++i) {
            const std::string name = key(table->header[i]);
            if (name == key(field.attribute) ||
                (!field.label.empty() && name == key(field.label))) {
                columnOf.emplace(field.attribute, i);
                used.insert(i);
                break;
            }
        }
    }
    for (std::size_t i = 0; i < table->header.size(); ++i) {
        if (!used.contains(i)) {
            check.notInSchema.push_back(table->header[i]);
        }
    }
    const auto valueOf = [&columnOf](const detail::RawRow& row,
                                     const std::string& attribute) -> std::optional<std::string> {
        const auto found = columnOf.find(attribute);
        if (found == columnOf.end()) {
            return std::nullopt;
        }
        return row.fields[found->second];
    };

    for (const SchemaField& field : schema.fields) {
        if (field.requirement == Requirement::Mandatory && !columnOf.contains(field.attribute)) {
            check.findings.push_back(
                {FindingSeverity::Error, field.attribute, "is mandatory and has no column", 0});
        }
        if (field.requirement == Requirement::Conditional && columnOf.contains(field.attribute)) {
            ++check.conditionalNotChecked;
        }
    }

    std::set<std::string> identifiers;
    for (const detail::RawRow& row : table->rows) {
        const auto finding = [&](FindingSeverity severity, const std::string& attribute,
                                 std::string problem) {
            check.findings.push_back({severity, attribute, std::move(problem), row.lineNumber});
        };
        if (!schema.identifierAttribute.empty()) {
            if (const auto id = valueOf(row, schema.identifierAttribute); id && !id->empty()) {
                identifiers.insert(*id);
            }
        }
        const std::optional<std::string> parent =
            schema.parentAttribute.empty() ? std::nullopt : valueOf(row, schema.parentAttribute);

        for (const SchemaField& field : schema.fields) {
            const std::optional<std::string> value = valueOf(row, field.attribute);
            if (!value) {
                continue;
            }
            if (value->empty()) {
                if (field.requirement == Requirement::Mandatory) {
                    finding(FindingSeverity::Error, field.attribute, "is empty");
                }
                continue;
            }
            const std::string lowerType = core::lowered(field.type);
            if (startsWith(lowerType, "domain list")) {
                const auto domain = schema.domains.find(field.attribute);
                if (domain == schema.domains.end()) {
                    continue; // a domain the schema file does not list: nothing to test against
                }
                std::vector<const SchemaValue*> exact;
                const SchemaValue* nearly = nullptr;
                for (const SchemaValue& listed : domain->second.values) {
                    if (listed.value == *value) {
                        exact.push_back(&listed);
                    } else if (!nearly && key(listed.value) == key(*value)) {
                        nearly = &listed;
                    }
                }
                if (!exact.empty()) {
                    // A value listed under parents - a subtype under its asset
                    // type - must be listed under this row's.
                    const bool anyParent = std::any_of(exact.begin(), exact.end(),
                                                       [](auto* v) { return v->parent.empty(); });
                    if (!anyParent && parent && !parent->empty() &&
                        field.attribute != schema.parentAttribute &&
                        std::none_of(exact.begin(), exact.end(),
                                     [&parent](auto* v) { return v->parent == *parent; })) {
                        finding(FindingSeverity::Error, field.attribute,
                                quoted(*value) + " is not listed under " + schema.parentAttribute +
                                    " " + quoted(*parent));
                    }
                } else if (nearly) {
                    finding(FindingSeverity::Warning, field.attribute,
                            quoted(*value) + " is listed as " + quoted(nearly->value) +
                                "; values must be spelt exactly");
                } else if (!(domain->second.open && (isNumber(*value) || isArrangement(*value)))) {
                    finding(FindingSeverity::Error, field.attribute,
                            quoted(*value) + " is not in the domain");
                }
            } else if (startsWith(lowerType, "date")) {
                if (!isDate(*value)) {
                    finding(FindingSeverity::Error, field.attribute,
                            quoted(*value) + " is not a date as YYYY/MM/DD");
                }
            } else if (startsWith(lowerType, "real number") || startsWith(lowerType, "integer")) {
                if (!isNumber(*value)) {
                    finding(FindingSeverity::Error, field.attribute,
                            quoted(*value) + " is not a number");
                }
            }
        }

        for (const auto& [attribute, other] : schema.prefixes) {
            const auto value = valueOf(row, attribute);
            const auto start = valueOf(row, other);
            if (value && start && !value->empty() && !start->empty() &&
                !startsWith(*value, *start)) {
                finding(FindingSeverity::Warning, attribute,
                        "does not start with its " + other + " " + quoted(*start));
            }
        }
    }
    check.assets = identifiers.size();
    return check;
}

std::string renderSchemaCheck(const SchemaCheck& check, const DeliverySchema& schema)
{
    std::string text = "Delivery schema check";
    if (!schema.title.empty()) {
        text += " against " + schema.title;
        if (!schema.version.empty()) {
            text += " v" + schema.version;
        }
    }
    text += "\n" + std::to_string(check.rows) + " rows";
    if (!schema.identifierAttribute.empty()) {
        text +=
            ", " + std::to_string(check.assets) + " assets (" + schema.identifierAttribute + ")";
    }
    text += std::format(": {} errors, {} warnings\n", check.count(FindingSeverity::Error),
                        check.count(FindingSeverity::Warning));
    if (!check.notInSchema.empty()) {
        std::string names;
        for (const std::string& name : check.notInSchema) {
            names += (names.empty() ? "" : ", ") + name;
        }
        text += "Columns the schema does not define (not checked): " + names + "\n";
    }
    if (check.conditionalNotChecked > 0) {
        text += std::to_string(check.conditionalNotChecked) +
                " conditional fields present; whether each is required is not evaluated\n";
    }

    // Each distinct finding once, in order of first appearance, errors first.
    struct Group {
        const SchemaFinding* first = nullptr;
        std::size_t count = 0;
        std::vector<std::size_t> lines;
    };
    std::vector<Group> groups;
    for (const SchemaFinding& finding : check.findings) {
        auto found = std::find_if(groups.begin(), groups.end(), [&finding](const Group& group) {
            return group.first->severity == finding.severity &&
                   group.first->attribute == finding.attribute &&
                   group.first->problem == finding.problem;
        });
        if (found == groups.end()) {
            groups.push_back({&finding, 0, {}});
            found = std::prev(groups.end());
        }
        ++found->count;
        if (finding.lineNumber > 0 && found->lines.size() < 5) {
            found->lines.push_back(finding.lineNumber);
        }
    }
    std::stable_sort(groups.begin(), groups.end(), [](const Group& a, const Group& b) {
        return a.first->severity < b.first->severity;
    });
    if (groups.empty()) {
        return text + "The schedule meets the schema.\n";
    }
    text += "\n";
    for (const Group& group : groups) {
        text += group.first->severity == FindingSeverity::Error ? "  error    " : "  warning  ";
        text += group.first->attribute + " " + group.first->problem;
        if (!group.lines.empty()) {
            text += " (" + std::to_string(group.count) + (group.count == 1 ? " row" : " rows") +
                    ": line";
            text += group.lines.size() == 1 ? " " : "s ";
            for (std::size_t i = 0; i < group.lines.size(); ++i) {
                text += (i == 0 ? "" : ", ") + std::to_string(group.lines[i]);
            }
            text += group.count > group.lines.size() ? ", ...)" : ")";
        }
        text += "\n";
    }
    return text;
}

} // namespace katana::survey::subsurface
