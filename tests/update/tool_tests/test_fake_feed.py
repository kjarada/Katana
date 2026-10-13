"""tools/update_fake_feed.py: the loopback release host the end-to-end tests fetch from.

It is asked with urllib, over real sockets on 127.0.0.1, with proxies turned off (an empty ProxyHandler: the
machine's proxy settings must not send a loopback request anywhere). What every answer is held to comes from
outside the feed: the bytes are the ones this test wrote into the folder, sliced here; the statuses and the
Content-Range forms are RFC 9110's (14.1.2 a range's first and last byte, 14.4 Content-Range, 15.5.17 the 416),
worked out by hand for a 5000-byte file; and what GitHub's API answers is the shape include/katana/update/feed.hpp
gives for it.
"""

from __future__ import annotations

import argparse
import http.client
import json
import socket
import subprocess
import sys
import threading
import unittest
import urllib.error
import urllib.request
from dataclasses import dataclass
from email.message import Message
from pathlib import Path
from typing import Optional

from support import TOOLS, ToolTest, noise

import update_fake_feed as fake  # noqa: E402  (support puts tools/ on sys.path)

ALPHA = noise("fake-feed-alpha", 5000)
INNER = b"a file one folder down\n"
MANIFEST = b'{"tag":"v9.9.9","version":"9.9.9"}\n'
SECRET = b"this file is beside the served folder, never in it\n"


class _NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):  # noqa: ANN001  (urllib's signature)
        return None  # the 302 itself is the answer


# No proxy, whatever the environment says: these requests go to 127.0.0.1 and nowhere else.
DIRECT = urllib.request.build_opener(urllib.request.ProxyHandler({}))
NO_FOLLOW = urllib.request.build_opener(urllib.request.ProxyHandler({}), _NoRedirect())


@dataclass
class Reply:
    status: int
    headers: Message
    body: bytes
    cut_short: bool = False  # the connection ended before the announced length had arrived


def get(url: str, headers: Optional[dict[str, str]] = None, *, follow: bool = True,
        unredirected: Optional[dict[str, str]] = None, method: str = "GET") -> Reply:
    request = urllib.request.Request(url, headers=headers or {}, method=method)
    for name, value in (unredirected or {}).items():
        request.add_unredirected_header(name, value)
    try:
        with (DIRECT if follow else NO_FOLLOW).open(request, timeout=30) as response:
            try:
                return Reply(response.status, response.headers, response.read())
            except http.client.IncompleteRead as cut:
                return Reply(response.status, response.headers, cut.partial, cut_short=True)
    except urllib.error.HTTPError as error:
        with error:
            return Reply(error.code, error.headers, error.read())


class FeedTest(ToolTest):
    """A folder to serve, with a file beside it that must never be served."""

    def setUp(self) -> None:
        super().setUp()
        self.root = self.tmp / "release"
        (self.root / "sub").mkdir(parents=True)
        (self.root / "alpha.bin").write_bytes(ALPHA)
        (self.root / "empty.bin").write_bytes(b"")
        (self.root / "Katana-update-win64.json").write_bytes(MANIFEST)
        (self.root / "sub" / "inner.txt").write_bytes(INNER)
        (self.tmp / "secret.txt").write_bytes(SECRET)

    def serve(self, *rules: fake.Rule, **options: object) -> fake.FakeFeed:
        feed = fake.FakeFeed(self.root, rules, **options)  # type: ignore[arg-type]
        feed.start()
        self.addCleanup(feed.stop)
        return feed


class WhereItListens(FeedTest):
    def test_both_listeners_are_on_the_loopback_address_on_two_ports_the_system_chose(self) -> None:
        feed = self.serve()
        self.assertEqual(feed.address, "127.0.0.1")
        self.assertNotEqual(feed.port, 0)
        self.assertNotEqual(feed.second_port, 0)
        self.assertNotEqual(feed.port, feed.second_port)
        self.assertEqual(feed.url, f"http://127.0.0.1:{feed.port}")
        self.assertEqual(get(feed.url + "/alpha.bin").body, ALPHA)
        self.assertEqual(get(feed.second_url + "/alpha.bin").body, ALPHA)

    def test_a_folder_that_is_not_there_is_refused_before_anything_listens(self) -> None:
        with self.assertRaises(FileNotFoundError):
            fake.FakeFeed(self.tmp / "no-such-folder").start()


