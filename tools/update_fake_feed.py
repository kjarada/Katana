#!/usr/bin/env python3
"""A fake release feed for the tests of Katana's update system, on the loopback address and nowhere else.

    python tools/update_fake_feed.py --root <release folder> [switches]

It serves the files of one folder (a release: Katana-update-<target>.json, its .sig, the update zip) the
way a release host does, and misbehaves on request, so that the engine's transport rules
(include/katana/update/feed.hpp) are tested against real HTTP and not only against a function that
pretends to be it. Standard library only (http.server). It listens on 127.0.0.1, port 0, twice, and
prints the two ports the system gave it, one `key=value` a line, before it serves anything:

    port=<n>            the feed
    second_port=<n>     the host a redirect points at (a release's storage host); it serves the same
                        folder and exists so that what a client sends on the SECOND hop can be read

It never listens on another address and has no option to: a test may start its own server on the
loopback address (CLAUDE.md, section 4), and that is all this is. It does not look a name up either.

What it does by itself, with no switch:
  GET <file>                    200, the whole file, `Accept-Ranges: bytes`
  GET <file> + Range: bytes=a-b 206, `Content-Range: bytes a-b/<size>`, exactly those bytes (RFC 9110,
                                14.1.2: `a-`, `-n` and a last byte past the end are read as the RFC
                                says); a range past the end is 416 with `Content-Range: bytes */<size>`;
                                several ranges in one request are 501, which is what the release host
                                answered when it was measured (ranges.hpp); a Range that is not one is
                                ignored, as the RFC allows
  anything else                 404; a method other than GET is 501, and a request that cannot be read
                                as one is 400 (414, 431 and 505 as http.server finds them), each with
                                an empty body

The switches, each of which may be repeated. GLOB is matched against the request's path (decoded,
starting with '/', without its query) by fnmatch, so `*.zip` is every zip and `*` is everything. A
rule holds on both ports unless `@first` or `@second` is written after it:
  --status CODE:GLOB            answer CODE (400 to 599): 403, 429, 404 ...
  --html GLOB                   answer 200 with a web page (`text/html`) in place of the file: what a
                                captive portal does to every address
  --redirect GLOB               (the feed's port) answer 302 to the same path on the second port, with a
                                query after it as a signed storage address has; the second port then
                                serves the file, and its log shows the headers the client sent there
  --ignore-range GLOB           answer 200 with the whole file whatever `Range` asked for
  --truncate GLOB[:BYTES]       announce the whole body and send only its first BYTES bytes (half of it
                                when not given), then close: a connection that dropped
  --github OWNER/NAME           also answer as GitHub does for a PRIVATE repository (feed.hpp, "the
                                private-repository route"): the release addresses under /OWNER/NAME/
                                are 404, and under /repos/OWNER/NAME/releases/ `latest` and
                                `tags/<tag>` list the folder's files as assets and `assets/<id>`
                                answers 302 to the second port - all three only for a request that
                                carries `Authorization: Bearer <--token>`, and 404 without it
  --token TEXT                  the token --github asks for
  --storage-base URL            what a redirect names in the place of http://127.0.0.1:<second_port>
                                (a test whose client is told to reach that name on the second port)
On each port, in this order: --status, --html, then on the feed's port --github and --redirect, then
the file with --ignore-range and --truncate.

THE LOG. Every request, on either port, is one record: its number, the port (`first` or `second`),
the method, the path with its query, the request headers as they arrived (names in lower case), the
status answered, what answered (`behaviour`), the range served (first and last byte) when one was,
the length announced and `served`: the bytes of body written to the connection. --log FILE appends
each as one line of JSON; FakeFeed.requests() gives the same records to a test in this process. It is
how a test proves what went over the wire: which bytes of the zip were asked for, and that the second
hop carried no `Authorization`. So EVERY request is a record, the ones the feed refuses too: a method
other than GET, and a request it could not read (`behaviour` is `refused`; the method, the path and
the headers are what was read of them, which may be nothing). A request made in a way no test
expected is then in the log, and not merely absent from it.

    --lifetime SECONDS          stop after this long (default 3600; 0: never), so that a server whose
                                test died does not live for ever
    --until-stdin-closes        also stop when standard input reaches its end (a parent that holds a
                                pipe to this process and goes away)

Exit status: 0 stopped as asked, 2 a usage error.
"""

from __future__ import annotations

