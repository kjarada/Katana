#!/usr/bin/env python3
# tools/project_dump.py <project-directory> [--ignore-metadata <key>]...
# tools/project_dump.py --self-test
#
# A Katana project - the directory the storage layer writes: project.db,
# backups/, terrain/, pointcloud/, assets/, cache/ - as text, read with
# Python's own sqlite3 module. It is neither the C++ storage layer nor its
# Rust port, so that when step 3's round-trip gate compares a project the C++
# saved with one the Rust saved (docs/rust.md, "The order of the work";
# rust/crates/katana_app/tests/round_trip.rs), the judge is not one of the two
# programs being judged: a reader shared by both would agree with a writer
# shared by both, mistakes included.
#
# What is dumped, in this order:
#   * the directory: every folder and file under it, by path, with each
#     file's size and SHA-256; backups/ by the number of backups in it and
#     project.db by name, both dumped below instead;
#   * project.db: the pragmas a project keeps in its header (application_id,
#     user_version, encoding, page_size, auto_vacuum); every row of
#     sqlite_master but its rootpage, which is where SQLite happened to put the
#     table, not what the table is; then every table, its columns as
#     PRAGMA table_info gives them and every row in primary-key order (by
#     rowid where a table declares no key), each value with its storage
#     class: integers and reals exactly (a real by Python's repr, the shortest
#     text that reads back as the same double), text as JSON, blobs as hex;
#   * each backup in backups/, in the order the store ranks them (by the time
#     and sequence in their names, project_store.cpp parseBackupName), dumped
#     as project.db is.
#
# Two saves of the same content differ in a few values by design, and those
# are normalised: each is listed below with where it comes from. A volatile
# value that is not empty is shown as <stamp>; an empty one stays empty, since
# one program writing a stamp where the other writes nothing is a difference.
# The CREATE statements in sqlite_master have their line ends normalised to
# LF: SQLite keeps a statement as it was written, and the C++ built in WSL
# from a Windows checkout - where core.autocrlf gives the sources CRLF -
# stores CRLF where the Rust, from the same text, stores LF (docs/rust.md,
# "Not done"). Nothing else is normalised: a value is compared as it is
# stored.
#
# --ignore-metadata <key> leaves that key's metadata row out of the dump, for
# a comparison that must not judge it - the round-trip gate names each one it
# uses and why. The dump says which were left out.
#
# --self-test builds projects in a scratch folder and checks that the dump
# sees what it must and ignores only what it may; the `project_dump_self_test`
# test runs it.
#
# Prints the dump on standard output and exits 0; exits 2 for a directory
# that holds no project.db.

import argparse
import hashlib
import json
import os
import re
import shutil
import sqlite3
import sys
import tempfile
import urllib.request

DUMP_VERSION = 1
STAMP = "<stamp>"

# Metadata keys whose values two saves of the same content do not share.
VOLATILE_METADATA = {
    # ProjectStore::create stamps the clock (project_store.cpp utcTimestamp),
    # so every project has the moment it was made.
    "created_utc": "the time the project was created",
    # ProjectStore::save writes the clock on every save (writeMetadata).
    "modified_utc": "the time of the last save",
    # The version of the program that wrote the project, which two programs
    # need not share and which an older Katana may have recorded.
    "application_version": "the writing program's version",
}

# Columns whose values two saves of the same content do not share, by table.
VOLATILE_COLUMNS = {
    # SURVEY IMPORT stamps the job with the clock (survey_verbs.cpp nowUtc,
    # cad/survey_job.cpp nowUtc): the C++ field importedUtc.
    ("survey_jobs", "imported_utc"): "the time the job was imported",
    # The reduction report's own stamp, from the same clock (reportCreatedUtc).
    ("survey_jobs", "report_created_utc"): "the time the report was made",
}

# Text columns that embed their row's volatile values: each one found in the
# text is replaced by <stamp> there too.
STAMPED_TEXT_COLUMNS = {
    # The report says when it was made (survey_job.cpp renderInto).
    ("survey_jobs", "report_text"),
    ("survey_jobs", "report_html"),
}

