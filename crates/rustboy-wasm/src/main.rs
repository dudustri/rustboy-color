//! Builds the browser host, then serves it.
//!
//! `cargo run -p rustboy-wasm` then open the address it prints. Pass --no-build to skip the build.

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::thread;

const DEFAULT_PORT: u16 = 8080;
const DEFAULT_ROOT: &str = "web";

/// The repository, found from where this crate sits rather than the shell's directory.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap_or(Path::new("."))
        .to_path_buf()
}

/// Turn the browser host into `web/pkg`. Returns false if the build did not happen.
fn build_wasm() -> bool {
    println!("building the browser host...");
    let status = Command::new("wasm-pack")
        .current_dir(workspace_root())
        .args([
            "build",
            "crates/rustboy-wasm",
            "--target",
            "web",
            "--out-dir",
            "../../web/pkg",
        ])
        .status();

    match status {
        Ok(status) if status.success() => true,
        Ok(_) => {
            eprintln!("wasm-pack failed, serving whatever was built before");
            false
        }
        Err(_) => {
            eprintln!("wasm-pack is not installed: cargo install wasm-pack");
            false
        }
    }
}

fn main() {
    let mut args = std::env::args().skip(1).peekable();
    let skip_build = std::env::args().any(|a| a == "--no-build");
    let mut positional = args.by_ref().filter(|a| !a.starts_with("--"));

    let root = positional
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace_root().join(DEFAULT_ROOT));
    let port: u16 = positional
        .next()
        .and_then(|p| p.parse().ok())
        .unwrap_or(DEFAULT_PORT);

    if !skip_build {
        build_wasm();
    }

    let root = match fs::canonicalize(&root) {
        Ok(root) => root,
        Err(error) => {
            eprintln!("could not find {}: {error}", root.display());
            return;
        }
    };

    let listener = match TcpListener::bind(("127.0.0.1", port)) {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("could not listen on port {port}: {error}");
            return;
        }
    };

    println!("serving {} at http://localhost:{port}", root.display());
    for stream in listener.incoming().flatten() {
        let root = root.clone();
        thread::spawn(move || serve(stream, &root));
    }
}

// Read one request and answer it. Anything malformed is simply dropped.
fn serve(mut stream: TcpStream, root: &Path) {
    let mut reader = BufReader::new(match stream.try_clone() {
        Ok(clone) => clone,
        Err(_) => return,
    });

    let mut request = String::new();
    if reader.read_line(&mut request).is_err() {
        return;
    }

    // Headers are not needed, but they must be drained before replying.
    let mut line = String::new();
    while reader.read_line(&mut line).is_ok_and(|n| n > 2) {
        line.clear();
    }

    let Some(target) = request.split_whitespace().nth(1) else {
        return;
    };
    match resolve(target, root) {
        Some(path) => send_file(&mut stream, &path),
        None => send(&mut stream, "404 Not Found", "text/plain", b"not found"),
    }
}

/// Turn a request target into a file inside `root`, or `None` if it escapes.
fn resolve(target: &str, root: &Path) -> Option<PathBuf> {
    let path = target.split(['?', '#']).next().unwrap_or("/");
    let mut full = root.to_path_buf();

    // Rebuild the path a segment at a time so `..` can never climb out.
    for part in path.split('/').filter(|p| !p.is_empty()) {
        let decoded = percent_decode(part);
        let component = Path::new(&decoded).components().next()?;
        match component {
            Component::Normal(name) => full.push(name),
            _ => return None,
        }
    }

    if full.is_dir() {
        full.push("index.html");
    }
    full.starts_with(root)
        .then_some(full)
        .filter(|f| f.is_file())
}

/// Turn `%20` and friends back into the bytes they stand for.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(&text[i + 1..i + 3], 16) {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

// Browsers refuse to stream wasm unless the type is exactly right.
fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("wasm") => "application/wasm",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json",
        Some("png") => "image/png",
        Some("jpeg" | "jpg") => "image/jpeg",
        Some("svg") => "image/svg+xml",
        _ => "application/octet-stream",
    }
}

fn send_file(stream: &mut TcpStream, path: &Path) {
    match fs::read(path) {
        Ok(body) => send(stream, "200 OK", content_type(path), &body),
        Err(_) => send(stream, "404 Not Found", "text/plain", b"not found"),
    }
}

fn send(stream: &mut TcpStream, status: &str, content_type: &str, body: &[u8]) {
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body);
    let _ = stream.flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_escapes_are_decoded() {
        assert_eq!(percent_decode("my%20game.gb"), "my game.gb");
        assert_eq!(percent_decode("plain.js"), "plain.js");
    }

    #[test]
    fn climbing_out_of_the_root_is_refused() {
        let root = fs::canonicalize(".").unwrap();
        assert_eq!(resolve("/../../etc/passwd", &root), None);
        assert_eq!(resolve("/%2e%2e/secret", &root), None);
    }

    #[test]
    fn wasm_gets_the_type_browsers_insist_on() {
        assert_eq!(content_type(Path::new("a.wasm")), "application/wasm");
        assert_eq!(
            content_type(Path::new("a.mystery")),
            "application/octet-stream"
        );
    }
}
