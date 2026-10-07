//! Instance names to file names and back.
//!
//! Vortex allows almost any name, file systems don't. Unsafe characters are
//! percent encoded, so `a/b` becomes `a%2Fb` and decodes back exactly. Names
//! that only differ by case, or that collide after encoding, get a `~2`,
//! `~3`... suffix and keep their real name in the instance file.

use std::collections::HashSet;

const RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

fn needs_escape(c: char) -> bool {
    matches!(
        c,
        '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '%' | '~'
    ) || c.is_control()
}

fn push_escaped(out: &mut String, c: char) {
    let mut buf = [0u8; 4];
    for b in c.encode_utf8(&mut buf).bytes() {
        out.push_str(&format!("%{b:02X}"));
    }
}

/// Encodes a name into a file name stem. Decoding it with [`decode`] gives the
/// name back unchanged.
pub fn encode(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let last = name.chars().count().saturating_sub(1);
    for (i, c) in name.chars().enumerate() {
        // Windows drops trailing dots and spaces, and a leading dot hides files elsewhere
        // and a leading underscore would look like a folder's own `_part.json` file
        let edge =
            (i == 0 && matches!(c, '.' | ' ' | '_')) || (i == last && matches!(c, '.' | ' '));
        if needs_escape(c) || edge {
            push_escaped(&mut out, c);
        } else {
            out.push(c);
        }
    }
    if out.is_empty() {
        return "%00".into();
    }
    // CON, NUL and friends can't be file names on Windows, even with an extension
    let upper = out.to_ascii_uppercase();
    if RESERVED.contains(&upper.as_str()) {
        let mut chars = out.chars();
        let first = chars.next().unwrap();
        let mut escaped = String::new();
        push_escaped(&mut escaped, first);
        out = escaped + chars.as_str();
    }
    out
}

/// Reverses [`encode`]. Returns None for broken escapes, which only happen in
/// hand made file names.
pub fn decode(stem: &str) -> Option<String> {
    if stem == "%00" {
        return Some(String::new());
    }
    let bytes = stem.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = stem.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// Hands out unique file stems among siblings. Comparison ignores case,
/// since Windows and macOS treat `Part` and `part` as the same file.
#[derive(Default)]
pub struct Siblings {
    taken: HashSet<String>,
}

impl Siblings {
    pub fn claim(&mut self, name: &str) -> String {
        let base = encode(name);
        let mut stem = base.clone();
        let mut n = 2;
        while !self.taken.insert(stem.to_lowercase()) {
            stem = format!("{base}~{n}");
            n += 1;
        }
        stem
    }
}

/// The name a file stem stands for, ignoring a `~N` collision suffix.
pub fn name_from_stem(stem: &str) -> Option<String> {
    let base = match stem.rsplit_once('~') {
        Some((base, n)) if !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) => base,
        _ => stem,
    };
    decode(base)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrips() {
        for name in [
            "Part",
            "a/b",
            "50%",
            "x~2",
            "C:\\stuff",
            "trailing.",
            " lead",
            "tail ",
            ".hidden",
            "",
            "CON",
            "nul",
            "ünïcode ✓",
            "tab\there",
            "Model~",
            "_part",
            "init",
        ] {
            let enc = encode(name);
            assert_eq!(decode(&enc).as_deref(), Some(name), "{name:?} -> {enc:?}");
            assert_eq!(
                name_from_stem(&enc).as_deref(),
                Some(name),
                "{name:?} -> {enc:?}"
            );
            assert!(
                !enc.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|']),
                "{enc}"
            );
        }
    }

    #[test]
    fn plain_names_stay_readable() {
        assert_eq!(encode("Lava Floor 2"), "Lava Floor 2");
        assert_eq!(encode("Main"), "Main");
    }

    #[test]
    fn reserved_and_edges() {
        assert_eq!(encode("CON"), "%43ON");
        assert_eq!(encode("x."), "x%2E");
        assert_eq!(encode("a~b"), "a%7Eb");
    }

    #[test]
    fn siblings_get_suffixes() {
        let mut s = Siblings::default();
        assert_eq!(s.claim("Part"), "Part");
        assert_eq!(s.claim("Part"), "Part~2");
        // "part~2" would clash with "Part~2" on a case insensitive disk
        assert_eq!(s.claim("part"), "part~3");
        assert_eq!(s.claim("Part"), "Part~4");
        assert_eq!(name_from_stem("Part~3").as_deref(), Some("Part"));
    }

    #[test]
    fn broken_escapes() {
        assert_eq!(decode("%zz"), None);
        assert_eq!(decode("%4"), None);
    }
}
