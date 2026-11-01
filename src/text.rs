//! Post text: cleaning what people type, and turning it into safe HTML.

pub const MAX_CHARS: usize = 2000;

/// Trims, keeps ordinary newlines and tabs, and rejects an empty or oversized post.
pub fn clean_body(raw: &str) -> Result<String, String> {
    let body = raw.replace("\r\n", "\n").replace('\r', "\n");
    let body = body.trim();
    if body.is_empty() {
        return Err("Write something first.".into());
    }
    if body.chars().count() > MAX_CHARS {
        return Err(format!("Posts can be at most {MAX_CHARS} characters."));
    }
    if body
        .chars()
        .any(|c| c.is_control() && c != '\n' && c != '\t')
    {
        return Err("Posts cannot contain control characters.".into());
    }
    Ok(body.to_string())
}

/// Escapes `text` for HTML and makes its `http(s)://` links clickable. Nothing
/// else in a post becomes markup.
pub fn linkify(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 16);
    let mut rest = text;
    while let Some(start) = find_link(rest) {
        escape_into(&mut out, &rest[..start]);
        let tail = &rest[start..];
        let url = trim_link(&tail[..tail.find(char::is_whitespace).unwrap_or(tail.len())]);
        if url.len() <= "https://".len() {
            // A bare scheme is just text.
            escape_into(&mut out, &tail[..url.len()]);
        } else {
            out.push_str("<a href=\"");
            escape_into(&mut out, url);
            out.push_str("\" rel=\"nofollow noopener noreferrer\" target=\"_blank\">");
            escape_into(&mut out, url);
            out.push_str("</a>");
        }
        rest = &tail[url.len()..];
    }
    escape_into(&mut out, rest);
    out
}

/// Where the next link starts: `http://` or `https://` at the start of a word.
fn find_link(text: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(offset) = text[from..].find("http") {
        let start = from + offset;
        let tail = &text[start..];
        let boundary = text[..start]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_alphanumeric());
        if boundary && (tail.starts_with("http://") || tail.starts_with("https://")) {
            return Some(start);
        }
        from = start + "http".len();
    }
    None
}

/// Leaves sentence punctuation after a link out of it, and a closing bracket
/// unless the link opened one.
fn trim_link(mut url: &str) -> &str {
    while let Some(last) = url.chars().next_back() {
        let unbalanced =
            |open, close| last == close && url.matches(open).count() < url.matches(close).count();
        if matches!(last, '.' | ',' | '!' | '?' | ';' | ':' | '\'' | '"')
            || unbalanced('(', ')')
            || unbalanced('[', ']')
        {
            url = &url[..url.len() - last.len_utf8()];
        } else {
            break;
        }
    }
    url
}

fn escape_into(out: &mut String, text: &str) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
}

/// A UUID as `loki_sdk::random_id` makes them.
pub fn is_id(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 36
        && bytes.iter().enumerate().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => *c == b'-',
            _ => c.is_ascii_hexdigit(),
        })
}

pub fn unix_now() -> i64 {
    loki_sdk::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

/// A UTC label and an ISO-8601 timestamp. The browser shows the reader's own
/// time zone from the timestamp; the label is for readers without JavaScript.
pub fn format_time(unix: i64) -> (String, String) {
    let secs = unix.max(0) as u64;
    let (year, month, day) = civil_ymd((secs / 86_400) as i64);
    let clock = secs % 86_400;
    let (hour, minute, second) = (clock / 3_600, (clock % 3_600) / 60, clock % 60);
    (
        format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02} UTC"),
        format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z"),
    )
}

/// Days since 1970-01-01 to a proleptic Gregorian date (Howard Hinnant's algorithm).
fn civil_ymd(days_since_epoch: i64) -> (i64, u64, u64) {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { y + 1 } else { y };
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn epoch_formats_as_utc() {
        assert_eq!(
            format_time(0),
            ("1970-01-01 00:00 UTC".into(), "1970-01-01T00:00:00Z".into())
        );
        assert_eq!(
            format_time(1_700_000_000),
            ("2023-11-14 22:13 UTC".into(), "2023-11-14T22:13:20Z".into())
        );
    }
    #[test]
    fn bodies_are_trimmed_and_bounded() {
        assert_eq!(clean_body("  hi\r\nthere  ").unwrap(), "hi\nthere");
        for bad in ["", "   \n  ", "hello\u{0001}", &"a".repeat(MAX_CHARS + 1)] {
            assert!(clean_body(bad).is_err(), "{bad:?}");
        }
        assert!(clean_body(&"枝".repeat(MAX_CHARS)).is_ok());
    }
    #[test]
    fn links_are_clickable_and_everything_else_is_escaped() {
        assert_eq!(
            linkify("see https://example.com/a?b=1&c=2."),
            "see <a href=\"https://example.com/a?b=1&amp;c=2\" rel=\"nofollow noopener noreferrer\" target=\"_blank\">https://example.com/a?b=1&amp;c=2</a>."
        );
        assert_eq!(
            linkify("<b>x</b> 'q' \"r\""),
            "&lt;b&gt;x&lt;/b&gt; &#39;q&#39; &quot;r&quot;"
        );
        // Only http(s), only at a word start, and no markup sneaks into the link.
        assert!(!linkify("javascript:alert(1)").contains("<a"));
        assert!(!linkify("xhttps://example.com").contains("<a"));
        assert!(!linkify("https://").contains("<a"));
        assert!(
            linkify("https://e.com/\"onmouseover=\"x")
                .contains("href=\"https://e.com/&quot;onmouseover=&quot;x\"")
        );
        assert!(
            linkify("(see https://en.wikipedia.org/wiki/A_(b))")
                .contains(">https://en.wikipedia.org/wiki/A_(b)</a>)")
        );
        assert!(linkify("(https://e.com/x)").contains(">https://e.com/x</a>)"));
        assert_eq!(
            linkify("line\nhttp://a.b"),
            "line\n<a href=\"http://a.b\" rel=\"nofollow noopener noreferrer\" target=\"_blank\">http://a.b</a>"
        );
    }
    #[test]
    fn ids_are_uuids() {
        assert!(is_id("11111111-1111-4111-8111-111111111111"));
        for bad in [
            "",
            "ada",
            "11111111-1111-4111-8111-11111111111g",
            "111111111111-4111-8111-111111111111-",
        ] {
            assert!(!is_id(bad), "{bad}");
        }
    }
}