# How the store names a backup (project_store.cpp backup): the compact UTC
# time, and a sequence when two land in the same millisecond.
BACKUP_NAME = re.compile(r"^project-(\d{8}T\d{6}-\d{3})(?:-(\d+))?\.db$")

DATABASE_FILE = "project.db"
BACKUP_DIRECTORY = "backups"

HEADER_PRAGMAS = ["application_id", "user_version", "encoding", "page_size", "auto_vacuum"]


def quoted_identifier(name):
    return '"' + name.replace('"', '""') + '"'


def text_literal(raw):
    """Text as stored (bytes): JSON where it is UTF-8, else its bytes in hex,
    so that text a reader would refuse is still dumped and still compared."""
    try:
        return "t:" + json.dumps(raw.decode("utf-8"), ensure_ascii=True)
    except UnicodeDecodeError:
        return "t!" + raw.hex()


def value_literal(kind, value):
    """One value with its storage class, as typeof() names it."""
    if kind == "null":
        return "null"
    if kind == "integer":
        return "i:" + str(value)
    if kind == "real":
        return "r:" + repr(float(value))
    if kind == "text":
        return text_literal(value if isinstance(value, bytes) else str(value).encode("utf-8"))
    if kind == "blob":
        return "b:" + bytes(value).hex()
    return "?" + kind + ":" + repr(value)


def connect(path):
    # Read-only, so that dumping a project can never change it, and with text
    # as bytes, so that text that is not UTF-8 is dumped rather than raised.
    uri = "file:" + urllib.request.pathname2url(os.path.abspath(path)) + "?mode=ro"
    connection = sqlite3.connect(uri, uri=True)
    connection.text_factory = bytes
    return connection


def as_text(raw):
    if isinstance(raw, bytes):
        return raw.decode("utf-8", errors="replace")
    return "" if raw is None else str(raw)


def table_columns(connection, table):
    rows = connection.execute("PRAGMA table_info(" + quoted_identifier(table) + ")").fetchall()
    # cid, name, type, notnull, dflt_value, pk
    return [(int(r[0]), as_text(r[1]), as_text(r[2]), int(r[3]), r[4], int(r[5])) for r in rows]


def dump_database(path, ignored_metadata, out):
    connection = connect(path)
    try:
        for pragma in HEADER_PRAGMAS:
            value = connection.execute("PRAGMA " + pragma).fetchone()[0]
            out.append("pragma %s=%s" % (pragma, as_text(value)))

        out.append("schema:")
        master = connection.execute(
            "SELECT type, name, tbl_name, sql FROM sqlite_master ORDER BY type, name").fetchall()
        tables = []
        for kind, name, owner, sql in master:
            kind, name, owner = as_text(kind), as_text(name), as_text(owner)
            if sql is None:
                statement = "null"
            else:
                raw = sql if isinstance(sql, bytes) else str(sql).encode("utf-8")
                statement = text_literal(raw.replace(b"\r\n", b"\n"))
            out.append("  %s %s on %s: %s" % (kind, json.dumps(name), json.dumps(owner), statement))
            if kind == "table":
                tables.append(name)

        for table in sorted(tables):
            columns = table_columns(connection, table)
            keys = sorted((c for c in columns if c[5] > 0), key=lambda c: c[5])
            out.append("table %s" % json.dumps(table))
            for cid, name, declared, notnull, default, pk in columns:
                out.append("  column %d %s %s notnull=%d default=%s pk=%d" % (
                    cid, json.dumps(name), json.dumps(declared), notnull,
                    "null" if default is None else json.dumps(as_text(default)), pk))
            selected = ", ".join("typeof(%s), %s" % (quoted_identifier(c[1]), quoted_identifier(c[1]))
                                 for c in columns)
            order = ", ".join(quoted_identifier(c[1]) for c in keys) if keys else "rowid"
            rows = connection.execute(
                "SELECT %s FROM %s ORDER BY %s" % (selected, quoted_identifier(table), order)).fetchall()
            names = [c[1] for c in columns]
            written = 0
            lines = []
            for row in rows:
                pairs = [(as_text(row[2 * i]), row[2 * i + 1]) for i in range(len(names))]
                line = dump_row(table, names, pairs, ignored_metadata)
                if line is not None:
                    lines.append("  " + line)
                    written += 1
            out.append("  rows %d" % written)
            out.extend(lines)
    finally:
        connection.close()


