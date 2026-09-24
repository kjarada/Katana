#pragma once

// Reading a survey file with the reader its format registered (format.hpp).
//
//     auto detection = surveyio::detectFormat(...);              // which format?
//     auto read = surveyio::readSurvey(surveyio::formatRegistry(), formatId,
//                                      bytes, fileName, options); // read it
//
// readSurvey() is the one door to every reader, so that the rules below hold
// for all of them however each is written.
//
// THE DELIMITED-POINTS FORMAT HAS NO READER, deliberately. A coordinate CSV is
// read through a column layout (delimited_points.hpp) that the import wizard's
// layout page shows the person and lets them correct; a reader that guessed
// the layout on its own would import easting as northing without a word the
// first time a file's header was ambiguous. readSurvey() on it is Unsupported,
// saying so; the wizard keeps driving parseDelimitedPoints() as it does today.

#include <cstddef>
#include <filesystem>
#include <string>
#include <string_view>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/surveyio/format.hpp"

namespace katana::surveyio {

// Bytes. The largest file readSurvey() hands a reader, and the default cap on
// one sibling. Policy, stated once: a day's RINEX at 1 Hz from a multi-system
// receiver is a few hundred megabytes, and anything past this is not a survey
// file a person meant to import - refusing it is kinder than a program that
// stops answering while it allocates.
inline constexpr std::size_t kMaxSurveyFileBytes = std::size_t{1} << 30; // 1 GiB

// How many sibling files one read may fetch. A Leica DBX job is a handful of
// files; a reader asking for more is looping on something the file said.
inline constexpr std::size_t kMaxSiblingFiles = 64;

// Reads `bytes` as format `formatId`.
//
// What it adds to the reader's own work:
//   * `fileName` goes through survey::sourceFileName() before the reader sees it;
//   * files over kMaxSurveyFileBytes are refused (InvalidArgument) unread;
//   * every sibling request is checked (a plain name only - see SiblingLookup),
//     counted against kMaxSiblingFiles, and RECORDED with its bytes in
//     ReadResult::siblingsRead, so a survey job can keep exactly what was read
//     and re-read it later without the folder;
//   * the project the reader returns must pass survey::validateProject(); one
//     that does not is FileImportFailure, naming the format - a reader that
//     returns an inconsistent project has a bug, and the file should not be
//     half-imported because of it;
//   * a reader that throws is caught and reported (Internal), never allowed
//     to take the application down;
//   * the result is stamped with the format's id and parser version.
// NotFound for an unknown format; Unsupported for a format with no reader.
[[nodiscard]] katana::core::Result<ReadResult> readSurvey(const FormatRegistry& registry,
                                                          std::string_view formatId,
                                                          std::string_view bytes,
                                                          std::string_view fileName,
                                                          const ReadOptions& options = {});

// A SiblingLookup over one folder on disk: a plain name only, a regular file
// only, at most `maxBytes`. Where the exact name is not there, the folder is
// searched for a name that matches ignoring ASCII case, because the files of a
// DBX job name each other in whatever case the instrument felt like and a case
// sensitive file system would otherwise lose them.
[[nodiscard]] SiblingLookup siblingsInFolder(std::filesystem::path folder,
                                             std::size_t maxBytes = kMaxSurveyFileBytes);

// A SiblingLookup over files already in memory - a survey job re-reading what
// it stored. Same name rules, same case-insensitive fallback.
[[nodiscard]] SiblingLookup siblingsInMemory(std::vector<SiblingFile> files);

// The ImportResult shape the import wizard's report page already shows, for a
// reader's result: warnings through describe(), counts as they are.
[[nodiscard]] ImportResult toImportResult(ReadResult result);

} // namespace katana::surveyio