import argparse
import fnmatch
import http.server
import json
import re
import socketserver
import sys
import threading
import time
import urllib.parse
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable, Optional

LOOPBACK = "127.0.0.1"
CHUNK = 64 * 1024
RULE_KINDS = ("status", "html", "redirect", "ignore-range", "truncate")
RULE_PORTS = ("any", "first", "second")
# What a redirect's address carries after the path: a release's storage address is a signed one, and all
# of its query is the secret. A test looks for this text where no query may ever be written.
REDIRECT_QUERY = "sig=FAKE-SIGNED-ADDRESS-SECRET"
HTML_PAGE = (b"<!DOCTYPE html>\n<html><head><title>Sign in to this network</title></head>\n"
             b"<body><p>Sign in to continue.</p></body></html>\n")
FIRST_ASSET_ID = 1001
_ONE_RANGE = re.compile(r"bytes=([0-9]*)-([0-9]*)")


@dataclass(frozen=True)
class Rule:
    """One way of misbehaving, for the requests whose path matches `glob` (see the module docstring)."""

    kind: str
    glob: str = "*"
    value: int = 0      # status: the code. truncate: how many bytes of body are sent (0: half)
    port: str = "any"   # "any", "first" (the feed) or "second" (the redirect's target)

    def __post_init__(self) -> None:
        if self.kind not in RULE_KINDS:
            raise ValueError(f"'{self.kind}' is not one of {' '.join(RULE_KINDS)}")
        if self.port not in RULE_PORTS:
            raise ValueError(f"'{self.port}' is not one of {' '.join(RULE_PORTS)}")
        if self.kind == "status" and not 400 <= self.value <= 599:
            raise ValueError(f"a status rule answers 400 to 599, not {self.value}")
        if self.kind == "truncate" and self.value < 0:
            raise ValueError("a truncate rule sends 0 or more bytes")
        if self.kind == "redirect" and self.port == "second":
            raise ValueError("a redirect is answered by the feed's port: the second port is where it leads")

    def matches(self, path: str, port: str) -> bool:
        return self.port in ("any", port) and fnmatch.fnmatchcase(path, self.glob)


def parse_range(header: Optional[str], size: int) -> tuple[str, int, int]:
    """What a Range header asks of a file `size` bytes long: ("whole", 0, 0), ("part", first, last),
    ("unsatisfiable", 0, 0) or ("several", 0, 0). RFC 9110, 14.1.2 and 14.2; a header that is not a
    range of bytes is ignored, which the RFC allows a server."""
    if header is None:
        return ("whole", 0, 0)
    text = header.strip()
    if "," in text:
        return ("several", 0, 0)
    match = _ONE_RANGE.fullmatch(text)
    if not match or (match.group(1) == "" and match.group(2) == ""):
        return ("whole", 0, 0)
    first_text, last_text = match.group(1), match.group(2)
    if first_text == "":                      # bytes=-n: the last n bytes
        count = int(last_text)
        if count == 0 or size == 0:
            return ("unsatisfiable", 0, 0)
        return ("part", max(0, size - count), size - 1)
    first = int(first_text)
    if last_text != "" and int(last_text) < first:
        return ("whole", 0, 0)                # not a range: ignored
    if first >= size:
        return ("unsatisfiable", 0, 0)
    last = size - 1 if last_text == "" else min(int(last_text), size - 1)
    return ("part", first, last)


class _Server(http.server.ThreadingHTTPServer):
    daemon_threads = True
    # Port 0 never needs it, and on Windows the option lets another process take the same port.
    allow_reuse_address = False

    def __init__(self, feed: "FakeFeed", which: str):
        self.feed = feed
        self.which = which
        super().__init__((LOOPBACK, 0), _Handler)

    def server_bind(self) -> None:
        # Not HTTPServer.server_bind: it asks the resolver for this host's name (socket.getfqdn), which
        # can leave the machine, and a test here reaches the loopback address only.
        socketserver.TCPServer.server_bind(self)
        self.server_name = LOOPBACK
        self.server_port = self.server_address[1]


