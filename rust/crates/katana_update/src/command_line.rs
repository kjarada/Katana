//! A program's command line as one string, by the rules Windows parses it with
//! (CommandLineToArgvW, which every main() of the update system reads through utf8_arguments).
//! Internal to the update library (the C++'s private header command_line.hpp).
//!
//! Two users, hence a module and not two copies. The Windows launcher (spawn_win.rs) hands the
//! string to CreateProcessW; the apply writes the same string to the RunOnce value that re-runs
//! `katana_updater --recover <R>` at the next logon, and that string meets the same parser.
//! Arguments are UTF-8; the quoting only treats the ASCII characters that matter (space, tab,
//! quote, backslash), which no multi-byte UTF-8 sequence contains.
//!
//! The rule (Microsoft, "Parsing C++ command-line arguments"): an argument with no space, tab,
//! line break or quote and not empty is written as it is; otherwise it is wrapped in quotes, a
//! quote inside gets a backslash, and a run of backslashes that ends at a quote - an inner one or
//! the closing one - is doubled.

pub fn quote_argument(argument: &str) -> String {
    if !argument.is_empty() && !argument.contains([' ', '\t', '\n', '\x0b', '"']) {
        return argument.to_owned();
    }
    let bytes = argument.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len() + 2);
    out.push(b'"');
    let mut i = 0;
    loop {
        let mut backslashes = 0;
        while i < bytes.len() && bytes[i] == b'\\' {
            i += 1;
            backslashes += 1;
        }
        if i == bytes.len() {
            out.resize(out.len() + backslashes * 2, b'\\'); // before the closing quote
            break;
        }
        if bytes[i] == b'"' {
            // before an inner quote, and the quote itself
            out.resize(out.len() + backslashes * 2 + 1, b'\\');
            out.push(b'"');
        } else {
            out.resize(out.len() + backslashes, b'\\');
            out.push(bytes[i]);
        }
        i += 1;
    }
    out.push(b'"');
    // Only ASCII bytes were added, around and between whole characters of `argument`.
    String::from_utf8(out).expect("quoting adds ASCII only")
}

/// `"<program>" arg arg ...`. The program is always quoted: it is a path, and a path may hold a
/// space. (CommandLineToArgvW reads the first token without backslash escapes, which is why it is
/// wrapped in plain quotes and not run through quote_argument; a Windows path holds no quote.)
pub fn command_line(program: &str, arguments: &[String]) -> String {
    let mut line = format!("\"{program}\"");
    for argument in arguments {
        line.push(' ');
        line.push_str(&quote_argument(argument));
    }
    line
}
