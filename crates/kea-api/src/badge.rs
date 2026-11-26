//! The embeddable "proven by Kea" badge: an SVG a project can put in its README or site.

use std::fmt::Write as _;

const OLIVE: &str = "#5a6524";
const IVORY: &str = "#f6f2e9";
const INK: &str = "#262a12";
/// The deeper glaze FOUND text sits on everywhere: ivory on it clears AA at badge size.
const DEEP_GLAZE: &str = "#b8401b";

/// What the badge says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Badge {
    /// Proven and found counts from a report.
    Checked {
        /// Meaningful claims proven.
        proven: usize,
        /// Powers or breaks replayed on the live chain.
        found: usize,
    },
    /// No report yet.
    NotChecked,
    /// The code changed since its report; a new check is on its way.
    Changed,
}

impl Badge {
    fn text(self) -> String {
        match self {
            Self::Checked { proven, found: 0 } => format!("{proven} proven"),
            Self::Checked { proven, found } => format!("{proven} proven · {found} found"),
            Self::NotChecked => "not checked".into(),
            Self::Changed => "code changed".into(),
        }
    }

    /// The badge as SVG. Every character in it comes from this file or a number.
    pub fn svg(self) -> String {
        let label = "proven by Kea";
        let value = self.text();
        // An SVG shown as an image cannot load web fonts, so the badge uses a face every system has.
        // Verdana at 11px is close to 6.7px a character; the badge does not need more precision.
        let width = |text: &str| u32::try_from(text.chars().count()).unwrap_or(40) * 67 / 10 + 16;
        let (left, right) = (width(label), width(&value));
        let total = left + right;
        let fill = match self {
            Self::Checked { found, .. } if found > 0 => DEEP_GLAZE,
            Self::Checked { .. } => OLIVE,
            Self::NotChecked | Self::Changed => INK,
        };
        let mut svg = String::new();
        let _ = write!(
            svg,
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="{total}" height="22" viewBox="0 0 {total} 22" role="img" aria-label="{label}: {value}"><title>{label}: {value}</title><rect width="{total}" height="22" rx="4" fill="{IVORY}"/><rect x="{left}" width="{right}" height="22" rx="4" fill="{fill}"/><rect x="{left}" width="4" height="22" fill="{fill}"/><g font-family="Verdana, 'DejaVu Sans', sans-serif" font-size="11"><text x="8" y="15" fill="{INK}">{label}</text><text x="{x}" y="15" fill="{IVORY}">{value}</text></g></svg>"#,
            x = left + 8,
        );
        svg
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_badge_says_what_the_report_says() {
        let clean = Badge::Checked { proven: 5, found: 0 }.svg();
        assert!(clean.contains(">5 proven<") && clean.contains(OLIVE));
        let found = Badge::Checked { proven: 4, found: 2 }.svg();
        assert!(found.contains(">4 proven · 2 found<") && found.contains(DEEP_GLAZE));
        assert!(Badge::NotChecked.svg().contains(">not checked<"));
        assert!(Badge::Changed.svg().contains(">code changed<"));
        assert!(clean.starts_with("<svg") && clean.ends_with("</svg>"));
    }
}