class WhatItServesByItself(FeedTest):
    def test_a_file_is_served_whole_with_its_length(self) -> None:
        reply = get(self.serve().url + "/alpha.bin")
        self.assertEqual(reply.status, 200)
        self.assertEqual(reply.body, ALPHA)
        self.assertEqual(reply.headers["Content-Length"], "5000")
        self.assertEqual(reply.headers["Accept-Ranges"], "bytes")
        self.assertIsNone(reply.headers["Content-Range"])

    def test_a_file_in_a_folder_and_an_empty_file_are_served(self) -> None:
        feed = self.serve()
        self.assertEqual(get(feed.url + "/sub/inner.txt").body, INNER)
        empty = get(feed.url + "/empty.bin")
        self.assertEqual((empty.status, empty.body, empty.headers["Content-Length"]), (200, b"", "0"))

    def test_a_range_is_answered_206_with_exactly_those_bytes_and_their_content_range(self) -> None:
        feed = self.serve()
        # RFC 9110, 14.1.2: first-last inclusive; `a-` to the end; `-n` the last n; a last byte past the
        # end means the end. The file is 5000 bytes, so its last byte is 4999.
        cases = [
            ("bytes=10-19", ALPHA[10:20], "bytes 10-19/5000"),
            ("bytes=0-0", ALPHA[0:1], "bytes 0-0/5000"),
            ("bytes=4990-", ALPHA[4990:], "bytes 4990-4999/5000"),
            ("bytes=-7", ALPHA[4993:], "bytes 4993-4999/5000"),
            ("bytes=4990-9999", ALPHA[4990:], "bytes 4990-4999/5000"),
            ("bytes=0-4999", ALPHA, "bytes 0-4999/5000"),
            ("bytes=-9999", ALPHA, "bytes 0-4999/5000"),
        ]
        for asked, expected, content_range in cases:
            with self.subTest(asked):
                reply = get(feed.url + "/alpha.bin", {"Range": asked})
                self.assertEqual(reply.status, 206)
                self.assertEqual(reply.body, expected)
                self.assertEqual(reply.headers["Content-Range"], content_range)
                self.assertEqual(reply.headers["Content-Length"], str(len(expected)))

    def test_a_range_past_the_end_is_416_naming_the_length(self) -> None:
        feed = self.serve()
        for path, asked, content_range in [("/alpha.bin", "bytes=5000-", "bytes */5000"),
                                           ("/alpha.bin", "bytes=5000-5001", "bytes */5000"),
                                           ("/alpha.bin", "bytes=-0", "bytes */5000"),
                                           ("/empty.bin", "bytes=0-0", "bytes */0"),
                                           ("/empty.bin", "bytes=-5", "bytes */0")]:
            with self.subTest(path=path, asked=asked):
                reply = get(feed.url + path, {"Range": asked})
                self.assertEqual(reply.status, 416)
                self.assertEqual(reply.headers["Content-Range"], content_range)
                self.assertEqual(reply.body, b"")

    def test_several_ranges_in_one_request_are_501_as_the_release_host_answered(self) -> None:
        reply = get(self.serve().url + "/alpha.bin", {"Range": "bytes=0-1,5-6"})
        self.assertEqual(reply.status, 501)

    def test_a_range_header_that_is_not_a_range_of_bytes_is_ignored(self) -> None:
        feed = self.serve()
        for asked in ("lines=1-2", "bytes=9-3", "bytes=-", "bytes=a-b", "bytes 0-9"):
            with self.subTest(asked):
                reply = get(feed.url + "/alpha.bin", {"Range": asked})
                self.assertEqual((reply.status, reply.body), (200, ALPHA))

    def test_what_is_not_a_file_of_the_folder_is_404_and_nothing_outside_it_is_served(self) -> None:
        feed = self.serve()
        for path in ("/", "/missing.bin", "/sub", "/sub/", "/../secret.txt", "/%2e%2e/secret.txt",
                     "/sub/../../secret.txt", "/sub/%2e%2e/%2e%2e/secret.txt", "/..%5csecret.txt"):
            with self.subTest(path):
                reply = get(feed.url + path)
                self.assertEqual(reply.status, 404)
                self.assertNotIn(b"beside the served folder", reply.body)

    def test_a_method_other_than_get_is_501_with_no_body_and_is_in_the_log(self) -> None:
        feed = self.serve()
        for method in ("HEAD", "DELETE"):
            with self.subTest(method):
                reply = get(feed.url + "/alpha.bin?x=1", {"X-Marker": "seen"}, method=method)
                self.assertEqual((reply.status, reply.body, reply.headers["Content-Length"]), (501, b"", "0"))
        # The log is what proves which requests were made: one made in a way no test expected is IN it.
        records = feed.requests()
        self.assertEqual([(r["n"], r["port"], r["method"], r["path"], r["status"], r["behaviour"], r["length"],
                           r["served"]) for r in records],
                         [(1, "first", "HEAD", "/alpha.bin?x=1", 501, "refused", 0, 0),
                          (2, "first", "DELETE", "/alpha.bin?x=1", 501, "refused", 0, 0)])
        self.assertEqual([r["headers"].get("x-marker") for r in records], ["seen", "seen"])

    def test_a_request_that_cannot_be_read_as_one_is_400_and_is_in_the_log_with_what_was_read(self) -> None:
        feed = self.serve()
        # One word where a method, an address and a version belong (RFC 9112, 3: the request line), sent
        # to the second port. urllib cannot be made to say this, so it goes over a socket of the test's own.
        with socket.create_connection(("127.0.0.1", feed.second_port), timeout=30) as raw:
            raw.sendall(b"HELLO\r\n")
            answer = b""
            while True:
                piece = raw.recv(4096)
                if not piece:
                    break
                answer += piece
        head, blank, body = answer.partition(b"\r\n\r\n")
        lines = head.split(b"\r\n")
        self.assertEqual(lines[0], b"HTTP/1.1 400 Bad Request")
        self.assertIn(b"Content-Length: 0", lines[1:])
        self.assertEqual((blank, body), (b"\r\n\r\n", b""))
        self.assertEqual(feed.requests(), [{"n": 1, "port": "second", "method": "", "path": "", "headers": {},
                                            "status": 400, "behaviour": "refused", "range": None, "length": 0,
                                            "served": 0}])


