//! Sets of grammatical error correction rules.

use crate::types::*;
use crate::utils::parallelism::MaybeParallelRefIterator;
use crate::{rule::id::Selector, rule::MatchSentence, rule::Rule, tokenizer::Tokenizer, Error};
use fs_err::File;
use serde::{Deserialize, Serialize};
use std::{
    io::{BufReader, Read, Write},
    iter::FromIterator,
    path::Path,
};

/// Language-dependent options for a rule set.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RulesLangOptions {
    /// Whether to allow errors while constructing the rules.
    pub allow_errors: bool,
    /// Grammar Rule selectors to use in this set.
    #[serde(default)]
    pub ids: Vec<Selector>,
    /// Grammar Rule selectors to ignore in this set.
    #[serde(default)]
    pub ignore_ids: Vec<Selector>,
}

impl Default for RulesLangOptions {
    fn default() -> Self {
        RulesLangOptions {
            allow_errors: true,
            ids: Vec::new(),
            ignore_ids: Vec::new(),
        }
    }
}

/// A set of grammatical error correction rules.
#[derive(Serialize, Deserialize, Default)]
pub struct Rules {
    /// Shared auxiliary data for the ported Java rule filters.
    pub(crate) filter_data: crate::rule::filter_data::FilterData,
    pub(crate) rules: Vec<Rule>,
    /// Language code of this rule set, used by the built-in Java-only rules
    /// (e.g. `UPPERCASE_SENTENCE_START`). Not serialized: set it with
    /// [Rules::with_lang] (built-ins degrade to language-independent
    /// behavior when unset).
    #[serde(skip)]
    pub(crate) builtin_lang: Option<String>,
    /// Shared morphological synthesizer used by rules to inflect lemmas in suggestions.
    pub(crate) synth: Option<std::sync::Arc<crate::rule::synthesizer::Synthesizer>>,
}


/// Port of `CleanOverlappingFilter.filter` (LT 6.5). Priorities are all
/// treated as equal (no language overrides priority in the languages we
/// build; the `picky` tag only matters at picky level).
fn clean_overlapping(suggestions: Vec<Suggestion>) -> Vec<Suggestion> {
    let mut clean = Vec::new();
    let mut iter = suggestions.into_iter();
    let mut prev = match iter.next() {
        Some(first) => first,
        None => return clean,
    };
    let (mut prev_from, mut prev_to) =
        (prev.span().char().start, prev.span().char().end);

    for rule_match in iter {
        let (cur_from, cur_to) = (
            rule_match.span().char().start,
            rule_match.span().char().end,
        );
        if cur_from < prev_from {
            // cannot happen (the list is sorted); treat as non-overlapping
            clean.push(prev);
            prev = rule_match;
            let span = prev.span().char().clone();
            prev_from = span.start;
            prev_to = span.end;
            continue;
        }

        let mut is_duplicate_suggestion = false;
        if let (Some(suggestion), Some(prev_suggestion)) = (
            rule_match.replacements().first(),
            prev.replacements().first(),
        ) {
            let (pf, pt) = (prev_from, prev_to); let _ = pf;
            if !suggestion.is_empty() && !prev_suggestion.is_empty() {
                // juxtaposed errors adding a comma in the same place
                if cur_from == pt && prev_suggestion.ends_with(',') && suggestion.starts_with(", ") {
                    is_duplicate_suggestion = true;
                }
                // duplicate suggestion for the same position
                if suggestion.contains(' ')
                    && prev_suggestion.contains(' ')
                    && cur_from == pt + 1
                {
                    let parts: Vec<&str> = suggestion.split(' ').collect();
                    let parts_prev: Vec<&str> = prev_suggestion.split(' ').collect();
                    if parts_prev.len() > 1
                        && parts.len() > 1
                        && parts_prev[1] == parts[0]
                    {
                        is_duplicate_suggestion = true;
                    }
                }
            }
        }

        // no overlap (juxtaposed errors are kept)
        if cur_from >= prev_to && !is_duplicate_suggestion {
            clean.push(prev);
            prev = rule_match;
            let span = prev.span().char().clone();
            prev_from = span.start;
            prev_to = span.end;
            continue;
        }

        // overlapping: equal priorities -> the longer match wins, then the
        // later match wins
        let cur_priority = cur_to - cur_from;
        let prev_priority = prev_to - prev_from;
        if cur_priority >= prev_priority {
            prev = rule_match;
            let span = prev.span().char().clone();
            prev_from = span.start;
            prev_to = span.end;
        }
    }
    clean.push(prev);
    clean
}

impl Rules {
    /// Creates a new rule set from a path to a binary.
    ///
    /// # Errors
    /// - If the file can not be opened.
    /// - If the file content can not be deserialized to a rules set.
    pub fn new<P: AsRef<Path>>(p: P) -> Result<Self, Error> {
        let reader = BufReader::new(File::open(p.as_ref())?);
        let rules: Rules = bincode::deserialize_from(reader)?;
        Ok(rules)
    }

