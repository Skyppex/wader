//! `file://` URIs to and from paths.

use std::path::{Path, PathBuf};

use crate::lsp::Uri;

/// The path a `file://` URI names. `None` for any other scheme.
pub fn to_path(uri: &Uri) -> Option<PathBuf> {
    let rest = uri.0.strip_prefix("file://")?;
    // `file:///home/x` and `file://localhost/home/x` both name `/home/x`.
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    let mut bytes = Vec::with_capacity(rest.len());
    let raw = rest.as_bytes();
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == b'%'
            && let Some(b) = raw
                .get(i + 1..i + 3)
                .and_then(|h| std::str::from_utf8(h).ok())
                .and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            bytes.push(b);
            i += 3;
            continue;
        }
        bytes.push(raw[i]);
        i += 1;
    }
    let path = String::from_utf8(bytes).ok()?;
    // `file:///C:/x` on Windows.
    let path = match path.as_bytes() {
        [b'/', _, b':', ..] => path[1..].to_owned(),
        _ => path,
    };
    Some(PathBuf::from(path))
}

/// The `file://` URI of `path`.
pub fn from_path(path: &Path) -> Uri {
    let path = path.to_string_lossy().replace('\\', "/");
    let mut out = String::from("file://");
    if !path.starts_with('/') {
        out.push('/');
    }
    for b in path.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'-' | b'_' | b'.' | b'~' | b':' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    Uri(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let uri = Uri::from("file:///home/me/my%20songs/song.rill");
        let path = to_path(&uri).unwrap();
        assert_eq!(path, PathBuf::from("/home/me/my songs/song.rill"));
        assert_eq!(from_path(&path), uri);
        assert_eq!(to_path(&Uri::from("untitled:Untitled-1")), None);
    }
}
