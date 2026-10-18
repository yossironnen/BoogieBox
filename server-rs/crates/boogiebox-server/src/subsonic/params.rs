//! Subsonic request parameters: the query string merged with an `application/x-www-form-urlencoded` body
//! (OpenSubsonic `formPost`). Repeated keys (`id=1&id=2`) are preserved in order.

use super::response::SubsonicError;

/// Longest value accepted for any single parameter.
const MAX_VALUE_LEN: usize = 4096;
/// Most parameters accepted per request (repeated ids included).
const MAX_PAIRS: usize = 2048;

/// Parsed request parameters.
#[derive(Debug, Clone, Default)]
pub struct Params {
    pairs: Vec<(String, String)>,
}

impl Params {
    /// Parses the raw query string and form body.
    pub fn parse(query: Option<&str>, body: &[u8]) -> Self {
        let mut pairs = Vec::new();
        let body = std::str::from_utf8(body).unwrap_or("");
        for source in [query.unwrap_or(""), body] {
            for part in source.split('&').filter(|p| !p.is_empty()) {
                if pairs.len() >= MAX_PAIRS {
                    break;
                }
                let (k, v) = part.split_once('=').unwrap_or((part, ""));
                let key = decode(k);
                let mut value = decode(v);
                if value.len() > MAX_VALUE_LEN {
                    let mut end = MAX_VALUE_LEN;
                    while !value.is_char_boundary(end) {
                        end -= 1;
                    }
                    value.truncate(end);
                }
                pairs.push((key, value));
            }
        }
        Params { pairs }
    }

    /// First value for `key`.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.pairs
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// First non-blank value for `key`, trimmed.
    pub fn text(&self, key: &str) -> Option<&str> {
        self.get(key).map(str::trim).filter(|v| !v.is_empty())
    }

    /// Required non-blank value (error 10 otherwise).
    pub fn required(&self, key: &str) -> Result<&str, SubsonicError> {
        self.text(key).ok_or_else(|| SubsonicError::missing(key))
    }

    /// Every non-blank value for `key`, in request order.
    pub fn all(&self, key: &str) -> Vec<&str> {
        self.pairs
            .iter()
            .filter(|(k, _)| k == key)
            .map(|(_, v)| v.trim())
            .filter(|v| !v.is_empty())
            .collect()
    }

    /// Optional integer; a malformed value is error 0 (Subsonic has no "bad value" code).
    pub fn int(&self, key: &str) -> Result<Option<i64>, SubsonicError> {
        match self.text(key) {
            None => Ok(None),
            Some(v) => v
                .parse::<i64>()
                .map(Some)
                .map_err(|_| SubsonicError::generic(format!("Invalid value for parameter: {key}"))),
        }
    }

    /// Integer clamped to `[min, max]`, `default` when absent.
    pub fn clamped(
        &self,
        key: &str,
        default: i64,
        min: i64,
        max: i64,
    ) -> Result<i64, SubsonicError> {
        Ok(self.int(key)?.unwrap_or(default).clamp(min, max))
    }

    /// Boolean (`true`/`false`, `1`/`0`), `default` when absent or unrecognized.
    pub fn flag(&self, key: &str, default: bool) -> bool {
        match self.text(key).map(str::to_ascii_lowercase).as_deref() {
            Some("true") | Some("1") => true,
            Some("false") | Some("0") => false,
            _ => default,
        }
    }
}

/// Percent-decodes `+`-as-space form encoding; invalid escapes are kept literally.
fn decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => match (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                (Some(h), Some(l)) => {
                    out.push(h * 16 + l);
                    i += 2;
                }
                _ => out.push(b'%'),
            },
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merges_query_and_form_body_with_repeated_keys() {
        let p = Params::parse(
            Some("u=alice&id=1&id=2&f=json"),
            b"id=3&title=Hello+World%21",
        );
        assert_eq!(p.get("u"), Some("alice"));
        assert_eq!(p.all("id"), ["1", "2", "3"]);
        assert_eq!(p.get("title"), Some("Hello World!"));
        assert_eq!(p.get("missing"), None);
    }

    #[test]
    fn decodes_utf8_and_keeps_bad_escapes() {
        let p = Params::parse(Some("q=caf%C3%A9&bad=100%&worse=%zz&tail=%4"), b"");
        assert_eq!(p.get("q"), Some("café"));
        assert_eq!(p.get("bad"), Some("100%"));
        assert_eq!(p.get("worse"), Some("%zz"));
        assert_eq!(p.get("tail"), Some("%4"));
    }

    #[test]
    fn typed_getters() {
        let p = Params::parse(Some("size=900&offset=-5&n=abc&blank=%20&s=true&z=0"), b"");
        assert_eq!(p.clamped("size", 10, 0, 500).unwrap(), 500);
        assert_eq!(p.clamped("offset", 0, 0, i64::MAX).unwrap(), 0);
        assert_eq!(p.clamped("absent", 10, 0, 500).unwrap(), 10);
        assert_eq!(p.int("n").unwrap_err().code, 0);
        assert_eq!(p.text("blank"), None);
        assert_eq!(p.required("blank").unwrap_err().code, 10);
        assert!(p.flag("s", false));
        assert!(!p.flag("z", true));
        assert!(p.flag("absent", true));
    }

    #[test]
    fn bounds_value_length_and_pair_count() {
        let long = "a".repeat(MAX_VALUE_LEN + 10);
        let p = Params::parse(Some(&format!("x={long}")), b"");
        assert_eq!(p.get("x").unwrap().len(), MAX_VALUE_LEN);
        let many = (0..MAX_PAIRS + 50)
            .map(|i| format!("id={i}"))
            .collect::<Vec<_>>()
            .join("&");
        assert_eq!(Params::parse(Some(&many), b"").all("id").len(), MAX_PAIRS);
    }
}