    /// Creates a new rule set from a path to a binary, recording the language
    /// code so the built-in Java-only rules (`UPPERCASE_SENTENCE_START`, ...)
    /// can apply their language-specific behavior. The language is never
    /// serialized with the binary.
    pub fn with_lang<P: AsRef<Path>>(p: P, lang: &str) -> Result<Self, Error> {
        let mut rules = Self::new(p)?;
        rules.builtin_lang = Some(lang.to_string());
        Ok(rules)
    }

    /// Creates a new rules set from a reader.
    pub fn from_reader<R: Read>(reader: R) -> Result<Self, Error> {
        Ok(bincode::deserialize_from(reader)?)
    }

    /// Serializes this rules set to a writer.
    pub fn to_writer<W: Write>(&self, writer: W) -> Result<(), Error> {
        Ok(bincode::serialize_into(writer, &self)?)
    }

    /// All rules ordered by priority.
    pub fn rules(&self) -> &[Rule] {
        &self.rules
    }

    /// The shared morphological synthesizer used by rules to inflect lemmas
    /// in suggestions, if this language has a synthesis dictionary.
    pub fn synthesizer(&self) -> Option<&std::sync::Arc<crate::rule::synthesizer::Synthesizer>> {
        self.synth.as_ref()
    }

    /// The shared filter data (speller, multitoken tables, ...) if any.
    pub fn filter_data(&self) -> &crate::rule::filter_data::FilterData {
        &self.filter_data
    }

    /// All rules ordered by priority (mutable).
    pub fn rules_mut(&mut self) -> &mut [Rule] {
        &mut self.rules
    }

    /// Returns an iterator over all rules matching the selector.
    pub fn select<'a>(&'a self, selector: &'a Selector) -> RulesIter<'a> {
        RulesIter {
            inner: self.rules.iter(),
            selector: Some(selector),
        }
    }

    /// Returns an iterator over all rules matching the selector (mutable).
    pub fn select_mut<'a>(&'a mut self, selector: &'a Selector) -> RulesIterMut<'a> {
        RulesIterMut {
            inner: self.rules.iter_mut(),
            selector: Some(selector),
        }
    }

    /// Compute the suggestions for the given sentence by checking all rules.
    /// Builtin rules that are Java classes in LT (no XML): duplicated words,
    /// doubled punctuation and sentence-start casing.
    fn builtin_suggestions(
        &self,
        sentence: &Sentence,
        ctx: &crate::builtins::TextRuleContext,
    ) -> Vec<Suggestion> {
        // the speller before the casing rule: at equal spans the later match
        // wins in clean_overlapping, so the casing rule replaces the speller
        // only when it is longer, and a pattern rule replaces both - as on
        // the LT server
        let mut out = crate::builtins::morfologik_spelling(
            sentence,
            self.builtin_lang.as_deref(),
            self.filter_data.speller.as_ref(),
        );
        out.extend(crate::builtins::uppercase_sentence_start(
            sentence,
            self.builtin_lang.as_deref(),
            ctx,
        ));
        let tokens: Vec<_> = sentence.iter().collect();
        if tokens.len() < 2 {
            return out;
        }
        // WORD_REPEAT_RULE: adjacent identical words
        for pair in tokens.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            let ta = a.word().as_str();
            let tb = b.word().as_str();
            if !b.has_space_before() || ta.len() < 2 {
                continue;
            }
            if ta.chars().all(char::is_alphabetic)
                && ta.eq_ignore_ascii_case(tb)
                && !matches!(ta.to_lowercase().as_str(), "that" | "had")
            {
                let text = sentence.text();
                let span = Span::from_positions(a.span().start(), a.span().end());
                let word = ta;
                out.push(Suggestion::new(
                    "WORD_REPEAT_RULE".to_string(),
                    format!(
                        "Possible typo: you repeated a word",
                    ),
                    span,
                    vec![word.to_string()],
                ));
                let _ = text;
                break;
            }
        }
        // DOUBLE_PUNCTUATION: doubled , ; : ! ?
        for pair in tokens.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            let ta = a.word().as_str();
            let tb = b.word().as_str();
            if b.has_space_before() {
                continue;
            }
            if ta.chars().count() == 1
                && ta == tb
                && [",", ";", ":", "!", "?"].contains(&ta)
            {
                let span = Span::from_positions(a.span().start(), b.span().end());
                out.push(Suggestion::new(
                    "DOUBLE_PUNCTUATION".to_string(),
                    "Double punctuation.".to_string(),
                    span,
                    vec![ta.to_string()],
                ));
                break;
            }
        }
        out
    }

    pub fn apply(&self, sentence: &Sentence) -> Vec<Suggestion> {
        self.apply_with_context(sentence, &crate::builtins::TextRuleContext::default())
    }

    /// Like [Rules::apply], with the text-level context of the built-in rules
    /// (position of this sentence within the checked text).
    pub fn apply_with_context(
        &self,
        sentence: &Sentence,
        ctx: &crate::builtins::TextRuleContext,
    ) -> Vec<Suggestion> {
        let plain_sentence = sentence;
        let sentence = MatchSentence::new(sentence);

        let mut output: Vec<(usize, Suggestion)> = self
            .rules
            .maybe_par_iter()
            .enumerate()
            .filter(|(_, rule)| rule.enabled())
            .map(|(i, rule)| {
                let mut output = Vec::new();

                for suggestion in rule.apply_with_synth(
                    &sentence,
                    self.synth.as_deref(),
                    Some(&self.filter_data),
                ) {
                    output.push((i, suggestion));
                }

                output
            })
            .flatten()
            .collect();

        for (i, suggestion) in self.builtin_suggestions(plain_sentence, ctx)
            .into_iter()
            .enumerate()
            .map(|(j, sugg)| (self.rules.len() + j, sugg))
        {
            output.push((i, suggestion));
        }


        output.sort_by(|(ia, a), (ib, b)| {
            a.span()
                .char()
                .start
                .cmp(&b.span().char().start)
                .then_with(|| ib.cmp(ia))
        });

        // Port of LT's CleanOverlappingFilter (the HTTP server always runs
        // with it): walk the position-sorted matches, drop overlapping ones -
        // ties resolved by longer match, then by the later match - and the
        // juxtaposed comma duplicate-suggestion cases. The list is already
        // sorted by start; at equal starts the built-ins (appended last,
        // sorted first) come first, so a pattern or speller match at the same
        // span replaces them, like on the server.
        let suggestions: Vec<_> = output
            .into_iter()
            .map(|(_, suggestion)| {
                suggestion
                    .span()
                    .clone()
                    .lshift(sentence.span().start());
                suggestion
            })
            .collect();

        clean_overlapping(suggestions)
    }

    /// Compute the suggestions for a text by checking all rules.
    pub fn suggest(&self, text: &str, tokenizer: &Tokenizer) -> Vec<Suggestion> {
        if text.is_empty() {
            return Vec::new();
        }

        let mut suggestions = Vec::new();

        // get suggestions sentence by sentence, carrying the text-level state
        // LT's text-level built-in rules see (previous sentence's last token,
        // numbered-list detection)
        let sentences: Vec<_> = tokenizer.pipe(text).collect();
        let mut prev_last_token: Option<String> = None;
        let mut prev_numbered_list = false;

        for (i, sentence) in sentences.iter().enumerate() {
            let ctx = crate::builtins::TextRuleContext {
                is_only_sentence: sentences.len() == 1,
                prev_last_token: prev_last_token.clone(),
                prev_numbered_list,
            };
            suggestions.extend(self.apply_with_context(sentence, &ctx));

            prev_last_token = crate::builtins::last_significant_token(sentence);
            prev_numbered_list = crate::builtins::is_numbered_list_item(sentence.text());
            let _ = i;
        }

        suggestions
    }

    /// Correct a text by first tokenizing, then finding all suggestions and choosing the first replacement of each suggestion.
    pub fn correct(&self, text: &str, tokenizer: &Tokenizer) -> String {
        let suggestions = self.suggest(text, tokenizer);
        apply_suggestions(text, &suggestions)
    }
}