class TheRangeReader(unittest.TestCase):
    def test_it_reads_a_range_as_rfc_9110_does(self) -> None:
        cases = [
            (None, 100, ("whole", 0, 0)),
            ("bytes=0-9", 100, ("part", 0, 9)),
            ("bytes=90-", 100, ("part", 90, 99)),
            ("bytes=-10", 100, ("part", 90, 99)),
            ("bytes=-200", 100, ("part", 0, 99)),
            ("bytes=99-150", 100, ("part", 99, 99)),
            ("  bytes=5-6  ", 100, ("part", 5, 6)),
            ("bytes=100-", 100, ("unsatisfiable", 0, 0)),
            ("bytes=-0", 100, ("unsatisfiable", 0, 0)),
            ("bytes=0-", 0, ("unsatisfiable", 0, 0)),
            ("bytes=0-1,4-5", 100, ("several", 0, 0)),
            ("bytes=7-3", 100, ("whole", 0, 0)),
            ("bytes=-", 100, ("whole", 0, 0)),
            ("items=0-9", 100, ("whole", 0, 0)),
        ]
        for header, size, expected in cases:
            with self.subTest(header=header, size=size):
                self.assertEqual(fake.parse_range(header, size), expected)


class Misbehaving(FeedTest):
    def test_a_status_rule_answers_that_status_for_its_paths_and_no_others(self) -> None:
        feed = self.serve(fake.Rule("status", "*.json", 403), fake.Rule("status", "/sub/*", 429),
                          fake.Rule("status", "/empty.bin", 404))
        self.assertEqual(get(feed.url + "/Katana-update-win64.json").status, 403)
        self.assertEqual(get(feed.url + "/sub/inner.txt").status, 429)
        self.assertEqual(get(feed.url + "/empty.bin").status, 404)
        self.assertEqual(get(feed.url + "/alpha.bin").body, ALPHA)
        self.assertEqual([r["behaviour"] for r in feed.requests()], ["status-403", "status-429", "status-404", "file"])

    def test_a_status_rule_must_name_a_failure(self) -> None:
        for code in (200, 302, 399, 600):
            with self.subTest(code), self.assertRaises(ValueError):
                fake.Rule("status", "*", code)

    def test_an_html_rule_answers_every_address_with_a_page_even_one_that_names_no_file(self) -> None:
        feed = self.serve(fake.Rule("html"))
        for path in ("/alpha.bin", "/missing.bin"):
            with self.subTest(path):
                reply = get(feed.url + path, {"Range": "bytes=0-9"})
                self.assertEqual(reply.status, 200)
                self.assertEqual(reply.headers.get_content_type(), "text/html")
                self.assertTrue(reply.body.startswith(b"<!DOCTYPE html>"))

    def test_an_ignored_range_is_answered_200_with_the_whole_file(self) -> None:
        feed = self.serve(fake.Rule("ignore-range", "*.bin"))
        reply = get(feed.url + "/alpha.bin", {"Range": "bytes=10-19"})
        self.assertEqual((reply.status, reply.body), (200, ALPHA))
        self.assertIsNone(reply.headers["Content-Range"])
        self.assertEqual(feed.requests()[0]["behaviour"], "ignore-range")
        # ... and only where the rule says: another file's range is still a range.
        self.assertEqual(get(feed.url + "/sub/inner.txt", {"Range": "bytes=0-0"}).status, 206)

    def test_a_truncated_body_announces_all_of_it_and_sends_half(self) -> None:
        feed = self.serve(fake.Rule("truncate", "/alpha.bin"))
        reply = get(feed.url + "/alpha.bin")
        self.assertTrue(reply.cut_short)
        self.assertEqual(reply.headers["Content-Length"], "5000")
        self.assertEqual(reply.body, ALPHA[:2500])
        record = feed.requests()[0]
        self.assertEqual((record["behaviour"], record["length"], record["served"]), ("truncate", 5000, 2500))

    def test_a_truncated_range_is_cut_inside_the_range_and_a_byte_count_is_obeyed(self) -> None:
        feed = self.serve(fake.Rule("truncate", "/alpha.bin", 100))
        ranged = get(feed.url + "/alpha.bin", {"Range": "bytes=1000-1999"})
        self.assertTrue(ranged.cut_short)
        self.assertEqual(ranged.status, 206)
        self.assertEqual(ranged.headers["Content-Range"], "bytes 1000-1999/5000")
        self.assertEqual(ranged.headers["Content-Length"], "1000")
        self.assertEqual(ranged.body, ALPHA[1000:1100])
        # A count past the body sends the body: nothing is invented.
        feed.set_rules([fake.Rule("truncate", "/sub/*", 10_000)])
        whole = get(feed.url + "/sub/inner.txt")
        self.assertEqual((whole.cut_short, whole.body), (False, INNER))

    def test_a_rule_written_for_one_port_leaves_the_other_alone(self) -> None:
        feed = self.serve(fake.Rule("status", "*", 403, "second"))
        self.assertEqual(get(feed.url + "/alpha.bin").status, 200)
        self.assertEqual(get(feed.second_url + "/alpha.bin").status, 403)
        feed.set_rules([fake.Rule("ignore-range", "*", 0, "first")])
        self.assertEqual(get(feed.url + "/alpha.bin", {"Range": "bytes=0-0"}).status, 200)
        self.assertEqual(get(feed.second_url + "/alpha.bin", {"Range": "bytes=0-0"}).status, 206)

    def test_the_rules_can_be_replaced_while_it_serves(self) -> None:
        feed = self.serve(fake.Rule("status", "*", 429))
        self.assertEqual(get(feed.url + "/alpha.bin").status, 429)
        feed.set_rules([])
        self.assertEqual(get(feed.url + "/alpha.bin").body, ALPHA)

    def test_the_folder_can_be_replaced_while_it_serves(self) -> None:
        feed = self.serve()
        other = self.tmp / "other"
        other.mkdir()
        (other / "alpha.bin").write_bytes(b"another release\n")
        feed.set_root(other)
        self.assertEqual(get(feed.url + "/alpha.bin").body, b"another release\n")
        self.assertEqual(get(feed.url + "/empty.bin").status, 404)

    def test_an_unknown_kind_of_rule_or_port_is_refused(self) -> None:
        with self.assertRaises(ValueError):
            fake.Rule("teapot")
        with self.assertRaises(ValueError):
            fake.Rule("html", "*", 0, "third")
        with self.assertRaises(ValueError):
            fake.Rule("redirect", "*", 0, "second")


