use fuzzy_matcher::{skim::SkimMatcherV2, FuzzyMatcher};
use nucleo_matcher::{
    pattern::{CaseMatching, Normalization, Pattern},
    Config as NucleoConfig, Matcher as NucleoMatcher, Utf32Str,
};

/// Avoid rebuilding matcher state for every candidate.
pub(crate) enum SearchMatcher {
    Skim {
        matcher: Box<SkimMatcherV2>,
        query: String,
    },
    Simple {
        query: String,
    },
    Nucleo {
        matcher: NucleoMatcher,
        pattern: Pattern,
        utf32_buf: Vec<char>,
    },
}

impl SearchMatcher {
    pub(crate) fn new(engine: &str, query: &str) -> Self {
        match engine {
            "skim" => Self::Skim {
                matcher: Box::new(SkimMatcherV2::default()),
                query: query.into(),
            },
            "simple" => Self::Simple {
                query: query.into(),
            },
            _ => Self::Nucleo {
                matcher: NucleoMatcher::new(NucleoConfig::DEFAULT.match_paths()),
                pattern: Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart),
                utf32_buf: Vec::new(),
            },
        }
    }

    pub(crate) fn score(&mut self, haystack: &str) -> Option<i64> {
        match self {
            Self::Skim { matcher, query } => matcher.fuzzy_match(haystack, query),
            Self::Simple { query } => simple_fuzzy_score(haystack, query).map(|score| -score),
            Self::Nucleo {
                matcher,
                pattern,
                utf32_buf,
            } => pattern
                .score(Utf32Str::new(haystack, utf32_buf), matcher)
                .map(|score| score as i64),
        }
    }
}

#[cfg(test)]
pub(crate) fn match_score(engine: &str, haystack: &str, query: &str) -> Option<i64> {
    SearchMatcher::new(engine, query).score(haystack)
}

fn simple_fuzzy_score(hay: &str, q: &str) -> Option<i64> {
    let mut score = 0;
    let mut pos = 0;
    for qc in q.chars() {
        let rest = &hay[pos..];
        let found = rest.find(qc)?;
        score += found as i64;
        pos += found + qc.len_utf8();
    }
    Some(score)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_matchers_find_ordered_text() {
        for engine in ["nucleo", "skim", "simple"] {
            assert!(match_score(engine, "herdr navigator", "hn").is_some());
            assert!(match_score(engine, "herdr navigator", "zx").is_none());
        }
    }

    #[test]
    fn matcher_can_be_reused_across_candidates() {
        let mut matcher = SearchMatcher::new("nucleo", "nav");
        assert!(matcher.score("herdr navigator").is_some());
        assert!(matcher.score("workspace picker").is_none());
    }
}