def dump_row(table, names, pairs, ignored_metadata):
    """One row's values, tab-separated (a tab inside text is escaped by its
    JSON), normalised as the tables above say; None for a row left out."""
    if table == "metadata" and "key" in names and "value" in names:
        key = pairs[names.index("key")][1]
        key = as_text(key)
        if key in ignored_metadata:
            return None
        if key in VOLATILE_METADATA:
            index = names.index("value")
            kind, value = pairs[index]
            if kind == "text" and value not in (b"", ""):
                pairs[index] = ("text", STAMP.encode("utf-8"))
    stamps = []
    for index, name in enumerate(names):
        if (table, name) in VOLATILE_COLUMNS:
            kind, value = pairs[index]
            if kind == "text" and value not in (b"", ""):
                stamps.append(value if isinstance(value, bytes) else str(value).encode("utf-8"))
                pairs[index] = ("text", STAMP.encode("utf-8"))
    for index, name in enumerate(names):
        if (table, name) in STAMPED_TEXT_COLUMNS and stamps:
            kind, value = pairs[index]
            if kind == "text":
                raw = value if isinstance(value, bytes) else str(value).encode("utf-8")
                # Longest first, so that a stamp holding another is replaced whole.
                for stamp in sorted(set(stamps), key=len, reverse=True):
                    raw = raw.replace(stamp, STAMP.encode("utf-8"))
                pairs[index] = ("text", raw)
    return "\t".join(value_literal(kind, value) for kind, value in pairs)