class TheRedirectHop(FeedTest):
    def test_a_redirected_path_is_answered_302_to_the_same_path_on_the_second_port_with_a_query(self) -> None:
        feed = self.serve(fake.Rule("redirect", "*.bin"))
        reply = get(feed.url + "/alpha.bin", follow=False)
        self.assertEqual(reply.status, 302)
        self.assertEqual(reply.headers["Location"],
                         f"http://127.0.0.1:{feed.second_port}/alpha.bin?sig=FAKE-SIGNED-ADDRESS-SECRET")
        # Only what the rule names: the manifest is served where it was asked for.
        self.assertEqual(get(feed.url + "/Katana-update-win64.json", follow=False).status, 200)

    def test_the_second_port_serves_the_range_and_logs_the_headers_it_was_sent(self) -> None:
        feed = self.serve(fake.Rule("redirect", "*.bin"))
        reply = get(feed.url + "/alpha.bin", {"Range": "bytes=10-19", "X-Marker": "kept"})
        self.assertEqual((reply.status, reply.body), (206, ALPHA[10:20]))
        first, second = feed.requests()
        self.assertEqual((first["port"], first["status"], first["behaviour"], first["served"]),
                         ("first", 302, "redirect", 0))
        self.assertEqual((second["port"], second["status"], second["range"], second["served"]),
                         ("second", 206, [10, 19], 10))
        self.assertEqual(second["path"], "/alpha.bin?sig=FAKE-SIGNED-ADDRESS-SECRET")
        self.assertEqual(second["headers"]["range"], "bytes=10-19")
        self.assertEqual(second["headers"]["x-marker"], "kept")
        self.assertEqual(second["headers"]["host"], f"127.0.0.1:{feed.second_port}")

    def test_the_log_tells_a_client_that_carries_authorization_to_the_second_hop_from_one_that_does_not(self) -> None:
        feed = self.serve(fake.Rule("redirect", "*.bin"))
        # urllib sends a request's own headers again on a redirect: the log must show the token there ...
        get(feed.url + "/alpha.bin", {"Authorization": "Bearer LEAKED"})
        leaked = feed.requests()
        self.assertEqual(leaked[0]["headers"]["authorization"], "Bearer LEAKED")
        self.assertEqual(leaked[1]["headers"].get("authorization"), "Bearer LEAKED")
        # ... and not when the client keeps it to the first hop, which is the rule the engine is held to.
        feed.clear_log()
        get(feed.url + "/alpha.bin", unredirected={"Authorization": "Bearer KEPT"})
        kept = feed.requests()
        self.assertEqual(kept[0]["headers"]["authorization"], "Bearer KEPT")
        self.assertNotIn("authorization", kept[1]["headers"])

    def test_a_storage_base_is_what_the_redirect_names_instead_of_the_second_port(self) -> None:
        feed = self.serve(fake.Rule("redirect", "*"), storage_base="https://storage.example.invalid/")
        reply = get(feed.url + "/sub/inner.txt", follow=False)
        self.assertEqual(reply.headers["Location"],
                         "https://storage.example.invalid/sub/inner.txt?sig=FAKE-SIGNED-ADDRESS-SECRET")

    def test_the_second_port_redirects_nothing(self) -> None:
        feed = self.serve(fake.Rule("redirect", "*"))
        self.assertEqual(get(feed.second_url + "/alpha.bin", follow=False).status, 200)


