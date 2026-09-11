use fuzzy_matcher::{skim::SkimMatcherV2, FuzzyMatcher};
use nucleo_matcher::{
    pattern::{CaseMatching, Normalization, Pattern},
    Config as NucleoConfig, Matcher as NucleoMatcher, Utf32Str,
};

use crate::model::Entry;

/// Parsed copy only. The caller's raw query remains the editing and view state.
#[derive(Default)]
pub(crate) struct Query {
    pub(crate) plain: String,
    agent: Vec<String>,
    workspace_or_status: Vec<String>,
    path: Vec<String>,
    status: Vec<String>,
    pub(crate) all_agents: bool,
}
impl Query {
    pub(crate) fn parse(input: &str) -> Self {
        let mut query = Self::default();
        let mut plain = Vec::new();
        for raw in input.split_whitespace() {
            let token = raw.to_lowercase();
            let tokens = if let Some(rest) = token.strip_prefix('!') {
                Some((&mut query.agent, rest))
            } else if let Some(rest) = token.strip_prefix('@') {
                query.all_agents |= rest.is_empty();
                Some((&mut query.workspace_or_status, rest))
            } else if let Some(rest) = token.strip_prefix('/') {
                Some((&mut query.path, rest))
            } else if let Some(rest) = token.strip_prefix('#') {
                Some((&mut query.status, rest))
            } else {
                plain.push(token);
                None
            };
            if let Some((tokens, rest)) = tokens {
                if !rest.is_empty() {
                    tokens.push(rest.into());
                }
            }
        }
        query.plain = plain.join(" ");
        query
    }

    pub(crate) fn filters_match(&self, entry: &Entry) -> bool {
        let agent_query = self.all_agents
            || !self.agent.is_empty()
            || !self.workspace_or_status.is_empty()
            || !self.status.is_empty();
        if agent_query && entry.agent.is_none() {
            return false;
        }
        let agent_fields: Vec<_> = entry
            .agent
            .iter()
            .flat_map(|agent| {
                [
                    agent.agent.as_deref(),
                    agent.name.as_deref(),
                    agent.display_agent.as_deref(),
                ]
                .into_iter()
                .flatten()
                .chain(agent.aliases.iter().map(String::as_str))
            })
            .collect();
        let status = entry.agent.as_ref().map(|agent| agent.status.as_str());
        let workspace_fields: Vec<_> = [
            entry.workspace_id.as_deref(),
            entry.workspace_label.as_deref(),
            Some(entry.title.as_str()),
            status,
        ]
        .into_iter()
        .flatten()
        .collect();
        let path = entry.path_text();
        tokens_match(&self.agent, &agent_fields)
            && tokens_match(&self.workspace_or_status, &workspace_fields)
            && tokens_match(&self.path, &[&path])
            && tokens_match(&self.status, &[status.unwrap_or_default()])
    }
}

fn tokens_match(tokens: &[String], fields: &[&str]) -> bool {
    tokens.iter().all(|token| {
        fields
            .iter()
            .any(|field| field.to_lowercase().contains(token))
    })
}

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
    fn operators_intersect_each_token_with_only_its_allowed_fields() {
        let input = crate::navigation_test_support::FixtureClient::sample().picker_input();
        let mut entry = input
            .destinations
            .iter()
            .find(|d| {
                d.entry
                    .agent
                    .as_ref()
                    .is_some_and(|agent| agent.agent.as_deref() == Some("gemini"))
            })
            .unwrap()
            .entry
            .clone();
        let agent = entry.agent.as_mut().unwrap();
        agent.name = Some("Named worker".into());
        agent.display_agent = Some("Display identity".into());
        agent.aliases = vec!["Reviewer".into(), "東京".into()];
        assert!(Query::parse(
            "!gemini !named !display !reviewer !東京 @seo @idle /fixture /seo #idle"
        )
        .filters_match(&entry));
        for query in [
            "!seo",
            "!idle",
            "@reviewer",
            "#seo",
            "/missing",
            "!gemini !absent",
            "@seo @absent",
            "#idle #absent",
        ] {
            assert!(!Query::parse(query).filters_match(&entry), "{query}");
        }
        for query in ["@", "@w1", "@seo", "@idle", "#idle", "!reviewer"] {
            assert!(Query::parse(query).filters_match(&entry), "{query}");
        }
        entry.agent = None;
        for query in ["@", "@w1", "@seo", "#idle", "!reviewer"] {
            assert!(!Query::parse(query).filters_match(&entry), "{query}");
        }
        assert!(Query::parse("/seo").filters_match(&entry));
        let raw = "  東京   α  !ReVieWer @IDLE  ";
        let query = Query::parse(raw);
        assert_eq!(query.plain, "東京 α");
        assert_eq!(raw, "  東京   α  !ReVieWer @IDLE  ");
    }

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
