//! An MCP client of the server in the tests: the twin of the McpServer
//! fixture at the head of tests/app/test_mcp_server.cpp (and of the request
//! helpers of the other C++ files that drive the server). A session of no
//! program and a server over it, driven message by message as a client
//! drives katana_mcp, without a process; the session is lent to the server
//! for each message, so a test reads the drawing between them.
//!
//! One owner (step 6's C); a target includes it with
//! `#[path = "../support/mcp_client.rs"] pub mod mcp_client;`.

#![allow(
    dead_code,
    reason = "Each test target that includes the client uses a part of it."
)]

use katana_app::mcp::Server;
use katana_app::{Captured, Session};
use katana_core::json::Json;

/// A session and the server it is lent to, with the next message's id.
pub struct Client {
    pub session: Session,
    pub server: Server,
    next_id: i64,
}

impl Default for Client {
    fn default() -> Self {
        Self::new()
    }
}

impl Client {
    /// The C++ fixture's: a session of no program (no customisation, so what
    /// the tests see does not depend on whether this checkout has the
    /// reference one) and a server of version 9.9.9.
    pub fn new() -> Self {
        Self::over(Session::new(None, &mut Captured::default()))
    }

    /// A server over `session` - a test's own, a PROGRAM's session, which the
    /// fixture's is not.
    pub fn over(session: Session) -> Self {
        Self {
            session,
            server: Server::new("9.9.9"),
            next_id: 1,
        }
    }

    /// A request's reply, parsed: the C++'s requestOf.
    #[track_caller]
    pub fn request(&mut self, method: &str, params: Json) -> Json {
        let message = Json::object_of([
            ("jsonrpc", Json::from("2.0")),
            ("id", Json::from(self.next_id)),
            ("method", Json::from(method)),
            ("params", params),
        ]);
        self.next_id += 1;
        let Some(reply) = self.server.handle(&mut self.session, message.dump()) else {
            panic!("{method} was not answered");
        };
        Json::parse(reply).expect("a reply is JSON")
    }

    /// A tool's result: the C++'s callOf.
    #[track_caller]
    pub fn call(&mut self, tool: &str, arguments: Json) -> Json {
        let reply = self.request(
            "tools/call",
            Json::object_of([("name", Json::from(tool)), ("arguments", arguments)]),
        );
        assert!(reply.contains("result"), "{}", reply.dump());
        reply.find("result").cloned().unwrap_or_else(Json::object)
    }

    /// A tool called with no arguments.
    #[track_caller]
    pub fn call_bare(&mut self, tool: &str) -> Json {
        self.call(tool, Json::object())
    }

    /// initialize at the latest revision, as a client opens a session.
    #[track_caller]
    pub fn initialize(&mut self) {
        let _ = self.request(
            "initialize",
            Json::object_of([
                ("protocolVersion", Json::from("2025-06-18")),
                ("capabilities", Json::object()),
                (
                    "clientInfo",
                    Json::object_of([("name", Json::from("test")), ("version", Json::from("1"))]),
                ),
            ]),
        );
    }

    /// One line through katana_run_commands.
    #[track_caller]
    pub fn run_line(&mut self, line: &str) -> Json {
        self.call("katana_run_commands", commands(&[line]))
    }

    /// Lines through katana_run_commands.
    #[track_caller]
    pub fn run_lines(&mut self, lines: &[&str]) -> Json {
        self.call("katana_run_commands", commands(lines))
    }
}

/// A tool result's text: result["content"][0]["text"].
#[track_caller]
pub fn text_of(result: &Json) -> String {
    String::from(
        result["content"][0]["text"]
            .as_str()
            .expect("a tool's content is text"),
    )
}

/// A tool result's isError.
#[track_caller]
pub fn is_error(result: &Json) -> bool {
    result["isError"]
        .as_bool()
        .expect("a tool's isError is a boolean")
}

/// `result.value("isError", true)`: a result that does not say is a failure.
#[track_caller]
pub fn failed(result: &Json) -> bool {
    result
        .value("isError", true)
        .expect("a tool's isError is a boolean")
}

/// A string member's text: `json.get<std::string>()`.
#[track_caller]
pub fn string_of(json: &Json) -> String {
    String::from(json.as_str().expect("a string"))
}

/// A JSON object of these members: the C++'s `Json{{"a", ...}, {"b", ...}}`.
pub fn object<const N: usize>(members: [(&str, Json); N]) -> Json {
    Json::object_of(members)
}

/// A JSON list of strings: the C++'s `{"a", "b"}`.
pub fn strings(items: &[&str]) -> Json {
    Json::array_of(items.iter().map(|&item| Json::from(item)))
}

/// A JSON list of whole numbers: the C++'s `{1, 2}`.
pub fn integers(items: &[i64]) -> Json {
    Json::array_of(items.iter().map(|&item| Json::from(item)))
}

/// A JSON list of reals: the C++'s `{0.0, 1.5}`.
pub fn reals(items: &[f64]) -> Json {
    Json::array_of(items.iter().map(|&item| Json::from(item)))
}

/// katana_run_commands' arguments: {"commands": [...]}.
pub fn commands(lines: &[&str]) -> Json {
    object([("commands", strings(lines))])
}

/// EXPECT_DOUBLE_EQ: within 4 units in the last place (geometry's tests'
/// double_eq).
#[track_caller]
pub fn double_eq(actual: f64, expected: f64) {
    fn biased(value: f64) -> u64 {
        let bits = value.to_bits();
        if bits & (1 << 63) != 0 {
            (!bits).wrapping_add(1)
        } else {
            bits | (1 << 63)
        }
    }
    assert!(
        !actual.is_nan() && !expected.is_nan() && biased(actual).abs_diff(biased(expected)) <= 4,
        "actual={actual:?}, expected={expected:?}"
    );
}

/// The number a JSON value holds, as a double: `json.get<double>()`.
#[track_caller]
pub fn double_of(json: &Json) -> f64 {
    json.get::<f64>().expect("a number")
}

/// tests/surveyio/data, KATANA_SURVEYIO_DATA.
pub const SURVEYIO_DATA: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../../../tests/surveyio/data");