class TheLog(FeedTest):
    def test_every_request_is_a_record_of_what_was_asked_and_what_was_served(self) -> None:
        log = self.tmp / "requests.jsonl"
        feed = self.serve(log_path=log)
        get(feed.url + "/alpha.bin?x=1", {"User-Agent": "Katana/0.0 (test)"})
        get(feed.url + "/alpha.bin", {"Range": "bytes=100-299"})
        get(feed.url + "/missing.bin")
        get(feed.second_url + "/empty.bin")
        records = feed.requests()
        self.assertEqual([r["n"] for r in records], [1, 2, 3, 4])
        self.assertEqual([r["port"] for r in records], ["first", "first", "first", "second"])
        self.assertEqual([r["method"] for r in records], ["GET"] * 4)
        self.assertEqual([r["path"] for r in records], ["/alpha.bin?x=1", "/alpha.bin", "/missing.bin", "/empty.bin"])
        self.assertEqual([r["status"] for r in records], [200, 206, 404, 200])
        self.assertEqual([r["behaviour"] for r in records], ["file", "file", "missing", "file"])
        self.assertEqual([r["range"] for r in records], [None, [100, 299], None, None])
        self.assertEqual([r["length"] for r in records], [5000, 200, 4, 0])
        self.assertEqual([r["served"] for r in records], [5000, 200, 4, 0])
        self.assertEqual(records[0]["headers"]["user-agent"], "Katana/0.0 (test)")
        self.assertEqual(feed.requests("second"), records[3:])
        # The file holds the same records, a line each.
        lines = [json.loads(line) for line in log.read_text(encoding="utf-8").splitlines()]
        self.assertEqual(lines, records)
        feed.clear_log()
        self.assertEqual(feed.requests(), [])

    def test_clearing_the_log_waits_for_an_answer_that_is_still_being_sent(self) -> None:
        # A record is written when its answer has been sent. So a log cleared while an answer is on its
        # way must wait for that record and forget it with the rest: left to arrive afterwards, it would be
        # counted among the requests of whatever the test does next.
        #
        # The answer is held up from this side: 32 MiB that the client does not read. The loopback takes a
        # few megabytes of that into its buffers at the most, so the feed is still writing the body.
        size = 32 * 1024 * 1024
        (self.root / "big.bin").write_bytes(bytes(size))
        feed = self.serve()
        connection = http.client.HTTPConnection("127.0.0.1", feed.port, timeout=60)
        self.addCleanup(connection.close)
        connection.request("GET", "/big.bin")
        response = connection.getresponse()  # the headers are here: the answer has begun, its body is unread
        self.assertEqual((response.status, response.getheader("Content-Length")), (200, str(size)))
        clearing = threading.Thread(target=feed.clear_log, name="clearing-the-log", daemon=True)
        clearing.start()
        clearing.join(timeout=0.5)
        self.assertTrue(clearing.is_alive(), "clear_log returned while an answer was still being sent")
        self.assertEqual(len(response.read()), size)
        clearing.join(timeout=60)
        self.assertFalse(clearing.is_alive(), "clear_log did not return once the answer had been sent")
        self.assertEqual(feed.requests(), [])
        # What follows is the first record of a log that starts again.
        get(feed.url + "/alpha.bin")
        self.assertEqual([(r["n"], r["path"]) for r in feed.requests()], [(1, "/alpha.bin")])

    def test_clearing_the_log_does_not_wait_for_ever_for_an_answer_nobody_reads(self) -> None:
        (self.root / "big.bin").write_bytes(bytes(32 * 1024 * 1024))
        feed = self.serve()
        connection = http.client.HTTPConnection("127.0.0.1", feed.port, timeout=60)
        self.addCleanup(connection.close)
        connection.request("GET", "/big.bin")
        connection.getresponse()
        feed.clear_log(settle_seconds=0.2)  # returns: a test that hangs proves nothing
        self.assertEqual(feed.requests(settle_seconds=0.0), [])


