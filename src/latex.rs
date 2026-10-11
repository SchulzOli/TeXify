//! LaTeX escaping and locale-aware value formatting (dates, numbers, money).

use std::str::FromStr;

use chrono::{Datelike, Local, NaiveDate};
use rust_decimal::{Decimal, RoundingStrategy};

use crate::{bail, error::Result};

/// Escape plain text for LaTeX.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 8);
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str(r"\textbackslash{}"),
            '&' => out.push_str(r"\&"),
            '%' => out.push_str(r"\%"),
            '$' => out.push_str(r"\$"),
            '#' => out.push_str(r"\#"),
            '_' => out.push_str(r"\_"),
            '{' => out.push_str(r"\{"),
            '}' => out.push_str(r"\}"),
            '~' => out.push_str(r"\textasciitilde{}"),
            '^' => out.push_str(r"\textasciicircum{}"),
            // babel-ngerman makes " an active shorthand character
            '"' => out.push_str(r"\textquotedbl{}"),
            // characters pdflatex + T1 cannot typeset directly
            '\u{a0}' => out.push('~'),
            '\u{2009}' | '\u{202f}' => out.push_str(r"\,"),
            '\u{2212}' => out.push_str(r"\textminus{}"),
            '\u{2192}' => out.push_str(r"\ensuremath{\rightarrow}"),
            '\u{2190}' => out.push_str(r"\ensuremath{\leftarrow}"),
            '\u{2264}' => out.push_str(r"\ensuremath{\leq}"),
            '\u{2265}' => out.push_str(r"\ensuremath{\geq}"),
            '\u{d7}' => out.push_str(r"\ensuremath{\times}"),
            '\u{2713}' => out.push_str(r"\ensuremath{\checkmark}"),
            c => out.push(c),
        }
    }
    out
}

/// Escape a URL for use inside `\href{...}{}`.
pub fn escape_url(url: &str) -> String {
    url.replace('\\', "/").replace('%', r"\%").replace('#', r"\#")
}

// --------------------------------------------------------------------------
// Language
// --------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    De,
}

impl Lang {
    pub fn parse(value: Option<&str>) -> Result<Lang> {
        let v = value.unwrap_or("en").trim().to_lowercase();
        if v.starts_with("en") || v == "english" {
            Ok(Lang::En)
        } else if v.starts_with("de") || v == "german" || v == "ngerman" {
            Ok(Lang::De)
        } else {
            bail!("Language {v:?} is not supported (allowed: en, de)")
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::De => "de",
        }
    }
}

const MONTHS_EN: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
const MONTHS_DE: [&str; 12] = [
    "Januar",
    "Februar",
    "März",
    "April",
    "Mai",
    "Juni",
    "Juli",
    "August",
    "September",
    "Oktober",
    "November",
    "Dezember",
];

/// Parse ISO dates, German dd.mm.yyyy, or "today"/"heute".
pub fn to_date(value: &str) -> Option<NaiveDate> {
    let s = value.trim();
    if s.eq_ignore_ascii_case("today") || s.eq_ignore_ascii_case("heute") {
        return Some(Local::now().date_naive());
    }
    for fmt in ["%Y-%m-%d", "%d.%m.%Y", "%d.%m.%y"] {
        if let Ok(d) = NaiveDate::parse_from_str(s, fmt) {
            return Some(d);
        }
    }
    None
}

/// Format a date; unparseable values are returned unchanged ("2020 – today").
pub fn format_date(value: &str, lang: Lang, short: bool) -> String {
    let Some(d) = to_date(value) else {
        return value.to_string();
    };
    match (lang, short) {
        (Lang::De, true) => d.format("%d.%m.%Y").to_string(),
        (Lang::En, true) => d.format("%Y-%m-%d").to_string(),
        (Lang::De, false) => format!("{}. {} {}", d.day(), MONTHS_DE[d.month0() as usize], d.year()),
        (Lang::En, false) => format!("{} {}, {}", MONTHS_EN[d.month0() as usize], d.day(), d.year()),
    }
}