class _Handler(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    server_version = "KatanaFakeFeed"
    sys_version = ""

    def log_message(self, format: str, *args: object) -> None:  # noqa: A002  (the base class's name)
        pass  # the log is FakeFeed's, not standard error's

    def _record(self) -> dict:
        """The log's record of the request being answered, before its answer: what was read of it. A
        request the base class refused may have no path or no headers yet (see send_error)."""
        headers: dict[str, str] = {}
        received = getattr(self, "headers", None)
        for name, value in (received.items() if received is not None else ()):
            key = name.lower()
            headers[key] = f"{headers[key]}, {value}" if key in headers else value
        which: str = self.server.which       # type: ignore[attr-defined]
        return {"port": which, "method": getattr(self, "command", None) or "", "path": getattr(self, "path", ""),
                "headers": headers, "status": 0, "behaviour": "", "range": None, "length": 0, "served": 0}

    def do_GET(self) -> None:  # noqa: N802  (the base class's name)
        feed: FakeFeed = self.server.feed    # type: ignore[attr-defined]
        path = urllib.parse.unquote(urllib.parse.urlsplit(self.path).path)
        record = self._record()
        feed.begin()
        try:
            self._answer(feed, record["port"], path, record)
        finally:
            feed.log(record)

    def send_error(self, code: int, message: Optional[str] = None, explain: Optional[str] = None) -> None:
        """What http.server refuses by itself, without asking do_GET: a method other than GET (501), and a
        request it could not read (400; 414, 431, 505). Each is answered with that status and an empty
        body, and LOGGED like every other request - the log is what proves which requests were made.
        `message` and `explain` are the base class's words for its own page, which is not sent."""
        feed: FakeFeed = self.server.feed    # type: ignore[attr-defined]
        if not getattr(self, "command", None):
            # No request line was read. Some versions of http.server then answer as to HTTP/0.9, with
            # no status line at all (others, 3.14 among them, see to it themselves for some of the
            # cases). The refusal is said in HTTP/1.1 always, so that the status logged is one that was sent.
            self.request_version = self.protocol_version
        record = self._record()
        feed.begin()
        try:
            self._send(record, "refused", int(code), {"Content-Type": "text/plain"}, b"")
        finally:
            feed.log(record)

    # ---- what answers ------------------------------------------------------------------------

    def _answer(self, feed: "FakeFeed", which: str, path: str, record: dict) -> None:
        rules = feed.rules()

        def rule(kind: str) -> Optional[Rule]:
            return next((r for r in rules if r.kind == kind and r.matches(path, which)), None)

        status = rule("status")
        if status is not None:
            return self._send(record, f"status-{status.value}", status.value,
                              {"Content-Type": "text/plain"}, f"{status.value}\n".encode("ascii"))
        if rule("html") is not None:
            return self._send(record, "html", 200, {"Content-Type": "text/html; charset=utf-8"}, HTML_PAGE)
        if which == "first":
            if feed.github and self._github(feed, path, record):
                return None
            if rule("redirect") is not None:
                return self._redirect(feed, record, "redirect", path)

        file = feed.file_for(path)
        if file is None:
            return self._send(record, "missing", 404, {"Content-Type": "text/plain"}, b"404\n")
        data = file.read_bytes()
        headers = {"Content-Type": "application/octet-stream", "Accept-Ranges": "bytes"}
        behaviour, code, body = "file", 200, data
        asked, first, last = parse_range(self.headers.get("Range"), len(data))
        if rule("ignore-range") is not None:
            behaviour = "ignore-range"
        elif asked == "several":
            return self._send(record, "several-ranges", 501, {"Content-Type": "text/plain"}, b"501\n")
        elif asked == "unsatisfiable":
            return self._send(record, "range-not-satisfiable", 416,
                              {"Content-Type": "text/plain", "Content-Range": f"bytes */{len(data)}"}, b"")
        elif asked == "part":
            code, body = 206, data[first:last + 1]
            headers["Content-Range"] = f"bytes {first}-{last}/{len(data)}"
            record["range"] = [first, last]
        truncated = rule("truncate")
        if truncated is not None:
            keep = truncated.value if truncated.value > 0 else len(body) // 2
            return self._send(record, "truncate", code, headers, body, send=min(keep, len(body)))
        return self._send(record, behaviour, code, headers, body)

    def _redirect(self, feed: "FakeFeed", record: dict, behaviour: str, path: str) -> None:
        location = feed.storage_base + urllib.parse.quote(path) + "?" + REDIRECT_QUERY
        self._send(record, behaviour, 302, {"Location": location, "Content-Type": "text/plain"}, b"")

    def _github(self, feed: "FakeFeed", path: str, record: dict) -> bool:
        """The answers of github.com and api.github.com for a private repository. False: not theirs."""
        plain = {"Content-Type": "text/plain"}
        if path.startswith(f"/{feed.github}/releases/"):
            # An anonymous read of a private repository's release: github.com says there is nothing.
            self._send(record, "github-download-404", 404, plain, b"Not Found\n")
            return True
        prefix = f"/repos/{feed.github}/releases/"
        if not path.startswith(prefix):
            return False
        if not feed.authorises(self.headers.get("Authorization")):
            self._send(record, "github-api-unauthorised", 404, plain, b"Not Found\n")
            return True
        rest = path[len(prefix):]
        assets = feed.assets()
        tags = feed.tags()
        json_type = {"Content-Type": "application/json; charset=utf-8"}
        if rest == "latest" or (rest.startswith("tags/") and rest[len("tags/"):] in tags):
            if rest == "latest":
                tag = tags[-1] if tags else ""
            else:
                tag = rest[len("tags/"):]
            listing = {"tag_name": tag,
                       "assets": [{"id": number, "name": name, "size": size} for number, name, size in assets]}
            self._send(record, "github-release", 200, json_type, json.dumps(listing).encode("utf-8"))
            return True
        if rest.startswith("assets/") and rest[len("assets/"):].isdigit():
            wanted = int(rest[len("assets/"):])
            found = next((asset for asset in assets if asset[0] == wanted), None)
            if found is not None:
                if "application/octet-stream" in self.headers.get("Accept", ""):
                    self._redirect(feed, record, "github-asset-redirect", "/" + found[1])
                else:  # the asset's description, which is what the API gives when the bytes are not asked for
                    described = {"id": found[0], "name": found[1], "size": found[2]}
                    self._send(record, "github-asset", 200, json_type, json.dumps(described).encode("utf-8"))
                return True
        self._send(record, "github-api-404", 404, plain, b"Not Found\n")
        return True

    def _send(self, record: dict, behaviour: str, status: int, headers: dict[str, str], body: bytes,
              send: Optional[int] = None) -> None:
        """Answer with `body` announced in full, and written in full unless `send` says how much of it."""
        record["behaviour"] = behaviour
        record["status"] = status
        record["length"] = len(body)
        self.close_connection = True
        served = 0
        try:
            self.send_response(status)
            for name, value in headers.items():
                self.send_header(name, value)
            self.send_header("Content-Length", str(len(body)))
            self.send_header("Connection", "close")
            self.end_headers()
            sending = body if send is None else body[:send]
            for start in range(0, len(sending), CHUNK):
                piece = sending[start:start + CHUNK]
                self.wfile.write(piece)
                served += len(piece)
            self.wfile.flush()
        except OSError:
            pass  # the client went away: it had what it wanted, or would take no more
        record["served"] = served


class FakeFeed:
    """The two listeners, their rules and their log. Use it as a context manager, or start() and stop()."""

    def __init__(self, root: Path, rules: Iterable[Rule] = (), *, github: Optional[str] = None,
                 token: Optional[str] = None, storage_base: Optional[str] = None,
                 log_path: Optional[Path] = None):
        if github is not None and not re.fullmatch(r"[A-Za-z0-9._-]+/[A-Za-z0-9._-]+", github):
            raise ValueError(f"'{github}' is not owner/name")
        if github is not None and not token:
            raise ValueError("the private repository's answers need the token they are given for (--token)")
        self._root = Path(root)
        self._rules = tuple(rules)
        self.github = github
        self._token = token
        self._storage_base = storage_base.rstrip("/") if storage_base else None
        self._log_path = Path(log_path) if log_path else None
        self._lock = threading.Lock()
        self._settled = threading.Condition(self._lock)
        self._answering = 0  # requests begun and not yet in the log
        self._records: list[dict] = []
        self._servers: list[_Server] = []
        self._threads: list[threading.Thread] = []

    # ---- life --------------------------------------------------------------------------------

    def start(self) -> "FakeFeed":
        if self._servers:
            raise RuntimeError("the feed is already serving")
        if not self._root.is_dir():
            raise FileNotFoundError(f"{self._root} is not a folder")
        for which in ("first", "second"):
            server = _Server(self, which)
            thread = threading.Thread(target=server.serve_forever, kwargs={"poll_interval": 0.05},
                                      name=f"fake-feed-{which}", daemon=True)
            thread.start()
            self._servers.append(server)
            self._threads.append(thread)
        return self

    def stop(self) -> None:
        for server in self._servers:
            server.shutdown()
            server.server_close()
        for thread in self._threads:
            thread.join(timeout=10)
        self._servers, self._threads = [], []

    def __enter__(self) -> "FakeFeed":
        return self.start()

    def __exit__(self, *exc: object) -> None:
        self.stop()

    # ---- where it is ---------------------------------------------------------------------------

    @property
    def port(self) -> int:
        return self._servers[0].server_address[1]

    @property
    def second_port(self) -> int:
        return self._servers[1].server_address[1]

    @property
    def address(self) -> str:
        """The address both listeners are bound to, as the system reports it."""
        return self._servers[0].server_address[0]

    @property
    def url(self) -> str:
        return f"http://{LOOPBACK}:{self.port}"

    @property
    def second_url(self) -> str:
        return f"http://{LOOPBACK}:{self.second_port}"

    @property
    def storage_base(self) -> str:
        """What a redirect names: the second port, unless a test gave the name its client reaches it by."""
        return self._storage_base or self.second_url

    # ---- what it serves --------------------------------------------------------------------------

    def rules(self) -> tuple[Rule, ...]:
        with self._lock:
            return self._rules

    def set_rules(self, rules: Iterable[Rule]) -> None:
        """Replace the rules while serving: a feed that failed, then works."""
        with self._lock:
            self._rules = tuple(rules)

    def set_root(self, root: Path) -> None:
        """Serve another folder from now on: the release that is `latest` moved on."""
        with self._lock:
            self._root = Path(root)

    def file_for(self, path: str) -> Optional[Path]:
        """The file a request path names, or None: only plain files inside the folder are served."""
        with self._lock:
            root = self._root
        parts = [part for part in path.split("/") if part != ""]
        if not parts or any(part in (".", "..") or "\\" in part or ":" in part for part in parts):
            return None
        candidate = root.joinpath(*parts)
        if not candidate.is_file():
            return None
        try:
            candidate.resolve().relative_to(root.resolve())
        except ValueError:
            return None  # a link that leads out of the folder
        return candidate

    def authorises(self, header: Optional[str]) -> bool:
        return header is not None and header == f"Bearer {self._token}"

    def assets(self) -> list[tuple[int, str, int]]:
        """(id, name, size) of the folder's own files, as a release lists them. The ids follow the names'
        order, so they are the same for the same folder."""
        with self._lock:
            root = self._root
        names = sorted(entry.name for entry in root.iterdir() if entry.is_file())
        return [(FIRST_ASSET_ID + index, name, (root / name).stat().st_size) for index, name in enumerate(names)]

    def tags(self) -> list[str]:
        """The tags the folder's manifests name: the releases `tags/<tag>` knows."""
        with self._lock:
            root = self._root
        found = set()
        for manifest in sorted(root.glob("Katana-update-*.json")):
            try:
                tag = json.loads(manifest.read_text(encoding="utf-8")).get("tag")
            except (OSError, ValueError, AttributeError):
                continue
            if isinstance(tag, str) and tag:
                found.add(tag)
        return sorted(found)

    # ---- the log ---------------------------------------------------------------------------------

    def begin(self) -> None:
        """A request has arrived and is being answered; log() ends it."""
        with self._lock:
            self._answering += 1

    def log(self, record: dict) -> None:
        with self._settled:
            record["n"] = len(self._records) + 1
            self._records.append(record)
            if self._log_path is not None:
                with open(self._log_path, "a", encoding="utf-8", newline="\n") as sink:
                    sink.write(json.dumps(record, sort_keys=True) + "\n")
            self._answering -= 1
            self._settled.notify_all()

    def requests(self, port: Optional[str] = None, settle_seconds: float = 10.0) -> list[dict]:
        """A copy of the records so far, in the order the answers were finished; `port` keeps one port's.

        A record is written when its answer has been sent, which is a moment AFTER the client has read
        it. So this waits (up to `settle_seconds`) for every request that has begun to be in the log: a
        client that has its answer then finds its request here."""
        with self._settled:
            self._settled.wait_for(lambda: self._answering == 0, timeout=settle_seconds)
            return [json.loads(json.dumps(r)) for r in self._records if port is None or r["port"] == port]

    def clear_log(self, settle_seconds: float = 10.0) -> None:
        """Forget the records so far. Like requests(), it first waits (up to `settle_seconds`) for every
        request that has begun to be in the log: the record of an answer that is still being sent is one
        of those being forgotten, and must not turn up afterwards among the records of what follows."""
        with self._settled:
            self._settled.wait_for(lambda: self._answering == 0, timeout=settle_seconds)
            self._records = []


# ---- the command line ---------------------------------------------------------------------------


def _split_port(text: str) -> tuple[str, str]:
    for port in ("first", "second"):
        if text.endswith("@" + port):
            return text[: -len(port) - 1], port
    return text, "any"


def rules_from_arguments(args: argparse.Namespace) -> list[Rule]:
    """The rules the switches spell. ValueError names the switch that is wrong."""
    rules: list[Rule] = []
    for text in args.status:
        body, port = _split_port(text)
        code, separator, glob = body.partition(":")
        if not separator or not code.isdigit() or glob == "":
            raise ValueError(f"--status takes CODE:GLOB, not '{text}'")
        rules.append(Rule("status", glob, int(code), port))
    for kind, values in (("html", args.html), ("redirect", args.redirect), ("ignore-range", args.ignore_range)):
        for text in values:
            glob, port = _split_port(text)
            if glob == "":
                raise ValueError(f"--{kind} takes a GLOB")
            rules.append(Rule(kind, glob, 0, port))
    for text in args.truncate:
        body, port = _split_port(text)
        glob, separator, count = body.rpartition(":")
        if not separator or not count.isdigit():
            glob, count = body, "0"  # no count: the whole text is the pattern
        if glob == "" or glob.startswith(":"):
            raise ValueError(f"--truncate takes GLOB or GLOB:BYTES, not '{text}'")
        rules.append(Rule("truncate", glob, int(count), port))
    return rules


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="update_fake_feed.py",
        description="Serve a release folder on 127.0.0.1 for the update system's tests, misbehaving on request.",
        epilog=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument("--root", type=Path, required=True, help="the folder to serve: one release's files")
    parser.add_argument("--status", action="append", default=[], metavar="CODE:GLOB", help="answer CODE for these paths")
    parser.add_argument("--html", action="append", default=[], metavar="GLOB", help="answer with a web page")
    parser.add_argument("--redirect", action="append", default=[], metavar="GLOB", help="answer 302 to the second port")
    parser.add_argument("--ignore-range", action="append", default=[], metavar="GLOB", help="answer 200 with the whole file")
    parser.add_argument("--truncate", action="append", default=[], metavar="GLOB[:BYTES]", help="send only part of the body")
    parser.add_argument("--github", metavar="OWNER/NAME", help="answer as GitHub does for a private repository")
    parser.add_argument("--token", help="the token --github asks for")
    parser.add_argument("--storage-base", metavar="URL", help="what a redirect names instead of the second port")
    parser.add_argument("--log", type=Path, help="append one line of JSON for every request")
    parser.add_argument("--lifetime", type=float, default=3600.0, metavar="SECONDS", help="stop after this long (0: never)")
    parser.add_argument("--until-stdin-closes", action="store_true", help="also stop when standard input ends")
    return parser


def main(argv: Optional[list[str]] = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    try:
        feed = FakeFeed(args.root, rules_from_arguments(args), github=args.github, token=args.token,
                        storage_base=args.storage_base, log_path=args.log)
        feed.start()
    except (ValueError, OSError) as error:
        parser.print_usage(sys.stderr)
        print(f"update_fake_feed.py: error: {error}", file=sys.stderr)
        return 2
    stopped = threading.Event()
    if args.until_stdin_closes:
        def watch() -> None:
            try:
                while sys.stdin.buffer.read(4096):
                    pass
            except (OSError, ValueError):
                pass
            stopped.set()
        threading.Thread(target=watch, name="fake-feed-stdin", daemon=True).start()
    try:
        print(f"port={feed.port}\nsecond_port={feed.second_port}", flush=True)
        deadline = time.monotonic() + args.lifetime if args.lifetime > 0 else None
        while not stopped.is_set() and (deadline is None or time.monotonic() < deadline):
            stopped.wait(0.2)
    except KeyboardInterrupt:
        pass
    finally:
        feed.stop()
    return 0


if __name__ == "__main__":
    sys.exit(main())