class APrivateRepositoryOnGitHub(FeedTest):
    TOKEN = "fake-token-0123456789"

    def github(self, *rules: fake.Rule) -> fake.FakeFeed:
        return self.serve(*rules, github="owner/repo", token=self.TOKEN)

    def auth(self, **more: str) -> dict[str, str]:
        return {"Authorization": f"Bearer {self.TOKEN}", **more}

    def test_the_release_addresses_answer_an_anonymous_read_with_404(self) -> None:
        feed = self.github()
        for path in ("/owner/repo/releases/latest/download/Katana-update-win64.json",
                     "/owner/repo/releases/download/v9.9.9/alpha.bin"):
            with self.subTest(path):
                self.assertEqual(get(feed.url + path).status, 404)
        self.assertEqual({r["behaviour"] for r in feed.requests()}, {"github-download-404"})

    def test_the_api_answers_404_without_the_token_and_with_another(self) -> None:
        feed = self.github()
        for headers in ({}, {"Authorization": "Bearer something-else"}, {"Authorization": self.TOKEN}):
            with self.subTest(headers=headers):
                self.assertEqual(get(feed.url + "/repos/owner/repo/releases/latest", headers).status, 404)
        self.assertEqual({r["behaviour"] for r in feed.requests()}, {"github-api-unauthorised"})

    def test_the_latest_release_lists_the_folders_files_as_assets_with_their_sizes(self) -> None:
        feed = self.github()
        reply = get(feed.url + "/repos/owner/repo/releases/latest", self.auth())
        self.assertEqual(reply.status, 200)
        self.assertEqual(reply.headers.get_content_type(), "application/json")
        # The folder's own files in the order of their names (upper case first, as bytes sort), numbered
        # from 1001; the file in sub/ is not an asset.
        self.assertEqual(json.loads(reply.body), {
            "tag_name": "v9.9.9",
            "assets": [{"id": 1001, "name": "Katana-update-win64.json", "size": len(MANIFEST)},
                       {"id": 1002, "name": "alpha.bin", "size": 5000},
                       {"id": 1003, "name": "empty.bin", "size": 0}],
        })

    def test_a_release_is_found_by_the_tag_its_manifest_names_and_by_no_other(self) -> None:
        feed = self.github()
        by_tag = get(feed.url + "/repos/owner/repo/releases/tags/v9.9.9", self.auth())
        self.assertEqual(by_tag.status, 200)
        self.assertEqual(json.loads(by_tag.body)["tag_name"], "v9.9.9")
        self.assertEqual(len(json.loads(by_tag.body)["assets"]), 3)
        self.assertEqual(get(feed.url + "/repos/owner/repo/releases/tags/v1.0.0", self.auth()).status, 404)

    def test_an_asset_asked_for_as_bytes_is_redirected_to_the_second_port_which_wants_no_token(self) -> None:
        feed = self.github()
        asset = feed.url + "/repos/owner/repo/releases/assets/1002"
        hop = get(asset, self.auth(Accept="application/octet-stream"), follow=False)
        self.assertEqual(hop.status, 302)
        self.assertEqual(hop.headers["Location"],
                         f"http://127.0.0.1:{feed.second_port}/alpha.bin?sig=FAKE-SIGNED-ADDRESS-SECRET")
        ranged = get(hop.headers["Location"], {"Range": "bytes=0-9"})
        self.assertEqual((ranged.status, ranged.body), (206, ALPHA[:10]))
        self.assertNotIn("authorization", feed.requests("second")[0]["headers"])

    def test_an_asset_not_asked_for_as_bytes_is_described_and_an_unknown_one_is_404(self) -> None:
        feed = self.github()
        described = get(feed.url + "/repos/owner/repo/releases/assets/1002", self.auth())
        self.assertEqual(json.loads(described.body), {"id": 1002, "name": "alpha.bin", "size": 5000})
        for path in ("/repos/owner/repo/releases/assets/1999", "/repos/owner/repo/releases/assets/x",
                     "/repos/owner/repo/releases/nothing"):
            with self.subTest(path):
                self.assertEqual(get(feed.url + path, self.auth(Accept="application/octet-stream")).status, 404)

    def test_a_status_rule_comes_before_the_api_as_a_rate_limit_does(self) -> None:
        feed = self.github(fake.Rule("status", "/repos/*", 403))
        self.assertEqual(get(feed.url + "/repos/owner/repo/releases/latest", self.auth()).status, 403)

    def test_another_repository_is_not_this_one(self) -> None:
        feed = self.github()
        self.assertEqual(get(feed.url + "/repos/owner/other/releases/latest", self.auth()).status, 404)
        self.assertEqual(feed.requests()[0]["behaviour"], "missing")

    def test_the_private_answers_need_a_repository_name_and_the_token(self) -> None:
        with self.assertRaises(ValueError):
            fake.FakeFeed(self.root, github="owner/repo")
        with self.assertRaises(ValueError):
            fake.FakeFeed(self.root, github="not-a-repository", token="t")


