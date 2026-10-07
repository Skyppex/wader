//! Message framing: a `Content-Length` header, a blank line, then that many
//! bytes of JSON.

use std::io::{self, BufRead, Write};

use super::message::Message;

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

/// Read the next message. `Ok(None)` at a clean end of input.
///
/// A body that is not a valid message is an [`io::ErrorKind::InvalidData`]
/// error; the stream stays in sync, so the caller can keep reading.
pub fn read_message(r: &mut impl BufRead) -> io::Result<Option<Message>> {
    let mut length = None;
    let mut line = String::new();
    let mut first = true;
    loop {
        line.clear();
        if r.read_line(&mut line)? == 0 {
            return if first {
                Ok(None)
            } else {
                Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "end of input inside message headers",
                ))
            };
        }
        first = false;
        let header = line.trim_end_matches(['\r', '\n']);
        if header.is_empty() {
            break;
        }
        let Some((name, value)) = header.split_once(':') else {
            return Err(invalid(format!("malformed header {header:?}")));
        };
        if name.trim().eq_ignore_ascii_case("content-length") {
            let n = value
                .trim()
                .parse::<usize>()
                .map_err(|_| invalid(format!("bad Content-Length {:?}", value.trim())))?;
            length = Some(n);
        }
        // Content-Type is the only other header and it is always UTF-8 JSON.
    }
    let length = length.ok_or_else(|| invalid("missing Content-Length header"))?;
    let mut body = vec![0; length];
    r.read_exact(&mut body)?;
    serde_json::from_slice(&body)
        .map(Some)
        .map_err(|e| invalid(format!("bad message: {e}")))
}

pub fn write_message(w: &mut impl Write, message: &Message) -> io::Result<()> {
    let body = serde_json::to_vec(message).map_err(io::Error::other)?;
    write!(w, "Content-Length: {}\r\n\r\n", body.len())?;
    w.write_all(&body)?;
    w.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rpc::message::{Notification, Request};
    use serde_json::json;

    fn frame(body: &str) -> String {
        format!("Content-Length: {}\r\n\r\n{body}", body.len())
    }

    #[test]
    fn reads_back_what_it_writes() {
        let a: Message = Request {
            id: 1.into(),
            method: "hover".into(),
            params: Some(json!({"text": "ø 🎵"})),
        }
        .into();
        let b: Message = Notification {
            method: "exit".into(),
            params: None,
        }
        .into();
        let mut buf = Vec::new();
        write_message(&mut buf, &a).unwrap();
        write_message(&mut buf, &b).unwrap();

        let mut r = &buf[..];
        assert_eq!(read_message(&mut r).unwrap(), Some(a));
        assert_eq!(read_message(&mut r).unwrap(), Some(b));
        assert_eq!(read_message(&mut r).unwrap(), None);
    }

    #[test]
    fn length_counts_bytes() {
        // `ø` is two bytes; a char count would cut the body short.
        let text = frame(r#"{"jsonrpc":"2.0","method":"ø"}"#);
        let m = read_message(&mut text.as_bytes()).unwrap().unwrap();
        assert!(matches!(m, Message::Notification(n) if n.method == "ø"));
    }

    #[test]
    fn extra_headers_and_bare_newlines() {
        let body = r#"{"jsonrpc":"2.0","method":"x"}"#;
        let text = format!(
            "content-length: {}\nContent-Type: application/vscode-jsonrpc; charset=utf-8\n\n{body}",
            body.len()
        );
        assert!(read_message(&mut text.as_bytes()).unwrap().is_some());
    }

    #[test]
    fn errors() {
        let e = read_message(&mut "Foo: 1\r\n\r\n{}".as_bytes()).unwrap_err();
        assert!(e.to_string().contains("missing Content-Length"));

        let e = read_message(&mut "Content-Length: 10\r\n".as_bytes()).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::UnexpectedEof);

        // A bad body is reported, and the next message still reads.
        let text = frame("{nope}") + &frame(r#"{"jsonrpc":"2.0","method":"x"}"#);
        let mut r = text.as_bytes();
        assert_eq!(
            read_message(&mut r).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert!(read_message(&mut r).unwrap().is_some());
    }
}