/// Parse numbers given as `1234.5`, `1,234.5` (en) or `1.234,5` (de).
pub fn to_decimal(value: &str, what: &str) -> Result<Decimal> {
    let mut s: String = value.trim().chars().filter(|c| !matches!(c, '€' | ' ' | '\u{a0}')).collect();
    if s.contains(',') && s.contains('.') {
        // the later separator is the decimal one
        if s.rfind(',') > s.rfind('.') {
            s = s.replace('.', "").replace(',', ".");
        } else {
            s = s.replace(',', "");
        }
    } else if s.contains(',') {
        s = s.replace(',', ".");
    }
    match Decimal::from_str(&s) {
        Ok(d) => Ok(d),
        Err(_) => bail!("Invalid {what}: {value:?}"),
    }
}

fn group(int_part: &str, sep: &str) -> String {
    let digits: Vec<char> = int_part.chars().collect();
    let mut out = String::new();
    for (i, c) in digits.iter().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push_str(sep);
        }
        out.push(*c);
    }
    out
}

/// Language-aware number: `1,234.5` / `1.234,5`; `decimals` fixes the precision.
pub fn format_number(d: Decimal, lang: Lang, decimals: Option<u32>) -> String {
    let d = match decimals {
        Some(n) => d.round_dp_with_strategy(n, RoundingStrategy::MidpointAwayFromZero),
        None => d.normalize(),
    };
    let mut s = d.abs().to_string();
    if let Some(n) = decimals {
        // to_string drops trailing zeros of rounded values in some cases
        let frac_len = s.split('.').nth(1).map_or(0, str::len) as u32;
        if n > 0 && frac_len < n {
            if frac_len == 0 {
                s.push('.');
            }
            s.push_str(&"0".repeat((n - frac_len) as usize));
        }
    }
    let (int_part, frac) = s.split_once('.').unwrap_or((&s, ""));
    let (thousands, dec) = match lang {
        Lang::De => (".", ","),
        Lang::En => (",", "."),
    };
    let mut out = group(int_part, thousands);
    if !frac.is_empty() {
        out.push_str(dec);
        out.push_str(frac);
    }
    if d.is_sign_negative() && !d.is_zero() {
        out.insert(0, '-');
    }
    out
}

pub fn currency_symbol(code: &str) -> String {
    match code.to_uppercase().as_str() {
        "EUR" => "€".into(),
        "USD" => "$".into(),
        "GBP" => "£".into(),
        other => other.to_string(),
    }
}

/// `€1,234.50` (en) or `1.234,50 €` (de).
pub fn format_money(d: Decimal, lang: Lang, currency: &str) -> String {
    let num = format_number(d, lang, Some(2));
    let sym = currency_symbol(currency);
    if lang == Lang::De || sym.chars().count() > 1 { format!("{num}\u{a0}{sym}") } else { format!("{sym}{num}") }
}

/// Validate a 6-digit hex colour (with or without `#`).
pub fn color_hex(value: &str) -> Result<String> {
    let v = value.trim().trim_start_matches('#');
    if v.len() == 6 && v.chars().all(|c| c.is_ascii_hexdigit()) {
        Ok(v.to_uppercase())
    } else {
        bail!("Colour must be a hex value (e.g. 1F4E79), not {value:?}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_specials() {
        assert_eq!(escape("50% & $5_#{x}~^"), r"50\% \& \$5\_\#\{x\}\textasciitilde{}\textasciicircum{}");
    }

    #[test]
    fn numbers_and_money() {
        let d = to_decimal("1234.5", "n").unwrap();
        assert_eq!(format_number(d, Lang::De, None), "1.234,5");
        assert_eq!(format_number(d, Lang::En, Some(2)), "1,234.50");
        assert_eq!(format_money(to_decimal("4551.27", "n").unwrap(), Lang::De, "EUR"), "4.551,27\u{a0}€");
        assert_eq!(format_money(to_decimal("1850", "n").unwrap(), Lang::En, "EUR"), "€1,850.00");
        assert_eq!(to_decimal("0,10", "n").unwrap(), Decimal::new(10, 2));
        assert_eq!(to_decimal("1.234,50", "n").unwrap(), Decimal::new(123450, 2));
    }

    #[test]
    fn dates() {
        assert_eq!(format_date("2026-10-11", Lang::En, false), "October 11, 2026");
        assert_eq!(format_date("2026-10-11", Lang::De, false), "11. Oktober 2026");
        assert_eq!(format_date("2026-10-11", Lang::De, true), "11.10.2026");
        assert_eq!(format_date("2020 – today", Lang::En, false), "2020 – today");
    }
}