/// Correct a text by applying suggestions to it.
/// In the case of multiple possible replacements, always chooses the first one.
pub fn apply_suggestions(text: &str, suggestions: &[Suggestion]) -> String {
    let mut offset: isize = 0;
    let mut chars: Vec<_> = text.chars().collect();

    for suggestion in suggestions {
        let replacement: Vec<_> = suggestion.replacements()[0].chars().collect();
        chars.splice(
            (suggestion.span().char().start as isize + offset) as usize
                ..(suggestion.span().char().end as isize + offset) as usize,
            replacement.iter().cloned(),
        );
        offset = offset + replacement.len() as isize - suggestion.span().char().len() as isize;
    }

    chars.into_iter().collect()
}

/// An iterator over references to rules.
pub struct RulesIter<'a> {
    selector: Option<&'a Selector>,
    inner: std::slice::Iter<'a, Rule>,
}

impl<'a> Iterator for RulesIter<'a> {
    type Item = &'a Rule;
    fn next(&mut self) -> Option<Self::Item> {
        let selector = self.selector.as_ref();

        self.inner
            .find(|rule| selector.map_or(true, |s| s.is_match(rule.id())))
    }
}

/// An iterator over mutable references to rules.
pub struct RulesIterMut<'a> {
    selector: Option<&'a Selector>,
    inner: std::slice::IterMut<'a, Rule>,
}

impl<'a> Iterator for RulesIterMut<'a> {
    type Item = &'a mut Rule;
    fn next(&mut self) -> Option<Self::Item> {
        let selector = self.selector.as_ref();

        self.inner
            .find(|rule| selector.map_or(true, |s| s.is_match(rule.id())))
    }
}

impl IntoIterator for Rules {
    type Item = Rule;
    type IntoIter = std::vec::IntoIter<Rule>;
    fn into_iter(self) -> Self::IntoIter {
        self.rules.into_iter()
    }
}

impl<R> FromIterator<R> for Rules
where
    R: Into<Rule>,
{
    fn from_iter<I: IntoIterator<Item = R>>(iter: I) -> Self {
        let rules: Vec<Rule> = iter.into_iter().map(|x| x.into()).collect();
        Self {
            rules,
            synth: None,
            filter_data: Default::default(),
            builtin_lang: None,
        }
    }
}
