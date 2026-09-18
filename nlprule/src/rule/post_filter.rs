//! Runtime post-filters applied to grammar rule matches before they become
//! suggestions. These mirror LanguageTool's `RuleFilter` classes which operate
//! on a fired rule match (adjusting its span, its replacements, or rejecting it).

use crate::rule::engine::composition::MatchSentence;
use crate::types::Span;
use crate::utils::regex::Regex;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum PostFilter {
    /// Mirrors `UnderlineSpacesFilter`: extends the match span over adjacent whitespace.
    UnderlineSpaces { mode: UnderlineMode },
    /// Mirrors `ApostropheTypeFilter`: only accept the match if the matched text
    /// contains (or does not contain) a typographic apostrophe (’).
    ApostropheType { has_typographic: bool },
    /// Mirrors `RegexAntiPatternFilter`: rejects the match if any regex matches
    /// the sentence text overlapping the match span.
    RegexAntiPattern { regexes: Vec<Regex> },
    /// Mirrors `AdaptSuggestionsFilter` (`Language.adaptSuggestion`): adapts the
    /// case of the replacements to the original error text.
    AdaptSuggestions,
    /// Mirrors `*SuppressMisspelledSuggestionsFilter`: drops replacements that
    /// are not in the analyzer dictionary (approximating LT's spellchecker check).
    /// If all replacements are dropped, the match is suppressed entirely
    /// (unless `suppress_match` is false, in which case the original
    /// replacements are kept).
    SuppressMisspelled { suppress_match: bool },
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnderlineMode {
    Before,
    After,
    Both,
}

const TYPOGRAPHIC_APOSTROPHE: char = '\u{2019}';

/// Result of applying a post-filter to a fired rule match.
pub struct FilteredMatch {
    pub span: Span,
    pub replacements: Vec<String>,
}

impl PostFilter {
    /// Applies this filter. Returns `None` if the match should be rejected.
    pub(crate) fn apply(
        &self,
        sentence: &MatchSentence,
        span: Span,
        replacements: Vec<String>,
        matched_text: &str,
    ) -> Option<FilteredMatch> {
        match self {
            PostFilter::UnderlineSpaces { mode } => {
                let text = sentence.text();
                let mut start = span.byte().start;
                let mut end = span.byte().end;

                if matches!(mode, UnderlineMode::Before | UnderlineMode::Both) {
                    while start > 0 && text[..start].chars().last().map_or(false, char::is_whitespace)
                    {
                        start -= text[..start].chars().last().unwrap().len_utf8();
                    }
                }
                if matches!(mode, UnderlineMode::After | UnderlineMode::Both) {
                    while end < text.len() && text[end..].chars().next().map_or(false, char::is_whitespace)
                    {
                        end += text[end..].chars().next().unwrap().len_utf8();
                    }
                }

                // keep the char span consistent: recompute by counting chars
                let out = Span::new(
                    start..end,
                    text[..start].chars().count()..text[..end].chars().count(),
                );

                Some(FilteredMatch {
                    span: out,
                    replacements,
                })
            }
            PostFilter::ApostropheType {
                has_typographic,
            } => {
                let contains = matched_text.contains(TYPOGRAPHIC_APOSTROPHE);
                if contains == *has_typographic {
                    Some(FilteredMatch { span, replacements })
                } else {
                    None
                }
            }
            PostFilter::RegexAntiPattern { regexes } => {
                let text = sentence.text();
                let (from, to) = (span.byte().start, span.byte().end);

                for regex in regexes {
                    for mat in regex.find_iter(text) {
                        let (mstart, mend) = (mat.start(), mat.end());
                        if (mstart <= to && mend >= to) || (mstart <= from && mend >= from) {
                            return None;
                        }
                    }
                }

                Some(FilteredMatch { span, replacements })
            }
            PostFilter::AdaptSuggestions => {
                // adapt the case of each replacement to the original error text,
                // mirroring Language.adaptSuggestion's default behavior
                let replacements = replacements
                    .into_iter()
                    .map(|replacement| adapt_case(&replacement, matched_text))
                    .collect();

                Some(FilteredMatch { span, replacements })
            }
            PostFilter::SuppressMisspelled { suppress_match } => {
                let tagger = sentence.tagger();

                let known: Vec<String> = replacements
                    .iter()
                    .filter(|replacement| is_known_word(replacement, tagger))
                    .cloned()
                    .collect();

                if known.is_empty() {
                    if *suppress_match {
                        None
                    } else {
                        Some(FilteredMatch { span, replacements })
                    }
                } else {
                    Some(FilteredMatch {
                        span,
                        replacements: known,
                    })
                }
            }
        }
    }
}

/// Copies the case of `sample` onto `input` (all-upper, first-upper or as-is),
/// mirroring `Language.adaptSuggestion`'s case adaptation.
fn adapt_case(input: &str, sample: &str) -> String {
    let first_upper = sample.chars().next().map_or(false, |c| c.is_uppercase());

    if first_upper {
        let has_lower = sample.chars().any(|c| c.is_lowercase());
        let char_count = sample.chars().count();

        if char_count > 1 && !has_lower {
            input.to_uppercase()
        } else {
            crate::utils::apply_to_first(input, |c| c.to_uppercase().collect())
        }
    } else {
        input.to_string()
    }
}

/// Whether a replacement is considered correctly spelled:
/// every alphabetic part is in the analyzer dictionary (as-is, lowercased
/// or title-cased). Approximates LT's spellchecker-based check.
fn is_known_word(replacement: &str, tagger: &crate::tokenizer::tag::Tagger) -> bool {
    let parts: Vec<&str> = replacement
        .split(|c: char| {
            c.is_whitespace()
                || crate::utils::splitting_chars()
                    .chars()
                    .any(|s| s == c)
        })
        .filter(|p| p.chars().any(char::is_alphabetic))
        .collect();

    if parts.is_empty() {
        return true;
    }

    parts.iter().all(|part| {
        tagger.id_word((*part).into()).1.is_some()
            || tagger
                .id_word(part.to_lowercase().into())
                .1
                .is_some()
            || tagger.id_word(part.to_uppercase().into()).1.is_some()
    })
}
