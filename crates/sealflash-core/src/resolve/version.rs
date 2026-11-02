//! Versionsnummern vergleichen.
//!
//! Ein einfacher Textvergleich reicht nicht: alphabetisch ist `"9" > "10"` und `"24.4" > "24.10"`.
//! Wir zerlegen deshalb in Zahlen- und Textblöcke und vergleichen Zahlen als Zahlen:
//!
//! ```text
//! "26.04.1"  → [26] [.] [4] [.] [1]
//! "26.04"    → [26] [.] [4]          → kürzer, also kleiner
//! ```

use std::cmp::Ordering;

#[derive(Debug, PartialEq, Eq)]
enum Part<'a> {
    Num(&'a str),
    Text(&'a str),
}

fn parts(s: &str) -> Vec<Part<'_>> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut in_digits: Option<bool> = None;
    for (i, c) in s.char_indices() {
        let d = c.is_ascii_digit();
        if in_digits.is_some_and(|prev| prev != d) {
            out.push(if in_digits == Some(true) {
                Part::Num(&s[start..i])
            } else {
                Part::Text(&s[start..i])
            });
            start = i;
        }
        in_digits = Some(d);
    }
    if start < s.len() {
        out.push(if in_digits == Some(true) {
            Part::Num(&s[start..])
        } else {
            Part::Text(&s[start..])
        });
    }
    out
}

/// Zahlen beliebiger Länge ohne Überlauf vergleichen: führende Nullen weg, dann Länge, dann Ziffern.
fn cmp_num(a: &str, b: &str) -> Ordering {
    let a = a.trim_start_matches('0');
    let b = b.trim_start_matches('0');
    a.len().cmp(&b.len()).then_with(|| a.cmp(b))
}

/// Vergleicht zwei Versionsnummern „natürlich“.
pub fn compare(a: &str, b: &str) -> Ordering {
    let (pa, pb) = (parts(a), parts(b));
    for (x, y) in pa.iter().zip(&pb) {
        let ord = match (x, y) {
            (Part::Num(x), Part::Num(y)) => cmp_num(x, y),
            (Part::Text(x), Part::Text(y)) => x.cmp(y),
            // Zahl gegen Text (z. B. "1.0" vs "1.rc"): die Zahl gilt als neuer.
            (Part::Num(_), Part::Text(_)) => Ordering::Greater,
            (Part::Text(_), Part::Num(_)) => Ordering::Less,
        };
        if ord != Ordering::Equal {
            return ord;
        }
    }
    pa.len().cmp(&pb.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_order() {
        let mut v = vec![
            "24.10", "9", "24.04", "26.04.1", "24.4", "26.04", "10", "22.04.5",
        ];
        v.sort_by(|a, b| compare(a, b));
        assert_eq!(
            v,
            [
                "9", "10", "22.04.5", "24.04", "24.4", "24.10", "26.04", "26.04.1"
            ]
        );
    }

    #[test]
    fn dates_and_dashes() {
        assert_eq!(compare("2026.11.01", "2026.10.01"), Ordering::Greater);
        assert_eq!(compare("9.2-1", "9.10-1"), Ordering::Less);
        assert_eq!(compare("43.1", "43"), Ordering::Greater);
        assert_eq!(compare("20261030", "20261101"), Ordering::Less);
    }

    #[test]
    fn huge_numbers_do_not_overflow() {
        assert_eq!(
            compare("99999999999999999999999", "100000000000000000000000"),
            Ordering::Less
        );
    }
}