class TheCommandLine(FeedTest):
    SCRIPT = TOOLS / "update_fake_feed.py"

    def start(self, *args: object) -> subprocess.Popen:
        process = subprocess.Popen(
            [sys.executable, "-B", str(self.SCRIPT), "--root", str(self.root), "--until-stdin-closes", *map(str, args)],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, encoding="utf-8",
        )

        def end() -> None:
            if process.poll() is None:
                process.kill()
            process.communicate()

        self.addCleanup(end)
        return process

    def test_it_prints_the_ports_it_got_serves_and_stops_when_its_input_closes(self) -> None:
        log = self.tmp / "requests.jsonl"
        process = self.start("--status", "403:*.json", "--redirect", "/sub/*", "--log", log)
        assert process.stdout is not None and process.stdin is not None
        first, second = process.stdout.readline(), process.stdout.readline()
        self.assertRegex(first, r"^port=[1-9][0-9]*\n$")
        self.assertRegex(second, r"^second_port=[1-9][0-9]*\n$")
        port, second_port = int(first.split("=")[1]), int(second.split("=")[1])
        self.assertNotEqual(port, second_port)
        base = f"http://127.0.0.1:{port}"
        self.assertEqual(get(base + "/alpha.bin", {"Range": "bytes=0-4"}).body, ALPHA[:5])
        self.assertEqual(get(base + "/Katana-update-win64.json").status, 403)
        self.assertEqual(get(base + "/sub/inner.txt", follow=False).headers["Location"],
                         f"http://127.0.0.1:{second_port}/sub/inner.txt?sig=FAKE-SIGNED-ADDRESS-SECRET")
        process.stdin.close()
        self.assertEqual(process.wait(timeout=60), 0)
        records = [json.loads(line) for line in log.read_text(encoding="utf-8").splitlines()]
        self.assertEqual([(r["status"], r["served"]) for r in records], [(206, 5), (403, 4), (302, 0)])

    def test_a_lifetime_ends_it_by_itself(self) -> None:
        process = subprocess.run(
            [sys.executable, "-B", str(self.SCRIPT), "--root", str(self.root), "--lifetime", "0.3"],
            stdin=subprocess.DEVNULL, capture_output=True, text=True, encoding="utf-8", timeout=120, check=False,
        )
        self.assertEqual(process.returncode, 0, process.stderr)
        self.assertRegex(process.stdout, r"^port=[1-9][0-9]*\nsecond_port=[1-9][0-9]*\n$")

    def test_a_wrong_switch_is_a_usage_error_and_nothing_is_served(self) -> None:
        for args in (["--status", "teapot"], ["--status", "200:*"], ["--truncate", ":5"], ["--html", ""],
                     ["--github", "owner/repo"], ["--redirect", "*@second"]):
            with self.subTest(args):
                done = subprocess.run(
                    [sys.executable, "-B", str(self.SCRIPT), "--root", str(self.root), *args],
                    stdin=subprocess.DEVNULL, capture_output=True, text=True, encoding="utf-8", timeout=120, check=False,
                )
                self.assertEqual(done.returncode, 2, done.stderr)
                self.assertEqual(done.stdout, "")
        missing = subprocess.run(
            [sys.executable, "-B", str(self.SCRIPT), "--root", str(self.tmp / "no-such-folder")],
            stdin=subprocess.DEVNULL, capture_output=True, text=True, encoding="utf-8", timeout=120, check=False,
        )
        self.assertEqual((missing.returncode, missing.stdout), (2, ""))

    def test_the_switches_spell_the_rules(self) -> None:
        args = fake.build_parser().parse_args([
            "--root", "x", "--status", "429:*", "--status", "403:/a/*.sig@second", "--html", "*.json@first",
            "--redirect", "*.zip", "--ignore-range", "*.zip@second", "--truncate", "*.zip", "--truncate", "/b:c.zip:64@first",
        ])
        self.assertIsInstance(args, argparse.Namespace)
        self.assertEqual(fake.rules_from_arguments(args), [
            fake.Rule("status", "*", 429, "any"),
            fake.Rule("status", "/a/*.sig", 403, "second"),
            fake.Rule("html", "*.json", 0, "first"),
            fake.Rule("redirect", "*.zip", 0, "any"),
            fake.Rule("ignore-range", "*.zip", 0, "second"),
            fake.Rule("truncate", "*.zip", 0, "any"),
            fake.Rule("truncate", "/b:c.zip", 64, "first"),
        ])


if __name__ == "__main__":
    unittest.main()
