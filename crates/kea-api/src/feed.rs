//! Atom feeds of what changed after a contract was proved. A feed is the alert channel that needs
//! nothing from the reader and sends nothing from the server: any feed reader can follow one.

use crate::store::Event;
use std::fmt::Write as _;

/// Event kinds anyone may see. Others, such as a finding held for private disclosure, stay inside.
pub const PUBLIC: &[&str] = &["code-changed"];

/// Text made safe for XML. Contract names come from strangers' verified sources.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c if c.is_control() && c != '\n' && c != '\t' => {}
            c => out.push(c),
        }
    }
    out
}

/// Seconds since the epoch as an RFC 3339 UTC time.
pub fn rfc3339(seconds: i64) -> String {
    let (days, rest) = (seconds.div_euclid(86_400), seconds.rem_euclid(86_400));
    // Days to a civil date (H. Hinnant's algorithm), proleptic Gregorian.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z", rest / 3600, rest % 3600 / 60, rest % 60)
}

/// What an event says in a feed's title and summary.
fn words(event: &Event, name: &str) -> (String, String) {
    match event.kind.as_str() {
        "code-changed" => (
            format!("{name} now runs different code"),
            format!(
                "The code at {} changed after Kea proved it. Its report no longer describes it; a new check is queued \
                 while the day's budget allows.",
                event.address
            ),
        ),
        other => (format!("{name}: {other}"), format!("{other} at {}", event.address)),
    }
}

/// An Atom feed of `events`; `name` finds each event's contract name.
pub fn atom(title: &str, self_path: &str, site: &str, events: &[Event], name: impl Fn(&str) -> String) -> String {
    let updated = events.first().map_or_else(crate::store::now, |event| event.at);
    let mut xml =
        String::from("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<feed xmlns=\"http://www.w3.org/2005/Atom\">\n");
    let _ = writeln!(xml, "  <title>{}</title>", escape(title));
    let _ = writeln!(xml, "  <id>{}{}</id>", escape(site), escape(self_path));
    let _ = writeln!(xml, "  <link rel=\"self\" href=\"{}{}\"/>", escape(site), escape(self_path));
    let _ = writeln!(xml, "  <updated>{}</updated>", rfc3339(updated));
    let _ = writeln!(xml, "  <author><name>Kea</name></author>");
    for event in events {
        let (headline, summary) = words(event, &name(&event.address));
        let _ = writeln!(xml, "  <entry>");
        let _ = writeln!(xml, "    <id>{}/events/{}</id>", escape(site), event.id);
        let _ = writeln!(xml, "    <title>{}</title>", escape(&headline));
        let _ = writeln!(xml, "    <link href=\"{}/c/{}\"/>", escape(site), escape(&event.address));
        let _ = writeln!(xml, "    <updated>{}</updated>", rfc3339(event.at));
        let _ = writeln!(xml, "    <summary>{}</summary>", escape(&summary));
        let _ = writeln!(xml, "  </entry>");
    }
    xml.push_str("</feed>\n");
    xml
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn times_are_rfc3339_utc() {
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(rfc3339(1_795_612_745), "2026-11-25T13:19:05Z");
    }

    #[test]
    fn hostile_names_cannot_break_the_feed() {
        let event = Event { id: 7, address: "0xab".into(), kind: "code-changed".into(), detail: json!({}), at: 0 };
        let xml = atom("Kea", "/feed.xml", "https://keaproofs.io", &[event], |_| "<script>&\u{7}".into());
        assert!(xml.contains("&lt;script&gt;&amp; now runs different code"));
        assert!(!xml.contains("<script>") && !xml.contains('\u{7}'));
        assert!(xml.contains("<link href=\"https://keaproofs.io/c/0xab\"/>"));
    }
}
