// perception/text.rs — deterministic text matching over OCR candidates.
use regex::Regex;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MatchMode {
    Exact,
    Contains,
    Regex,
}

impl MatchMode {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "exact" => Some(Self::Exact),
            "contains" => Some(Self::Contains),
            "regex" => Some(Self::Regex),
            _ => None,
        }
    }
}

/// Collapse runs of whitespace and trim. Unicode case folding is applied only
/// when the caller asks for a case-insensitive comparison; the original text
/// is never rewritten in an observation.
pub fn normalize(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn compare(actual: &str, query: &str, mode: MatchMode, case_sensitive: bool) -> bool {
    let (a, q) = if case_sensitive {
        (actual.to_string(), query.to_string())
    } else {
        (actual.to_lowercase(), query.to_lowercase())
    };
    match mode {
        MatchMode::Exact => normalize(&a) == normalize(&q),
        MatchMode::Contains => normalize(&a).contains(&normalize(&q)),
        MatchMode::Regex => Regex::new(query)
            .map(|re| re.is_match(actual))
            .unwrap_or(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contains_is_whitespace_stable() {
        assert!(compare("  Save   As… ", "save as", MatchMode::Contains, false));
        assert!(!compare("Save", "save", MatchMode::Exact, true));
    }

    #[test]
    fn regex_uses_the_original_text() {
        assert!(compare("File 12", r"^\w+\s+\d+$", MatchMode::Regex, true));
    }
}