def file_digest(path):
    digest = hashlib.sha256()
    with open(path, "rb") as stream:
        for block in iter(lambda: stream.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def backup_rank(name):
    """The store's order (project_store.cpp backupsNewestFirst, oldest first
    here): by the time in the name, then by the sequence a collision adds.
    None for a name the store does not write."""
    found = BACKUP_NAME.match(name)
    if not found:
        return None
    return (found.group(1), int(found.group(2) or "0"))


def dump_project(directory, ignored_metadata=()):
    """The dump of a project directory, as a list of lines."""
    ignored_metadata = set(ignored_metadata)
    database = os.path.join(directory, DATABASE_FILE)
    if not os.path.isfile(database):
        raise FileNotFoundError("no %s in %s" % (DATABASE_FILE, directory))
    out = ["project_dump %d" % DUMP_VERSION]
    if ignored_metadata:
        out.append("ignored metadata: " + ", ".join(sorted(ignored_metadata)))

    out.append("directory:")
    backups = []
    for root, folders, files in os.walk(directory):
        folders.sort()
        relative_root = os.path.relpath(root, directory)
        relative_root = "" if relative_root == "." else relative_root.replace(os.sep, "/")
        in_backups = relative_root == BACKUP_DIRECTORY
        for folder in folders:
            path = (relative_root + "/" if relative_root else "") + folder
            out.append("  dir " + json.dumps(path))
        for name in sorted(files):
            path = (relative_root + "/" if relative_root else "") + name
            if not relative_root and name == DATABASE_FILE:
                out.append("  file " + json.dumps(path) + " (dumped below)")
                continue
            if in_backups:
                rank = backup_rank(name)
                if rank is not None:
                    backups.append((rank, name))
                    continue
                # Not a name the store writes: a file like any other.
            full = os.path.join(root, name)
            out.append("  file %s size=%d sha256=%s" % (
                json.dumps(path), os.path.getsize(full), file_digest(full)))
    out.append("  backups %d (dumped below, oldest first; their names hold the time they were taken)"
               % len(backups))

    out.append("== " + DATABASE_FILE)
    dump_database(database, ignored_metadata, out)
    for number, (_, name) in enumerate(sorted(backups), start=1):
        out.append("== backup %d" % number)
        dump_database(os.path.join(directory, BACKUP_DIRECTORY, name), ignored_metadata, out)
    return out


# ---- the self-test ---------------------------------------------------------------------

def make_project(scratch, label, stamp="2026-01-01T00:00:00.000Z", version="0.1.0",
                 crlf=False, reverse=False, backup_name="project-20260101T000000-000.db",
                 entity_weight=0.5, entity_blob=b"\x01\x02\x03", integer_extra=False,
                 reference_layers="", extra_file=False, backups=1, created=None,
                 layer_text=b"Ground"):
    """A small project of three tables, enough to show what the dump sees:
    a WITHOUT ROWID table keyed on text, a rowid table keyed on an integer
    (with a column of no declared type, the one kind of column where an
    integer and a real are stored as they were bound), and the survey jobs'
    stamped columns."""
    directory = os.path.join(scratch, label)
    os.makedirs(os.path.join(directory, BACKUP_DIRECTORY))
    os.makedirs(os.path.join(directory, "terrain"))
    end = "\r\n" if crlf else "\n"
    path = os.path.join(directory, DATABASE_FILE)
    connection = sqlite3.connect(path)
    connection.execute("PRAGMA application_id = %d" % 0x4B544E41)  # "KTNA", as the store marks one
    connection.execute("PRAGMA user_version = 13")
    connection.execute(end.join([
        "CREATE TABLE metadata (",
        "    key   TEXT PRIMARY KEY,",
        "    value TEXT NOT NULL",
        ") WITHOUT ROWID"]))
    connection.execute(end.join([
        "CREATE TABLE entities (",
        "    id       INTEGER PRIMARY KEY,",
        "    layer    TEXT NOT NULL,",
        "    weight   REAL NOT NULL,",
        "    geometry BLOB NOT NULL,",
        "    extra",
        ")"]))
    connection.execute(end.join([
        "CREATE TABLE survey_jobs (",
        "    position           INTEGER PRIMARY KEY,",
        "    report_text        TEXT NOT NULL,",
        "    report_created_utc TEXT NOT NULL,",
        "    imported_utc       TEXT NOT NULL",
        ")"]))
    metadata = [
        ("name", "corpus"),
        ("created_utc", stamp if created is None else created),
        ("modified_utc", stamp),
        ("application_version", version),
        ("reference_layers", reference_layers),
    ]
    entities = [(1, b"Ground", 1.0, b"\x00", 1 if integer_extra else 1.0),
                (2, layer_text, entity_weight, entity_blob, None)]
    if reverse:
        metadata.reverse()
        entities.reverse()
    connection.executemany("INSERT INTO metadata VALUES (?, ?)", metadata)
    for identifier, layer, weight, geometry, extra in entities:
        connection.execute("INSERT INTO entities VALUES (?, CAST(? AS TEXT), ?, ?, ?)",
                           (identifier, layer, weight, geometry, extra))
    connection.execute("INSERT INTO survey_jobs VALUES (1, ?, ?, ?)",
                       ("Report made " + stamp + " from job-1", stamp, stamp))
    connection.commit()
    connection.close()
    for number in range(backups):
        name = backup_name if number == 0 else backup_name.replace(".db", "-%d.db" % number)
        shutil.copyfile(path, os.path.join(directory, BACKUP_DIRECTORY, name))
    if extra_file:
        with open(os.path.join(directory, "terrain", "stray.bin"), "wb") as stream:
            stream.write(b"stray")
    return directory


def self_test():
    checks = 0
    failures = []

    def check(condition, what):
        nonlocal checks
        checks += 1
        if not condition:
            failures.append(what)

    def rows_of(lines, table):
        """The row lines of `table` in a dump's project.db."""
        found = []
        inside = False
        for line in lines:
            if line.startswith("== backup"):
                break
            if line.startswith("table ") or line.startswith("== "):
                inside = line == "table " + json.dumps(table)
                continue
            if inside and line.startswith("  ") and not line.startswith("  column ") \
                    and not line.startswith("  rows "):
                found.append(line)
        return found

    with tempfile.TemporaryDirectory(prefix="project_dump_") as scratch:
        made = 0

        def project(**kw):
            nonlocal made
            made += 1
            return dump_project(make_project(scratch, "p%d" % made, **kw))

        base = dump_project(make_project(scratch, "base"))
        # What two saves of the same content do not share, and nothing else.
        check(base == project(stamp="2026-11-08T12:34:56.789Z", version="0.3.0", crlf=True,
                              reverse=True, backup_name="project-20261108T123456-789.db"),
              "two saves of the same content - their own stamps, version, line ends, backup "
              "names and insertion order - dump the same")
        check(any(line.startswith("  table \"entities\"") or "\"CREATE TABLE entities" in line
                  for line in base),
              "the schema's CREATE statements are in the dump")
        check(any("\\r\\n" in line for line in dump_project(
                  make_project(scratch, "crlf_text", layer_text=b"Ground\r\nSouth"))),
              "a line end inside a value is a value, and is not normalised")
        # Each of these is a difference in content, and must be seen.
        check(base != project(entity_weight=0.25), "a real that differs")
        check(base != project(entity_weight=0.5 + 2.0 ** -52),
              "a real that differs in its last bit")
        check(base != project(integer_extra=True),
              "an integer 1 stored where the other stores the real 1.0")
        check(base != project(entity_blob=b"\x01\x02\x04"), "a blob that differs in one byte")
        check(base != project(created=""), "a stamp written by one and left empty by the other")
        check(base != project(version=""), "a version written by one and left empty by the other")
        check(base != project(reference_layers="raster source=a.tif"), "a metadata value that differs")
        check(base != project(extra_file=True), "a file one project has and the other does not")
        check(base != project(backups=2), "a backup one project has and the other does not")
        # Left out by name, and said so.
        left_a = dump_project(make_project(scratch, "ignore_a"), ["reference_layers"])
        left_b = dump_project(make_project(scratch, "ignore_b", reference_layers="raster source=a.tif"),
                              ["reference_layers"])
        check(left_a == left_b, "--ignore-metadata leaves the key's row out")
        check("ignored metadata: reference_layers" in left_a, "the dump says what it left out")
        # Text that is not UTF-8 is dumped, as bytes, rather than refused.
        latin = dump_project(make_project(scratch, "latin", layer_text=b"Stra\xdfe"))
        check(any("t!" + b"Stra\xdfe".hex() in line for line in latin),
              "text that is not UTF-8 is dumped as its bytes")
        check(latin != base, "and is compared")
        # The order is the primary key's, whatever the order of insertion.
        rows = rows_of(base, "entities")
        check(len(rows) == 2 and rows[0].startswith("  i:1\t") and rows[1].startswith("  i:2\t"),
              "rows in primary-key order")
        check(rows_of(base, "entities") == rows_of(project(reverse=True), "entities"),
              "rows inserted in the other order dump in the same order")
        # The stamp inside the report is its row's, and is replaced.
        check(any("Report made <stamp> from job-1" in line for line in base),
              "a report's own stamp inside its text is normalised")
        # A folder with no project.db is refused.
        try:
            dump_project(os.path.join(scratch, "nowhere"))
            check(False, "a folder with no project.db is refused")
        except FileNotFoundError:
            check(True, "a folder with no project.db is refused")

    if failures:
        for failure in failures:
            print("project_dump self-test FAILED: " + failure)
        print("project_dump self-test: %d of %d checks failed" % (len(failures), checks))
        return 1
    print("project_dump self-test: %d checks passed" % checks)
    return 0


def main(argv):
    parser = argparse.ArgumentParser(
        description="Dump a Katana project directory as text, independent of both programs.")
    parser.add_argument("directory", nargs="?", help="the project directory (holding project.db)")
    parser.add_argument("--ignore-metadata", action="append", default=[], metavar="KEY",
                        help="leave this metadata key's row out of the dump")
    parser.add_argument("--self-test", action="store_true", help="run the script's own checks")
    arguments = parser.parse_args(argv[1:])
    if arguments.self_test:
        return self_test()
    if not arguments.directory:
        parser.print_usage(sys.stderr)
        return 2
    try:
        lines = dump_project(arguments.directory, arguments.ignore_metadata)
    except FileNotFoundError as error:
        print("project_dump: " + str(error), file=sys.stderr)
        return 2
    sys.stdout.write("\n".join(lines) + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
