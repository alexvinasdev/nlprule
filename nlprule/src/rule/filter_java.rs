//! Runtime implementations of LanguageTool's Java `RuleFilter` classes.
//!
//! Each enum variant mirrors one Java filter family; the logic is ported
//! from LanguageTool 6.5 sources. Filters receive a [FilterCtx] with the
//! sentence, the fired match, its replacements and the language's
//! [FilterData] (speller, multitoken tables, confusion pairs).

use crate::rule::engine::composition::MatchSentence;
use crate::rule::filter_data::{
    levenshtein, osa_distance, remove_diacritics, FilterData, MultitokenSuggester,
};
use crate::rule::synthesizer::Synthesizer;
use crate::tokenizer::tag::Tagger;
use crate::types::{Position, Span, Token};
use crate::utils::regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

/// Which language's weekday/month tables a date filter uses.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateLang {
    En,
    De,
    Fr,
    Es,
    Ca,
    It,
    Nl,
    Pt,
    Ru,
    Pl,
    Uk,
    Sr,
    Br,
    Eo,
}

/// Variant of the `FindSuggestionsFilter` family.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindSuggVariant {
    En,
    Fr,
    Es,
    Ca,
}

/// A parsed LT `<filter class="...">` with its raw `args`
/// (values may contain `\N` backreferences resolved at runtime).
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct JavaFilter {
    pub class: JClass,
    pub args: Vec<(String, String)>,
    /// Args whose values are Java regexes, translated at compile time.
    pub regex_args: Vec<(String, Regex)>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum JClass {
    /// `*DateCheckFilter` (+ the "WithSuggestions" behavior used by most languages).
    DateCheck {
        lang: DateLang,
        with_suggestions: bool,
    },
    DMYDateCheck {
        lang: DateLang,
    },
    YMDDateCheck {
        lang: DateLang,
    },
    FutureDate {
        lang: DateLang,
    },
    NewYearDate {
        lang: DateLang,
        ymd: bool,
    },
    RecentYear,
    DateRange,
    ShortenedYearRange,
    WhitespaceCheck,
    MultitokenSpeller,
    SuppressMisspelledSuggestions,
    FindSuggestions {
        variant: FindSuggVariant,
    },
    AdvancedSynthesizer,
    PartialPosTag,
    AddCommas,
    OrdinalSuffix,
    CompoundCheck,
    NlCompound,
    InnNumber,
    DecadeSpelling,
    UppercaseNounReading,
    ConvertToSentenceCase,
    ConfusionCheck,
    DiacriticsCheck,
    RomanNumeral,
    RegularIrregularParticiple,
    ValidWord,
    RemoveUnknownCompounds,
}

/// Everything a Java filter needs at runtime.
pub struct FilterCtx<'a, 't> {
    pub sentence: &'a MatchSentence<'t>,
    pub tagger: &'a Tagger,
    pub synth: Option<&'a Synthesizer>,
    pub data: &'a FilterData,
    /// Base (already expanded) message of the rule, without filter rewrites.
    pub message: String,
    pub span: Span,
    pub replacements: Vec<String>,
    /// Matched text (the original error string).
    pub matched_text: String,
    /// Indices (into the sentence) of the tokens matched by the pattern.
    pub matched_tokens: Vec<usize>,
    /// Args of the currently applied filter (raw; `\N` unresolved).
    pub(crate) filter_args: Option<Vec<(String, String)>>,
    /// Regex args of the currently applied filter (already Java-translated).
    pub(crate) filter_regex_args: Option<Vec<(String, Regex)>>,
}

impl<'a, 't> FilterCtx<'a, 't> {
    /// The index of the first matched token in the sentence (LT's `patternTokenPos`).
    pub fn pattern_token_pos(&self) -> usize {
        self.matched_tokens.first().copied().unwrap_or(0)
    }

    pub fn arg(&self, key: &str) -> Option<String> {
        self.args_get(key).map(|v| self.resolve_arg(&v))
    }

    pub fn args_get(&self, key: &str) -> Option<String> {
        self.filter_args
            .as_ref()?
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
    }

    pub fn args_contains(&self, key: &str) -> bool {
        self.args_get(key).is_some()
    }

    /// A pre-compiled regex arg (cloned).
    pub fn regex_arg(&self, key: &str) -> Option<Regex> {
        self.filter_regex_args
            .as_ref()?
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
    }

    /// Replaces `\N` backreferences with the text of the N-th matched token.
    fn resolve_arg(&self, value: &str) -> String {
        let mut out = String::with_capacity(value.len());
        let chars: Vec<char> = value.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == '\\' && i + 1 < chars.len() && chars[i + 1].is_ascii_digit() {
                let mut j = i + 1;
                let mut num = 0usize;
                while j < chars.len() && chars[j].is_ascii_digit() {
                    num = num * 10 + (chars[j] as usize - '0' as usize);
                    j += 1;
                }
                if num >= 1 {
                    if let Some(&idx) = self.matched_tokens.get(num - 1) {
                        out.push_str(self.sentence.index(idx).word().as_str());
                    }
                }
                i = j;
            } else {
                out.push(chars[i]);
                i += 1;
            }
        }
        out
    }

    /// Text of the n-th matched token.
    fn token_at_matched(&self, n: usize) -> Option<&str> {
        let &idx = self.matched_tokens.get(n)?;
        Some(self.sentence.index(idx).word().as_str())
    }

    /// Span of the n-th matched token.
    fn token_span_at_matched(&self, n: usize) -> Option<Span> {
        let &idx = self.matched_tokens.get(n)?;
        Some(self.sentence.index(idx).span().clone())
    }

    fn token_has_space_before_at_matched(&self, n: usize) -> bool {
        self.matched_tokens
            .get(n)
            .map(|&idx| self.sentence.index(idx).has_space_before())
            .unwrap_or(false)
    }

    /// Span covering matched tokens n..=m.
    fn matched_range_span(&self, n: usize, m: usize) -> Option<Span> {
        let start = self.token_span_at_matched(n)?;
        let end = self.token_span_at_matched(m)?;
        Some(Span::from_positions(start.start(), end.end()))
    }

    fn sentence_tokens(&self) -> Vec<&Token<'_>> {
        (0..self.sentence.len()).map(|i| self.sentence.index(i)).collect()
    }

    /// The tokens matched by the pattern (LT passes `Arrays.copyOfRange(
    /// tokens, firstMatchToken, lastMatchToken + 1)` to its filters).
    fn matched_token_refs(&self) -> Vec<&Token<'_>> {
        self.matched_tokens
            .iter()
            .map(|&i| self.sentence.index(i))
            .collect()
    }
}

/// Result of a Java filter: possibly a new span, replacements and message.
pub struct JavaOutcome {
    pub span: Span,
    pub replacements: Vec<String>,
    pub message: Option<String>,
}

fn keep(ctx: &FilterCtx, replacements: Vec<String>) -> Option<JavaOutcome> {
    Some(JavaOutcome {
        span: ctx.span.clone(),
        replacements,
        message: None,
    })
}

impl JavaFilter {
    pub(crate) fn apply(&self, ctx: &mut FilterCtx) -> Option<JavaOutcome> {
        ctx.filter_args = Some(self.args.clone());
        ctx.filter_regex_args = Some(self.regex_args.clone());
        let out = self.class.apply(ctx);
        ctx.filter_args = None;
        ctx.filter_regex_args = None;
        out
    }
}

impl JClass {
    fn apply(&self, ctx: &mut FilterCtx) -> Option<JavaOutcome> {
        let replacements = std::mem::take(&mut ctx.replacements);
        match self {
            JClass::DateCheck {
                lang,
                with_suggestions,
            } => date::date_check(ctx, *lang, *with_suggestions, date::DateInput::Separate),
            JClass::DMYDateCheck { lang } => {
                date::date_check(ctx, *lang, true, date::DateInput::Dmy)
            }
            JClass::YMDDateCheck { lang } => {
                date::date_check(ctx, *lang, true, date::DateInput::Ymd)
            }
            JClass::FutureDate { lang } => date::future_date(ctx, *lang, replacements),
            JClass::NewYearDate { lang, ymd } => date::new_year_date(ctx, *lang, *ymd, replacements),
            JClass::RecentYear => {
                let this_year = now().0;
                let year: i32 = ctx.arg("year")?.parse().ok()?;
                let back: i32 = ctx.arg("maxYearsBack")?.parse().ok()?;
                if year < this_year && year >= this_year - back {
                    keep(ctx, replacements)
                } else {
                    None
                }
            }
            JClass::DateRange => {
                let x: i64 = ctx.arg("x")?.parse().ok()?;
                let y: i64 = ctx.arg("y")?.parse().ok()?;
                if x >= y {
                    keep(ctx, replacements)
                } else {
                    None
                }
            }
            JClass::ShortenedYearRange => {
                let xs = ctx.arg("x")?;
                let ys = ctx.arg("y")?;
                let x: i64 = xs.parse().ok()?;
                let prefix: String = xs.chars().take(2).collect();
                let y: i64 = format!("{}{}", prefix, ys).parse().ok()?;
                if x >= y {
                    keep(ctx, replacements)
                } else {
                    None
                }
            }
            JClass::WhitespaceCheck => {
                let ws_char = ctx.arg("whitespaceChar")?;
                let pos: usize = ctx.arg("position")?.parse().ok()?;
                let tokens = ctx.matched_token_refs();
                let token = tokens.get(pos.checked_sub(1)?)?;
                let has_ws = if token.has_space_before() { " " } else { "" };
                if has_ws != ws_char {
                    keep(ctx, replacements)
                } else {
                    None
                }
            }
            JClass::MultitokenSpeller => multitoken::apply(ctx),
            JClass::SuppressMisspelledSuggestions => suppress_misspelled(ctx, replacements),
            JClass::FindSuggestions { variant } => find_suggestions(ctx, *variant),
            JClass::AdvancedSynthesizer => advanced_synthesizer(ctx, replacements),
            JClass::PartialPosTag => partial_pos_tag(ctx, replacements),
            JClass::AddCommas => add_commas(ctx),
            JClass::OrdinalSuffix => {
                let mut ordinal: String = replacements
                    .first()
                    .map(|x| x.chars().filter(|c| c.is_ascii_digit()).collect())
                    .unwrap_or_default();
                if ordinal.is_empty() {
                    return None;
                }
                if ordinal.ends_with("11") || ordinal.ends_with("12") || ordinal.ends_with("13") {
                    ordinal.push_str("th");
                } else if ordinal.ends_with('1') {
                    ordinal.push_str("st");
                } else if ordinal.ends_with('2') {
                    ordinal.push_str("nd");
                } else if ordinal.ends_with('3') {
                    ordinal.push_str("rd");
                } else {
                    ordinal.push_str("th");
                }
                keep(ctx, vec![ordinal])
            }
            JClass::CompoundCheck => {
                let part1 = ctx.arg("part1")?.to_lowercase();
                let part2 = ctx.arg("part2")?.to_lowercase();
                let ok = ctx
                    .data
                    .added_compound
                    .as_ref()
                    .and_then(|m| m.get(&part1))
                    .map(|list| list.contains(&part2))
                    .unwrap_or(false);
                if ok {
                    keep(ctx, replacements)
                } else {
                    None
                }
            }
            JClass::NlCompound => {
                let mut words = Vec::new();
                for i in 1..=5 {
                    match ctx.arg(&format!("word{}", i)) {
                        Some(w) => words.push(w),
                        None => break,
                    }
                }
                let repl = glue_parts(ctx, &words);
                let message = rewrite_suggestion_element(&ctx.message, &repl);
                keep(ctx, vec![repl]).map(|mut o| {
                    o.message = Some(message);
                    o
                })
            }
            JClass::InnNumber => {
                let inn = ctx.arg("inn")?;
                if !inn.chars().all(|c| c.is_ascii_digit()) {
                    return None;
                }
                let digits: Vec<u32> = inn.chars().map(|c| c.to_digit(10).unwrap()).collect();
                match digits.len() {
                    10 => {
                        let kz = (digits[0] * 2
                            + digits[1] * 4
                            + digits[2] * 10
                            + digits[3] * 3
                            + digits[4] * 5
                            + digits[5] * 9
                            + digits[6] * 4
                            + digits[7] * 6
                            + digits[8] * 8)
                            % 11;
                        let kz = if kz > 9 { kz - 10 } else { kz };
                        if digits[9] != kz {
                            keep(ctx, replacements)
                        } else {
                            None
                        }
                    }
                    12 => {
                        let kz1 = (digits[0] * 7
                            + digits[1] * 2
                            + digits[2] * 4
                            + digits[3] * 10
                            + digits[4] * 3
                            + digits[5] * 5
                            + digits[6] * 9
                            + digits[7] * 4
                            + digits[8] * 6
                            + digits[9] * 8)
                            % 11;
                        let kz2 = (digits[0] * 3
                            + digits[1] * 7
                            + digits[2] * 2
                            + digits[3] * 4
                            + digits[4] * 10
                            + digits[5] * 3
                            + digits[6] * 5
                            + digits[7] * 9
                            + digits[8] * 4
                            + digits[9] * 6
                            + digits[10] * 8)
                            % 11;
                        let kz1 = if kz1 > 9 { kz1 - 10 } else { kz1 };
                        let kz2 = if kz2 > 9 { kz2 - 10 } else { kz2 };
                        if digits[10] != kz1 || digits[11] != kz2 {
                            keep(ctx, replacements)
                        } else {
                            None
                        }
                    }
                    _ => None,
                }
            }
            JClass::DecadeSpelling => {
                let lata = ctx.arg("lata")?;
                let decade: String = lata.chars().skip(2).collect();
                let century: String = lata.chars().take(2).collect();
                let cent: i32 = century.parse().ok()?;
                let message = ctx
                    .message
                    .replace("{dekada}", &decade)
                    .replace("{wiek}", &roman_number(cent + 1));
                Some(JavaOutcome {
                    span: ctx.span.clone(),
                    replacements,
                    message: Some(message),
                })
            }
            JClass::UppercaseNounReading => {
                let token = ctx.arg("token")?;
                let uppercase = uppercase_first(&token);
                let has_noun_reading = ctx
                    .tagger
                    .get_tags_with_options(&uppercase, Some(false), Some(false))
                    .any(|data| {
                        let pos = data.pos().as_str();
                        pos.starts_with("SUB:") && !pos.starts_with("ADJ")
                    });
                if has_noun_reading {
                    keep(ctx, replacements)
                } else {
                    None
                }
            }
            JClass::ConvertToSentenceCase => convert_to_sentence_case(ctx),
            JClass::ConfusionCheck => confusion_check(ctx, replacements, false),
            JClass::DiacriticsCheck => confusion_check(ctx, replacements, true),
            JClass::RomanNumeral => {
                let arabic = ctx.arg("arabicSource")?;
                let roman = roman_number_str(&arabic);
                keep(ctx, vec![roman])
            }
            JClass::RegularIrregularParticiple => regular_irregular_participle(ctx, replacements),
            JClass::ValidWord => {
                let word1a = ctx.arg("word1")?;
                let word2a = ctx.arg("word2")?;
                let word1 = format!("{}{}", word1a, word2a);
                let word2 = format!("{}{}", word1a, word2a.to_lowercase());
                let misspelled = |w: &str| !is_known_word_ctx(ctx, w);
                if !misspelled(&word1) || !misspelled(&word2) {
                    None
                } else {
                    keep(ctx, replacements)
                }
            }
            JClass::RemoveUnknownCompounds => {
                let compound = format!(
                    "{}{}",
                    ctx.arg("part1")?,
                    ctx.arg("part2")?.to_lowercase()
                );
                if is_known_word_ctx(ctx, &compound) {
                    keep(ctx, replacements)
                } else {
                    None
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

/// Current (year, month 1-12, day), honoring `NLPRULE_DATE_ANCHOR=YYYY-MM-DD`
/// so tests can reproduce LT's JUnit-test date hack (2014-01-01).
pub fn now() -> (i32, u32, u32) {
    static ANCHOR: OnceLock<Option<(i32, u32, u32)>> = OnceLock::new();
    let anchor = ANCHOR.get_or_init(|| {
        std::env::var("NLPRULE_DATE_ANCHOR")
            .ok()
            .and_then(|v| parse_ymd(&v))
    });
    if let Some(date) = anchor {
        return *date;
    }
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|x| x.as_secs() as i64)
        .unwrap_or(0);
    let days = secs.div_euclid(86400);
    civil_from_days(days)
}

/// Whether the test date anchor is active (LT's `TestHackHelper.isJUnitTest`).
fn test_hack() -> bool {
    std::env::var("NLPRULE_DATE_ANCHOR").is_ok()
}

fn parse_ymd(s: &str) -> Option<(i32, u32, u32)> {
    let mut parts = s.splitn(3, '-');
    let y = parts.next()?.parse().ok()?;
    let m = parts.next()?.parse().ok()?;
    let d = parts.next()?.parse().ok()?;
    Some((y, m, d))
}

/// Howard Hinnant's `civil_from_days`.
pub fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    ((y + i64::from(m <= 2)) as i32, m, d)
}

/// Days since the Unix epoch (1970-01-01).
pub fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    let y = y as i64 - i64::from(m <= 2);
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u64;
    let mp = if m > 2 { m - 3 } else { m + 9 } as u64;
    let doy = (153 * mp + 2) / 5 + d as u64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe as i64 - 719_468
}

fn is_leap(y: i32) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn days_in_month(y: i32, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(y) => 29,
        2 => 28,
        _ => 0,
    }
}

fn valid_date(y: i32, m: u32, d: u32) -> bool {
    (1..=12).contains(&m) && d >= 1 && d <= days_in_month(y, m)
}

/// Java `Calendar.DAY_OF_WEEK`: Sunday=1 ... Saturday=7.
fn day_of_week(y: i32, m: u32, d: u32) -> u32 {
    let days = days_from_civil(y, m, d);
    // 1970-01-01 was a Thursday (5)
    ((days + 4).rem_euclid(7) + 1) as u32
}

pub(crate) fn uppercase_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

pub(crate) fn is_capitalized(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => {
            c.is_uppercase() && !chars.as_str().is_empty() && chars.all(|x| x.is_lowercase())
        }
        None => false,
    }
}

pub(crate) fn is_all_uppercase(s: &str) -> bool {
    s.chars().any(|c| c.is_alphabetic()) && s.chars().all(|c| !c.is_lowercase())
}

/// `StringTools.preserveCase`.
pub(crate) fn preserve_case(input: &str, sample: &str) -> String {
    if is_all_uppercase(sample) {
        input.to_uppercase()
    } else if sample.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
        uppercase_first(input)
    } else {
        input.to_string()
    }
}

/// `StringTools.trimSpecialCharacters`: trim non-alphanumeric chars from both ends.
fn trim_special(s: &str) -> String {
    s.trim_matches(|c: char| !c.is_alphanumeric()).to_string()
}

fn roman_number(n: i32) -> String {
    let numbers = [1000, 900, 500, 400, 100, 90, 50, 40, 10, 9, 5, 4, 1];
    let letters = ["M", "CM", "D", "CD", "C", "XC", "L", "XL", "X", "IX", "V", "IV", "I"];
    let mut n = n;
    let mut out = String::new();
    for (i, &v) in numbers.iter().enumerate() {
        while n >= v {
            out.push_str(letters[i]);
            n -= v;
        }
    }
    out
}

fn roman_number_str(s: &str) -> String {
    s.parse::<i32>()
        .map(|n| roman_number(n))
        .unwrap_or_else(|_| s.to_string())
}

/// nl `Tools.glueParts`: join compound parts, inserting hyphens between
/// vowels/case changes/digits/hyphens.
fn glue_parts(ctx: &FilterCtx, words: &[String]) -> String {
    const VOWEL_PAIRS: [&str; 21] = [
        "aa", "ae", "ai", "ao", "au", "ee", "ei", "eu", "ée", "éi", "éu", "ie", "ii", "oe", "oi",
        "oo", "ou", "ui", "uu", "ij", "ij",
    ];
    let mut compound = words.first().cloned().unwrap_or_default();
    for word2 in words.iter().skip(1) {
        let spelled = is_known_word_ctx(ctx, &compound);
        if compound.chars().count() > 2 || spelled {
            let last_char = compound.chars().last().unwrap_or(' ');
            let first_char = word2.chars().next().unwrap_or(' ');
            let connection: String = [last_char, first_char].iter().collect();
            let ends_digit = compound.chars().last().map(|c| c.is_ascii_digit()).unwrap_or(false);
            let starts_digit = word2.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false);
            let needs_hyphen = VOWEL_PAIRS.iter().any(|p| connection.contains(p))
                || (first_char.is_uppercase() && last_char.is_lowercase())
                || (last_char.is_uppercase() && first_char.is_lowercase())
                || (last_char.is_uppercase() && first_char.is_uppercase())
                || ends_digit
                || starts_digit
                || compound.contains('-')
                || word2.contains('-');
            if needs_hyphen {
                compound.push('-');
            }
            compound.push_str(word2);
        } else {
            compound.push_str(word2);
        }
    }
    compound
}

fn rewrite_suggestion_element(message: &str, repl: &str) -> String {
    let mut out = String::new();
    let mut rest = message;
    while let Some(start) = rest.find("<suggestion>") {
        let end = rest[start..]
            .find("</suggestion>")
            .map(|e| start + e + "</suggestion>".len())
            .unwrap_or(rest.len());
        out.push_str(&rest[..start]);
        out.push_str("<suggestion>");
        out.push_str(repl);
        out.push_str("</suggestion>");
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

fn is_known_word_ctx(ctx: &FilterCtx, word: &str) -> bool {
    if word.is_empty() {
        return false;
    }
    if let Some(speller) = ctx.data.speller.as_ref() {
        if !speller.is_empty() {
            let mut any = false;
            for token in word.split(|c: char| c.is_whitespace() || c == '-') {
                if token.is_empty() {
                    continue;
                }
                any = true;
                if !speller.is_known(token) {
                    return false;
                }
            }
            if any {
                return true;
            }
        }
    }
    // fall back to the analyzer dictionary
    post_filter_is_known(word, ctx.tagger)
}

fn post_filter_is_known(replacement: &str, tagger: &Tagger) -> bool {
    let parts: Vec<&str> = replacement
        .split(|c: char| {
            c.is_whitespace() || crate::utils::splitting_chars().chars().any(|s| s == c)
        })
        .filter(|p| p.chars().any(char::is_alphabetic))
        .collect();

    if parts.is_empty() {
        return true;
    }

    parts.iter().all(|part| {
        tagger.id_word((*part).into()).1.is_some()
            || tagger.id_word(part.to_lowercase().into()).1.is_some()
            || tagger.id_word(part.to_uppercase().into()).1.is_some()
    })
}

/// Java `String.matches`/`matchesPosTagRegex`: full-string regex match.
fn matches_full_regex(regex: &Regex, text: &str) -> bool {
    regex
        .captures(text)
        .and_then(|c| c.get(0))
        .map(|m| m.as_str() == text)
        .unwrap_or(false)
}

fn matches_full(regex: &Regex, text: &str) -> bool {
    regex
        .captures(text)
        .and_then(|c| c.get(0))
        .map(|m| m.as_str() == text)
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// date filter family
// ---------------------------------------------------------------------------

mod date {
    use super::*;

    pub enum DateInput {
        Separate,
        Dmy,
        Ymd,
    }

    fn weekday_from_string(lang: DateLang, s: &str) -> Option<u32> {
        let day = trim_special(&s.to_lowercase().replace('\u{00AD}', ""));
        let day = day.as_str();
        let starts = |p: &str| day.starts_with(p);
        let eq = |p: &str| day == p;
        Some(match lang {
            DateLang::En if starts("su") => 1,
            DateLang::En if starts("mo") => 2,
            DateLang::En if starts("tu") => 3,
            DateLang::En if starts("we") => 4,
            DateLang::En if starts("th") => 5,
            DateLang::En if starts("fr") => 6,
            DateLang::En if starts("sa") => 7,

            DateLang::De if starts("sonnabend") => 7,
            DateLang::De if starts("so") => 1,
            DateLang::De if starts("mo") => 2,
            DateLang::De if starts("di") => 3,
            DateLang::De if starts("mi") => 4,
            DateLang::De if starts("do") => 5,
            DateLang::De if starts("fr") => 6,
            DateLang::De if starts("sa") => 7,

            DateLang::Fr if starts("dim") => 1,
            DateLang::Fr if starts("lun") => 2,
            DateLang::Fr if starts("mar") => 3,
            DateLang::Fr if starts("mer") => 4,
            DateLang::Fr if starts("jeu") => 5,
            DateLang::Fr if starts("ven") => 6,
            DateLang::Fr if starts("sam") => 7,

            DateLang::Es if eq("do") || eq("domingo") => 1,
            DateLang::Es if eq("lu") || eq("lunes") => 2,
            DateLang::Es if eq("ma") || eq("martes") => 3,
            DateLang::Es if eq("mi") || eq("miércoles") => 4,
            DateLang::Es if eq("ju") || eq("jueves") => 5,
            DateLang::Es if eq("vi") || eq("viernes") => 6,
            DateLang::Es if eq("sa") || eq("sábado") => 7,

            DateLang::Ca if eq("dg") || eq("diumenge") => 1,
            DateLang::Ca if eq("dl") || eq("dilluns") => 2,
            DateLang::Ca if eq("dt") || eq("dimarts") => 3,
            DateLang::Ca if eq("dc") || eq("dimecres") => 4,
            DateLang::Ca if eq("dj") || eq("dijous") => 5,
            DateLang::Ca if eq("dv") || eq("divendres") => 6,
            DateLang::Ca if eq("ds") || eq("dissabte") => 7,

            DateLang::It if starts("do") => 1,
            DateLang::It if starts("lu") => 2,
            DateLang::It if starts("ma") => 3,
            DateLang::It if starts("me") => 4,
            DateLang::It if starts("gi") => 5,
            DateLang::It if starts("ve") => 6,
            DateLang::It if starts("sa") => 7,

            DateLang::Nl if eq("zo") || eq("zondag") => 1,
            DateLang::Nl if eq("ma") || eq("maandag") => 2,
            DateLang::Nl if eq("di") || eq("dinsdag") => 3,
            DateLang::Nl if eq("wo") || eq("woensdag") => 4,
            DateLang::Nl if eq("do") || eq("donderdag") => 5,
            DateLang::Nl if eq("vr") || eq("vrijdag") => 6,
            DateLang::Nl if eq("za") || eq("zaterdag") => 7,

            DateLang::Pt if starts("dom") => 1,
            DateLang::Pt if starts("seg") => 2,
            DateLang::Pt if starts("ter") => 3,
            DateLang::Pt if starts("qua") => 4,
            DateLang::Pt if starts("qui") => 5,
            DateLang::Pt if starts("sex") => 6,
            DateLang::Pt if starts("sáb") => 7,

            DateLang::Ru if starts("суб") || starts("сб") => 7,
            DateLang::Ru if starts("вс") || starts("вос") => 1,
            DateLang::Ru if starts("пн") || starts("пон") => 2,
            DateLang::Ru if starts("вт") => 3,
            DateLang::Ru if starts("ср") => 4,
            DateLang::Ru if starts("чт") || starts("чет") => 5,
            DateLang::Ru if starts("пт") || starts("пят") => 6,

            DateLang::Pl if starts("pon") => 2,
            DateLang::Pl if starts("wt") => 3,
            DateLang::Pl if starts("śr") => 4,
            DateLang::Pl if starts("czw") => 5,
            DateLang::Pl if eq("pt") || starts("piątk") || eq("piątek") => 6,
            DateLang::Pl if starts("sob") => 7,
            DateLang::Pl if starts("niedz") => 1,

            DateLang::Uk if starts("по") || eq("пн") => 2,
            DateLang::Uk if starts("ві") || eq("вт") => 3,
            DateLang::Uk if starts("се") || eq("ср") => 4,
            DateLang::Uk if starts("че") || eq("чт") => 5,
            DateLang::Uk if starts("п'") || starts("п’") || eq("пт") => 6,
            DateLang::Uk if starts("су") || eq("сб") => 7,
            DateLang::Uk if starts("не") || eq("нд") => 1,

            DateLang::Sr if starts("по") => 2,
            DateLang::Sr if starts("ут") => 3,
            DateLang::Sr if starts("ср") => 4,
            DateLang::Sr if starts("че") => 5,
            DateLang::Sr if starts("пе") => 6,
            DateLang::Sr if starts("су") => 7,
            DateLang::Sr if starts("не") => 1,

            DateLang::Br if day.ends_with("sul") => 1,
            DateLang::Br if day.ends_with("lun") => 2,
            DateLang::Br if day.ends_with("meurzh") => 3,
            DateLang::Br if day.ends_with("merc’her") => 4,
            DateLang::Br if eq("yaou") || eq("diriaou") => 5,
            DateLang::Br if day.ends_with("gwener") => 6,
            DateLang::Br if day.ends_with("sadorn") => 7,

            DateLang::Eo if starts("dim") => 1,
            DateLang::Eo if starts("lun") => 2,
            DateLang::Eo if starts("mar") => 3,
            DateLang::Eo if starts("mer") => 4,
            DateLang::Eo if starts("ĵaŭ") || starts("jau") || starts("jhau") || starts("jxau") => 5,
            DateLang::Eo if starts("ven") => 6,
            DateLang::Eo if starts("sab") => 7,

            _ => return None,
        })
    }

    fn month_from_string(lang: DateLang, s: &str) -> Option<u32> {
        let mon = trim_special(&s.to_lowercase().replace('\u{00AD}', ""));
        let mon = mon.as_str();
        let starts = |p: &str| mon.starts_with(p);
        let eq = |p: &str| mon == p;
        Some(match lang {
            DateLang::En if starts("jan") => 1,
            DateLang::En if starts("feb") => 2,
            DateLang::En if starts("mar") => 3,
            DateLang::En if starts("apr") => 4,
            DateLang::En if starts("may") => 5,
            DateLang::En if starts("jun") => 6,
            DateLang::En if starts("jul") => 7,
            DateLang::En if starts("aug") => 8,
            DateLang::En if starts("sep") => 9,
            DateLang::En if starts("oct") => 10,
            DateLang::En if starts("nov") => 11,
            DateLang::En if starts("dec") => 12,

            DateLang::De if starts("jän") || starts("jan") => 1,
            DateLang::De if starts("feb") => 2,
            DateLang::De if starts("mär") => 3,
            DateLang::De if starts("apr") => 4,
            DateLang::De if starts("mai") => 5,
            DateLang::De if starts("jun") => 6,
            DateLang::De if starts("jul") => 7,
            DateLang::De if starts("aug") => 8,
            DateLang::De if starts("sep") => 9,
            DateLang::De if starts("okt") => 10,
            DateLang::De if starts("nov") => 11,
            DateLang::De if starts("dez") => 12,

            DateLang::Fr if starts("jan") => 1,
            DateLang::Fr if starts("fév") || starts("fev") => 2,
            DateLang::Fr if starts("mar") => 3,
            DateLang::Fr if starts("avr") => 4,
            DateLang::Fr if starts("mai") => 5,
            DateLang::Fr if starts("juin") => 6,
            DateLang::Fr if starts("juil") => 7,
            DateLang::Fr if starts("aou") || starts("aoû") => 8,
            DateLang::Fr if starts("sep") => 9,
            DateLang::Fr if starts("oct") => 10,
            DateLang::Fr if starts("nov") => 11,
            DateLang::Fr if starts("déc") || starts("dec") => 12,

            DateLang::Es if starts("en") => 1,
            DateLang::Es if starts("fe") => 2,
            DateLang::Es if starts("mar") || starts("mzo") => 3,
            DateLang::Es if starts("ab") => 4,
            DateLang::Es if starts("may") || starts("my") => 5,
            DateLang::Es if starts("jun") || eq("jn") => 6,
            DateLang::Es if starts("jul") || eq("jl") => 7,
            DateLang::Es if starts("ag") => 8,
            DateLang::Es if starts("se") || starts("sep") => 9,
            DateLang::Es if starts("oc") => 10,
            DateLang::Es if starts("no") => 11,
            DateLang::Es if starts("di") => 12,

            DateLang::Ca if starts("gen") => 1,
            DateLang::Ca if starts("febr") => 2,
            DateLang::Ca if starts("març") => 3,
            DateLang::Ca if starts("abr") => 4,
            DateLang::Ca if starts("maig") => 5,
            DateLang::Ca if starts("juny") => 6,
            DateLang::Ca if starts("jul") => 7,
            DateLang::Ca if starts("ag") => 8,
            DateLang::Ca if starts("set") => 9,
            DateLang::Ca if starts("oct") => 10,
            DateLang::Ca if starts("nov") => 11,
            DateLang::Ca if starts("des") => 12,

            DateLang::It if starts("gen") => 1,
            DateLang::It if starts("feb") => 2,
            DateLang::It if starts("mar") => 3,
            DateLang::It if starts("apr") => 4,
            DateLang::It if starts("mag") => 5,
            DateLang::It if starts("giu") => 6,
            DateLang::It if starts("lug") => 7,
            DateLang::It if starts("ago") => 8,
            DateLang::It if starts("set") => 9,
            DateLang::It if starts("ott") => 10,
            DateLang::It if starts("nov") => 11,
            DateLang::It if starts("dic") => 12,

            DateLang::Nl if starts("jan") => 1,
            DateLang::Nl if starts("feb") => 2,
            DateLang::Nl if starts("maa") || starts("mrt") || starts("mar") => 3,
            DateLang::Nl if starts("apr") => 4,
            DateLang::Nl if starts("mei") => 5,
            DateLang::Nl if starts("jun") => 6,
            DateLang::Nl if starts("jul") => 7,
            DateLang::Nl if starts("aug") => 8,
            DateLang::Nl if starts("sep") => 9,
            DateLang::Nl if starts("okt") || starts("oct") => 10,
            DateLang::Nl if starts("nov") => 11,
            DateLang::Nl if starts("dec") => 12,

            DateLang::Pt if starts("jan") => 1,
            DateLang::Pt if starts("fev") => 2,
            DateLang::Pt if starts("mar") => 3,
            DateLang::Pt if starts("abr") => 4,
            DateLang::Pt if starts("mai") => 5,
            DateLang::Pt if starts("jun") => 6,
            DateLang::Pt if starts("jul") => 7,
            DateLang::Pt if starts("ago") => 8,
            DateLang::Pt if starts("set") => 9,
            DateLang::Pt if starts("out") => 10,
            DateLang::Pt if starts("nov") => 11,
            DateLang::Pt if starts("dez") => 12,

            DateLang::Ru if starts("янв") => 1,
            DateLang::Ru if starts("фев") => 2,
            DateLang::Ru if starts("мар") => 3,
            DateLang::Ru if starts("апр") => 4,
            DateLang::Ru if starts("май") || starts("мая") => 5,
            DateLang::Ru if starts("июн") => 6,
            DateLang::Ru if starts("июл") => 7,
            DateLang::Ru if starts("авг") => 8,
            DateLang::Ru if starts("сен") => 9,
            DateLang::Ru if starts("окт") => 10,
            DateLang::Ru if starts("ноя") => 11,
            DateLang::Ru if starts("дек") => 12,

            DateLang::Pl if eq("stycznia") || eq("i") => 1,
            DateLang::Pl if eq("lutego") || eq("ii") => 2,
            DateLang::Pl if eq("marca") || eq("iii") => 3,
            DateLang::Pl if eq("kwietnia") || eq("iv") => 4,
            DateLang::Pl if eq("maja") || eq("v") => 5,
            DateLang::Pl if eq("czerwca") || eq("vi") => 6,
            DateLang::Pl if eq("lipca") || eq("vii") => 7,
            DateLang::Pl if eq("sierpnia") || eq("viii") => 8,
            DateLang::Pl if eq("września") || eq("ix") => 9,
            DateLang::Pl if eq("października") || eq("x") => 10,
            DateLang::Pl if eq("listopada") || eq("xi") => 11,
            DateLang::Pl if eq("grudnia") || eq("xii") => 12,

            DateLang::Uk if starts("сі") => 1,
            DateLang::Uk if starts("лю") => 2,
            DateLang::Uk if starts("бе") => 3,
            DateLang::Uk if starts("кв") => 4,
            DateLang::Uk if starts("тр") => 5,
            DateLang::Uk if starts("че") => 6,
            DateLang::Uk if starts("ли") => 7,
            DateLang::Uk if starts("се") => 8,
            DateLang::Uk if starts("ве") => 9,
            DateLang::Uk if starts("жо") => 10,
            DateLang::Uk if starts("гр") => 11,

            DateLang::Sr
                if mon == "јануар"
                    || mon == "i"
                    || mon == "јануара"
                    || mon == "јан" =>
            {
                1
            }
            DateLang::Sr
                if mon == "фебруар" || mon == "ii" || mon == "фебруара" || mon == "феб" =>
            {
                2
            }
            DateLang::Sr if mon == "март" || mon == "iii" || mon == "марта" || mon == "мар" => 3,
            DateLang::Sr if mon == "април" || mon == "iv" || mon == "априла" || mon == "апр" => 4,
            DateLang::Sr if mon == "мај" || mon == "v" || mon == "маја" => 5,
            DateLang::Sr if mon == "јун" || mon == "vi" || mon == "јуна" => 6,
            DateLang::Sr if mon == "јул" || mon == "vii" || mon == "јула" => 7,
            DateLang::Sr if mon == "август" || mon == "viii" || mon == "августа" || mon == "авг" => {
                8
            }
            DateLang::Sr
                if mon == "септембар" || mon == "ix" || mon == "септембра" || mon == "сеп" =>
            {
                9
            }
            DateLang::Sr
                if mon == "октобар" || mon == "x" || mon == "октобра" || mon == "окт" =>
            {
                10
            }
            DateLang::Sr
                if mon == "новембар" || mon == "xi" || mon == "новембра" || mon == "нов" =>
            {
                11
            }
            DateLang::Sr
                if mon == "децембар" || mon == "xii" || mon == "децембра" || mon == "дец" =>
            {
                12
            }

            DateLang::Br if mon == "genver" => 1,
            DateLang::Br if mon == "c’hwevrer" => 2,
            DateLang::Br if mon == "meurzh" => 3,
            DateLang::Br if mon == "ebrel" => 4,
            DateLang::Br if mon == "mae" => 5,
            DateLang::Br if mon == "mezheven" || mon == "even" => 6,
            DateLang::Br if mon == "gouere" || mon == "gouhere" => 7,
            DateLang::Br if mon == "eost" => 8,
            DateLang::Br if mon == "gwengolo" => 9,
            DateLang::Br if mon == "here" => 10,
            DateLang::Br if mon == "du" => 11,
            DateLang::Br if mon == "kerzu" => 12,

            DateLang::Eo if starts("jan") => 1,
            DateLang::Eo if starts("feb") => 2,
            DateLang::Eo if starts("mar") => 3,
            DateLang::Eo if starts("apr") => 4,
            DateLang::Eo if starts("maj") => 5,
            DateLang::Eo if starts("jun") => 6,
            DateLang::Eo if starts("jul") => 7,
            DateLang::Eo if starts("aŭg") => 8,
            DateLang::Eo if starts("sep") => 9,
            DateLang::Eo if starts("okt") => 10,
            DateLang::Eo if starts("nov") => 11,
            DateLang::Eo if starts("dec") => 12,

            _ => return None,
        })
    }

    /// Localized display name of a weekday (1=Sunday..7=Saturday).
    fn weekday_display(lang: DateLang, dow: u32) -> &'static str {
        const EN: [&str; 7] = [
            "Sunday",
            "Monday",
            "Tuesday",
            "Wednesday",
            "Thursday",
            "Friday",
            "Saturday",
        ];
        const DE: [&str; 7] = [
            "Sonntag",
            "Montag",
            "Dienstag",
            "Mittwoch",
            "Donnerstag",
            "Freitag",
            "Samstag",
        ];
        const FR: [&str; 7] = [
            "dimanche",
            "lundi",
            "mardi",
            "mercredi",
            "jeudi",
            "vendredi",
            "samedi",
        ];
        const ES: [&str; 7] = [
            "domingo",
            "lunes",
            "martes",
            "miércoles",
            "jueves",
            "viernes",
            "sábado",
        ];
        const CA: [&str; 7] = [
            "diumenge",
            "dilluns",
            "dimarts",
            "dimecres",
            "dijous",
            "divendres",
            "dissabte",
        ];
        const IT: [&str; 7] = [
            "domenica",
            "lunedì",
            "martedì",
            "mercoledì",
            "giovedì",
            "venerdì",
            "sabato",
        ];
        const NL: [&str; 7] = [
            "zondag",
            "maandag",
            "dinsdag",
            "woensdag",
            "donderdag",
            "vrijdag",
            "zaterdag",
        ];
        const PT: [&str; 7] = [
            "domingo",
            "segunda-feira",
            "terça-feira",
            "quarta-feira",
            "quinta-feira",
            "sexta-feira",
            "sábado",
        ];
        const RU: [&str; 7] = [
            "воскресенье",
            "понедельник",
            "вторник",
            "среда",
            "четверг",
            "пятница",
            "суббота",
        ];
        const PL: [&str; 7] = [
            "niedziela",
            "poniedziałek",
            "wtorek",
            "środa",
            "czwartek",
            "piątek",
            "sobota",
        ];
        const UK: [&str; 7] = [
            "неділя",
            "понеділок",
            "вівторок",
            "середа",
            "четвер",
            "п'ятниця",
            "субота",
        ];
        const SR: [&str; 7] = [
            "недеља",
            "понедељак",
            "уторак",
            "среда",
            "четвртак",
            "петак",
            "субота",
        ];
        const BR: [&str; 7] = [
            "Sul",
            "Lun",
            "Meurzh",
            "Merc’her",
            "Yaou",
            "Gwener",
            "Sadorn",
        ];
        const EO: [&str; 7] = [
            "dimanĉo",
            "lundo",
            "mardo",
            "merkredo",
            "jaŭdo",
            "vendredo",
            "sabato",
        ];
        let idx = dow.saturating_sub(1) as usize;
        match lang {
            DateLang::En => EN[idx],
            DateLang::De => DE[idx],
            DateLang::Fr => FR[idx],
            DateLang::Es => ES[idx],
            DateLang::Ca => CA[idx],
            DateLang::It => IT[idx],
            DateLang::Nl => NL[idx],
            DateLang::Pt => PT[idx],
            DateLang::Ru => RU[idx],
            DateLang::Pl => PL[idx],
            DateLang::Uk => UK[idx],
            DateLang::Sr => SR[idx],
            DateLang::Br => BR[idx],
            DateLang::Eo => EO[idx],
        }
    }

    /// Parses the day of the month: digits with optional suffix, or
    /// localized number words (br/eo).
    fn day_of_month(lang: DateLang, s: &str) -> Option<u32> {
        let s = s.replace('\u{00AD}', "");
        let digits: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
        if !digits.is_empty() {
            return digits.parse().ok();
        }
        let day = s.to_lowercase();
        let day = day.as_str();
        match lang {
            DateLang::Br => {
                let mut day = day.to_string();
                if day.starts_with('t') {
                    day.replace_range(0..1, "d");
                }
                if day.starts_with('p') {
                    day.replace_range(0..1, "b");
                }
                if day.ends_with("vet") {
                    let n = day.len() - 3;
                    day.truncate(n);
                }
                let day = day.as_str();
                Some(match day {
                    "c’hentañ" | "unan" => 1,
                    "daou" | "eil" => 2,
                    "dri" | "drede" | "deir" => 3,
                    "bevar" => 4,
                    "bemp" | "bem" => 5,
                    "c’hwerc’h" => 6,
                    "seizh" => 7,
                    "eizh" => 8,
                    "nav" | "na" => 9,
                    "dek" => 10,
                    "unnek" => 11,
                    "daouzek" => 12,
                    "drizek" => 13,
                    "bevarzek" => 14,
                    "bemzek" => 15,
                    "c’hwezek" => 16,
                    "seitek" => 17,
                    "driwec’h" => 18,
                    "naontek" => 19,
                    "ugent" => 20,
                    "dregont" => 30,
                    _ => 0,
                })
            }
            DateLang::Eo => {
                let mut day = day.to_string();
                if day.ends_with('n') {
                    day.truncate(day.len() - 1);
                }
                let mut n = 0u32;
                if day.starts_with("dek") {
                    n = 10;
                    day.replace_range(0..3, "");
                } else if day.starts_with("dudek") {
                    n = 20;
                    day.replace_range(0..5, "");
                } else if day.starts_with("tridek") {
                    n = 30;
                    day.replace_range(0..6, "");
                }
                if n > 0 && day.starts_with('-') {
                    day.replace_range(0..1, "");
                }
                let day = day.as_str();
                n += match day {
                    "unua" => 1,
                    "dua" => 2,
                    "tria" => 3,
                    "kvara" => 4,
                    "kvina" => 5,
                    "sesa" => 6,
                    "sepa" => 7,
                    "oka" => 8,
                    "naŭa" | "nauxa" | "naua" => 9,
                    _ => 0,
                };
                Some(n)
            }
            _ => Some(0),
        }
    }

    fn error_message_wrong_year(lang: DateLang) -> &'static str {
        match lang {
            DateLang::En => "This date is wrong. Did you mean \"{currentYear}\"?",
            DateLang::De => {
                "Dieses Datum stimmt nicht mit dem Tag überein. Meinten Sie \"{currentYear}\"?"
            }
            DateLang::Fr => {
                "Cette date est incorrecte. Faites-vous référence à l'année \"{currentYear}\" ?"
            }
            DateLang::Es => "Este fecha no es correcta. ¿Se refería al año \"{currentYear}\"?",
            DateLang::Ca => "Aquesta data no és correcta. ¿Us referiu a l'any \"{currentYear}\"?",
            DateLang::Nl => "Deze datum is onjuist. Bedoelt u misschien \"{currentYear}\"?",
            DateLang::Pt => {
                "Esta data está incorreta. Você está se referindo ao ano \"{currentYear}\"?"
            }
            _ => "This date is wrong. Did you mean \"{currentYear}\"?",
        }
    }

    fn adjust_suggestion_de(sugg: &str) -> String {
        let dot_comma = sugg.find(".,");
        if let Some(pos) = dot_comma {
            if pos > 5 && pos < 12 {
                return sugg.replace(".,", ",");
            }
        }
        if dot_comma.is_none() {
            if let Some(pos) = sugg.find(',') {
                if pos > 0 && pos < 5 {
                    return sugg.replace(",", ".,");
                }
            }
        }
        sugg.to_string()
    }

    fn day_str_like_original_en(day: &str, original: &str) -> String {
        if original.chars().all(|c| c.is_ascii_digit()) {
            return day.to_string();
        }
        let n: i32 = day.parse().unwrap_or(0);
        if (11..=13).contains(&n) {
            return format!("{}th", n);
        }
        match n % 10 {
            1 => format!("{}st", n),
            2 => format!("{}nd", n),
            3 => format!("{}rd", n),
            _ => format!("{}th", n),
        }
    }

    fn parse_month(lang: DateLang, s: &str) -> Option<u32> {
        if s.chars().all(|c| c.is_ascii_digit()) && !s.is_empty() {
            s.parse().ok()
        } else {
            month_from_string(lang, s)
        }
    }

    pub fn future_date(
        ctx: &mut FilterCtx,
        lang: DateLang,
        replacements: Vec<String>,
    ) -> Option<JavaOutcome> {
        let year_str = ctx.arg("year")?;
        if std::env::var("NLPRULE_DEBUG_FILTER").is_ok() {
            eprintln!(
                "FutureDateFilter: year={:?} month={:?} day={:?} matched_tokens={:?} span={:?}",
                year_str,
                ctx.arg("month"),
                ctx.arg("day"),
                ctx.matched_tokens,
                ctx.span
            );
        }
        let month_str = ctx.arg("month")?;
        let day_str = ctx.arg("day")?;

        let year: i32 = year_str.parse().ok()?;
        let month = parse_month(lang, &month_str)?;
        let day = day_of_month(lang, &day_str)?;

        if !valid_date(year, month, day) {
            return None;
        }
        let (ny, nm, nd) = now();
        let date_days = days_from_civil(year, month, day);
        let now_days = days_from_civil(ny, nm, nd);
        if date_days > now_days {
            keep(ctx, replacements)
        } else {
            None
        }
    }

    pub fn new_year_date(
        ctx: &mut FilterCtx,
        lang: DateLang,
        ymd: bool,
        replacements: Vec<String>,
    ) -> Option<JavaOutcome> {
        let (year_str, month_str, day_str) = if ymd {
            let date = ctx.arg("date")?;
            let mut parts = date.split('-');
            let y = parts.next()?.to_string();
            let m = parts.next()?.to_string();
            let d = parts.next()?.to_string();
            (Some(y), m, d)
        } else {
            (ctx.arg("year"), ctx.arg("month")?, ctx.arg("day")?)
        };

        let year: i32 = year_str.as_ref()?.parse().ok()?;
        let month = parse_month(lang, &month_str)?;
        let day = day_of_month(lang, &day_str)?;

        if !valid_date(year, month, day) {
            return None;
        }

        let (cur_year, cur_month, _) = now();
        if cur_month == 1 && month != 12 && year + 1 == cur_year {
            let mut message = ctx
                .message
                .replace("{year}", &year.to_string())
                .replace("{realYear}", &cur_year.to_string());
            if ymd {
                let real_date = format!("{}-{}-{}", year + 1, month_str, day_str);
                message = message.replace("{realDate}", &real_date);
            }
            Some(JavaOutcome {
                span: ctx.span.clone(),
                replacements,
                message: Some(message),
            })
        } else {
            None
        }
    }

    pub fn date_check(
        ctx: &mut FilterCtx,
        lang: DateLang,
        with_suggestions: bool,
        input: DateInput,
    ) -> Option<JavaOutcome> {
        if !with_suggestions {
            date_check_plain(ctx, lang, input)
        } else {
            date_check_with_suggestions(ctx, lang, input)
        }
    }

    fn date_check_plain(
        ctx: &mut FilterCtx,
        lang: DateLang,
        input: DateInput,
    ) -> Option<JavaOutcome> {
        let (day_str, month_str, year_str) = match input {
            DateInput::Separate => (
                ctx.arg("day")?,
                ctx.arg("month")?,
                ctx.arg("year"),
            ),
            DateInput::Dmy | DateInput::Ymd => {
                let date = ctx.arg("date")?;
                let mut iter = date.split('-');
                let a = iter.next()?.to_string();
                let b = iter.next()?.to_string();
                let c = iter.next()?.to_string();
                if let DateInput::Dmy = input {
                    (a, b, Some(c))
                } else {
                    (c, b, Some(a))
                }
            }
        };

        let weekday_str = ctx.arg("weekDay")?;
        let dow_str = weekday_from_string(lang, &weekday_str)?;
        let month = parse_month(lang, &month_str)?;
        let day = day_of_month(lang, &day_str)?;
        let year: i32 = match year_str {
            Some(y) => y.parse().ok()?,
            None => {
                if test_hack() {
                    2014
                } else {
                    now().0
                }
            }
        };

        if !valid_date(year, month, day) {
            return None;
        }

        let dow_date = day_of_week(year, month, day);
        if dow_str != dow_date {
            let current_year = if test_hack() { 2014 } else { now().0 };
            let message = ctx
                .message
                .replace("{realDay}", weekday_display(lang, dow_date))
                .replace("{day}", weekday_display(lang, dow_str))
                .replace("{currentYear}", &current_year.to_string());
            Some(JavaOutcome {
                span: ctx.span.clone(),
                replacements: std::mem::take(&mut ctx.replacements),
                message: Some(message),
            })
        } else {
            None
        }
    }

    fn date_check_with_suggestions(
        ctx: &mut FilterCtx,
        lang: DateLang,
        input: DateInput,
    ) -> Option<JavaOutcome> {
        let weekday_pos: usize = ctx.arg("weekDay")?.parse().ok()?;
        let date_pos: Option<usize> = ctx
            .arg("date")
            .and_then(|x| x.parse::<i64>().ok())
            .and_then(|x| if x >= 0 { Some(x as usize) } else { None });

        let (day_pos, month_pos, year_pos, day_str, month_str, year_str, full_date) =
            if let Some(dp) = date_pos {
                let token = ctx.token_at_matched(dp)?;
                let parts: Vec<&str> = token.split('-').collect();
                if parts.len() != 3 {
                    return None;
                }
                (
                    dp,
                    dp,
                    Some(dp),
                    parts[2].to_string(),
                    parts[1].to_string(),
                    Some(parts[0].to_string()),
                    true,
                )
            } else if let DateInput::Dmy = input {
                let date = ctx.arg("date")?;
                let mut iter = date.split('-');
                let d = iter.next()?.to_string();
                let m = iter.next()?.to_string();
                let y = iter.next()?.to_string();
                let dp: usize = ctx.args_get("date").and_then(|x| x.parse().ok())?;
                (dp, dp, Some(dp), d, m, Some(y), true)
            } else {
                let day_pos: usize = ctx.arg("day")?.parse().ok()?;
                let month_pos: usize = ctx.arg("month")?.parse().ok()?;
                let year_pos: Option<usize> = ctx
                    .arg("year")
                    .and_then(|x| x.parse::<i64>().ok())
                    .and_then(|x| if x >= 0 { Some(x as usize) } else { None });
                let day_str = ctx.token_at_matched(day_pos)?.to_string();
                let month_str = ctx.token_at_matched(month_pos)?.to_string();
                let year_str = match year_pos {
                    Some(yp) => Some(ctx.token_at_matched(yp)?.to_string()),
                    None => None,
                };
                (day_pos, month_pos, year_pos, day_str, month_str, year_str, false)
            };

        let weekday_str = ctx.token_at_matched(weekday_pos)?.to_string();
        let dow_str = weekday_from_string(lang, &weekday_str.replace('\u{00AD}', ""))?;

        let month = parse_month(lang, &month_str)?;
        let day = day_of_month(lang, &day_str.replace('\u{00AD}', ""))?;
        let year: i32 = match &year_str {
            Some(y) => y.parse().ok()?,
            None => {
                if test_hack() {
                    2014
                } else {
                    now().0
                }
            }
        };

        if !valid_date(year, month, day) {
            return None;
        }

        let dow_date = day_of_week(year, month, day);
        if dow_str == dow_date {
            return None;
        }

        let current_year = if test_hack() { 2014 } else { now().0 };

        // suggest changing the year (to the current year)
        if valid_date(current_year, month, day)
            && dow_str == day_of_week(current_year, month, day)
        {
            let message = error_message_wrong_year(lang)
                .replace("{currentYear}", &current_year.to_string());
            let span = ctx.token_span_at_matched(year_pos.unwrap_or(day_pos))?;
            let replacement = if full_date {
                format!("{}-{}-{}", current_year, month_str, day_str)
            } else {
                current_year.to_string()
            };
            return Some(JavaOutcome {
                span,
                replacements: vec![replacement],
                message: Some(message),
            });
        }

        // suggest changing day of week or day of month
        let message = ctx
            .message
            .clone()
            .replace("{realDay}", weekday_display(lang, dow_date))
            .replace("{day}", weekday_display(lang, dow_str))
            .replace("{currentYear}", &current_year.to_string());

        let (start_idx, end_idx) = if weekday_pos < day_pos {
            (weekday_pos, day_pos)
        } else {
            (day_pos, weekday_pos)
        };
        let span = ctx.matched_range_span(start_idx, end_idx)?;

        let mut suggestions = Vec::new();

        // suggest changing day of week
        let mut suggestion = String::new();
        for j in start_idx..=end_idx {
            if let Some(text) = ctx.token_at_matched(j) {
                if j > start_idx && ctx.token_has_space_before_at_matched(j) {
                    suggestion.push(' ');
                }
                if j == weekday_pos {
                    suggestion.push_str(&preserve_case(weekday_display(lang, dow_date), text));
                } else {
                    suggestion.push_str(text);
                }
            }
        }
        if !suggestion.is_empty() {
            suggestions.push(if lang == DateLang::De {
                adjust_suggestion_de(&suggestion)
            } else {
                suggestion
            });
        }

        // suggest changing day of month
        let mut corrected_day = String::new();
        let mut diff = 1u32;
        while diff < 7 {
            if day > diff
                && valid_date(year, month, day - diff)
                && dow_str == day_of_week(year, month, day - diff)
            {
                corrected_day = (day - diff).to_string();
                break;
            }
            if day + diff < 32
                && valid_date(year, month, day + diff)
                && dow_str == day_of_week(year, month, day + diff)
            {
                corrected_day = (day + diff).to_string();
                break;
            }
            diff += 1;
        }
        if !corrected_day.is_empty() {
            let mut suggestion = String::new();
            for j in start_idx..=end_idx {
                if let Some(text) = ctx.token_at_matched(j) {
                    if j > start_idx && ctx.token_has_space_before_at_matched(j) {
                        suggestion.push(' ');
                    }
                    if j == day_pos {
                        if full_date {
                            suggestion.push_str(&format!(
                                "{}-{}-{}",
                                year_str.clone().unwrap_or_default(),
                                month_str,
                                corrected_day
                            ));
                        } else if lang == DateLang::En {
                            suggestion
                                .push_str(&day_str_like_original_en(&corrected_day, &day_str));
                        } else {
                            suggestion.push_str(&corrected_day);
                        }
                    } else {
                        suggestion.push_str(text);
                    }
                }
            }
            if !suggestion.is_empty() {
                suggestions.push(if lang == DateLang::De {
                    adjust_suggestion_de(&suggestion)
                } else {
                    suggestion
                });
            }
        }

        Some(JavaOutcome {
            span,
            replacements: suggestions,
            message: Some(message),
        })
    }
}

// ---------------------------------------------------------------------------
// multitoken speller
// ---------------------------------------------------------------------------

mod multitoken {
    use super::*;

    const MAX_LENGTH_DIFF: usize = 3;

    pub fn apply(ctx: &mut FilterCtx) -> Option<JavaOutcome> {
        let suggester = ctx.data.multitoken.as_ref()?;
        if suggester.is_empty() {
            return None;
        }

        let underlined_error = ctx.matched_text.clone();
        if std::env::var("NLPRULE_DEBUG_FILTER").is_ok() {
            eprintln!("MultitokenSpellerFilter: error={:?} data={}", underlined_error, ctx.data.multitoken.is_some());
        }

        let all_accepted = ctx.data.multitoken_speller_check
            && !underlined_error
                .split_whitespace()
                .any(|w| !is_known_word_ctx(ctx, w));

        let replacements = get_suggestions(suggester, ctx, &underlined_error, all_accepted);
        if std::env::var("NLPRULE_DEBUG_FILTER").is_ok() {
            eprintln!("MultitokenSpellerFilter: suggestions={:?}", replacements);
        }
        if replacements.is_empty() {
            return None;
        }

        let replacements = if underlined_error.chars().count() > 4
            && is_all_uppercase(&underlined_error)
        {
            let mut out = Vec::new();
            for r in &replacements {
                let new = r.to_uppercase();
                if !out.contains(&new) && new != underlined_error {
                    out.push(new);
                }
            }
            out
        } else {
            let tokens = ctx.sentence_tokens();
            // LT's index 0 is the SENT_START pseudo-token; ours starts at 0
            let mut words_start = 0;
            while words_start < tokens.len()
                && (is_punct(tokens[words_start].word().as_str())
                    || !tokens[words_start]
                        .word()
                        .as_str()
                        .chars()
                        .any(char::is_alphabetic))
            {
                words_start += 1;
            }
            if ctx.pattern_token_pos() == words_start {
                let mut out = Vec::new();
                for r in &replacements {
                    let new = if r.chars().all(|c| !c.is_uppercase()) {
                        uppercase_first(r)
                    } else {
                        r.clone()
                    };
                    if !out.contains(&new) && new != underlined_error {
                        out.push(new);
                    }
                }
                out
            } else {
                let mut out = Vec::new();
                for r in &replacements {
                    if !out.contains(r) && r != &underlined_error {
                        out.push(r.clone());
                    }
                }
                out
            }
        };

        if replacements.is_empty() {
            return None;
        }

        keep(ctx, replacements)
    }

    fn is_punct(s: &str) -> bool {
        s.chars().count() == 1
            && s.chars()
                .next()
                .map(|c| c.is_ascii_punctuation() || c.is_whitespace())
                .unwrap_or(false)
    }

    /// Faithful port of `MultitokenSpeller.getSuggestions`.
    fn get_suggestions(
        suggester: &MultitokenSuggester,
        ctx: &FilterCtx,
        original_word: &str,
        are_tokens_accepted: bool,
    ) -> Vec<String> {
        let word = original_word.replace("- ", "-").replace(" -", "-");
        if discard_run_on_words(ctx, &word) {
            return Vec::new();
        }
        let normalized = MultitokenSuggester::normalize_key(&word);
        let mut weighted: Vec<(usize, String)> = Vec::new();

        let no_spaces_key = normalized.replace(' ', "");
        if let Some(candidates) = suggester.no_spaces.get(&no_spaces_key) {
            if stop_searching(candidates, original_word) {
                return Vec::new();
            }
            for candidate in candidates {
                weighted.push((0, candidate.clone()));
            }
        }

        if weighted.is_empty() {
            let first_char = normalized.chars().next().unwrap_or(' ');
            if let Some(bucket) = suggester.by_char.get(&first_char) {
                for (normalized_candidate, candidates) in bucket {
                    if stop_searching(candidates, original_word) {
                        return Vec::new();
                    }
                    if normalized_candidate.len().abs_diff(word.len()) > MAX_LENGTH_DIFF {
                        continue;
                    }
                    let candidate_parts: Vec<&str> =
                        normalized_candidate.split(' ').collect();
                    let word_parts: Vec<&str> = normalized.split(' ').collect();
                    let distances = distances_per_word(&candidate_parts, &word_parts);
                    let total: usize = distances.iter().sum();
                    if total < 1 {
                        for c in candidates {
                            weighted.push((0, c.clone()));
                        }
                        if weighted.len() == 2 {
                            break;
                        }
                        continue;
                    }
                    if normalized_candidate.chars().count() < 7 {
                        continue;
                    }
                    let mut exceeds = false;
                    for (i, &d) in distances.iter().enumerate() {
                        let max_d = if word_parts
                            .get(i)
                            .map(|w| w.chars().count() > 5)
                            .unwrap_or(false)
                            && candidate_parts
                                .get(i)
                                .map(|c| c.chars().count() > 4)
                                .unwrap_or(false)
                        {
                            2
                        } else {
                            1
                        };
                        if d > max_d {
                            exceeds = true;
                            break;
                        }
                    }
                    if exceeds {
                        continue;
                    }
                    if total <= max_edit_distance(normalized_candidate, &normalized) {
                        for c in candidates {
                            weighted.push((total, c.clone()));
                        }
                    }
                }
            }
        }

        if weighted.is_empty() {
            return Vec::new();
        }
        weighted.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
        let weight_first = weighted[0].0;
        if are_tokens_accepted && weighted[0].1.to_uppercase() == original_word {
            return Vec::new();
        }
        if are_tokens_accepted && weight_first > 1 {
            return Vec::new();
        }
        let mut results = Vec::new();
        for (weight, word) in weighted {
            if weight - weight_first < 1 && !results.contains(&word) {
                results.push(word);
            }
        }
        results
    }

    fn stop_searching(candidates: &[String], original: &str) -> bool {
        for c in candidates {
            if c == original {
                return true;
            }
        }
        for c in candidates {
            if *c == c.to_lowercase() && uppercase_first(&c.to_lowercase()) == *original {
                return true;
            }
        }
        false
    }

    fn is_anagram(a: &str, b: &str) -> bool {
        if a.chars().count() != b.chars().count() {
            return false;
        }
        let mut av: Vec<char> = a.chars().collect();
        let mut bv: Vec<char> = b.chars().collect();
        av.sort();
        bv.sort();
        av == bv
    }

    fn levenshtein_lang(s1: &str, s2: &str) -> usize {
        if s1.replace(' ', "") == s2.replace(' ', "") {
            return 0;
        }
        let mut distance = levenshtein(s1, s2);
        let ns1 = s1.replace('y', "i").replace("ko", "co").replace("ka", "ca");
        let ns2 = s2.replace('y', "i").replace("ko", "co").replace("ka", "ca");
        if s1 != ns1 || s2 != ns2 {
            distance = distance.min(levenshtein(&ns1, &ns2));
        }
        let anagram = is_anagram(s1, s2);
        if distance > 1 && anagram {
            distance -= 1;
        }
        if distance > 0 && s1.chars().count() == s2.chars().count() && anagram {
            distance = 1;
        }
        distance
    }

    fn distances_per_word(parts1: &[&str], parts2: &[&str]) -> Vec<usize> {
        if parts1.len() == parts2.len() && parts1.len() > 1 {
            parts1
                .iter()
                .zip(parts2.iter())
                .map(|(a, b)| levenshtein_lang(a, b))
                .collect()
        } else {
            let s1 = parts1.join(" ");
            let s2 = parts2.join(" ");
            vec![levenshtein_lang(&s1, &s2)]
        }
    }

    fn char_distance(a: char, b: char) -> f32 {
        if a == b {
            return 0.0;
        }
        if (a == 's' && b == 'z') || (a == 'z' && b == 's') {
            return 0.2;
        }
        if (a == 'b' && b == 'v') || (a == 'v' && b == 'b') {
            return 0.2;
        }
        if (a == 'i' && b == 'y') || (a == 'y' && b == 'i') {
            return 0.0;
        }
        1.0
    }

    fn max_edit_distance(candidate: &str, word: &str) -> usize {
        let total_length = word.chars().count();
        let correct_length = total_length.saturating_sub(number_of_correct_chars(candidate, word));
        let parts1: Vec<&str> = candidate.split(' ').collect();
        let parts2: Vec<&str> = word.split(' ').collect();
        let first_char_wrong = if parts1.len() == parts2.len() && parts1.len() == 2 {
            char_distance(
                parts1[0].chars().next().unwrap_or(' '),
                parts2[0].chars().next().unwrap_or(' '),
            ) + char_distance(
                parts1[1].chars().next().unwrap_or(' '),
                parts2[1].chars().next().unwrap_or(' '),
            )
        } else {
            0.0
        };
        if correct_length <= 7 {
            (2.0 - first_char_wrong) as usize
        } else {
            (2.0 + 0.25 * (correct_length - 7) as f32 - 0.6 * first_char_wrong) as usize
        }
    }

    fn number_of_correct_chars(s1: &str, s2: &str) -> usize {
        let parts1: Vec<&str> = s1.split(' ').collect();
        let parts2: Vec<&str> = s2.split(' ').collect();
        let mut correct = 0;
        if parts1.len() == parts2.len() && parts1.len() > 1 {
            for (a, b) in parts1.iter().zip(parts2.iter()) {
                if a == b {
                    correct += a.chars().count();
                }
            }
        }
        correct
    }

    fn discard_run_on_words(ctx: &FilterCtx, underlined_error: &str) -> bool {
        let parts: Vec<&str> = underlined_error.split(' ').collect();
        if parts.len() == 2 {
            if is_capitalized(parts[1]) {
                return false;
            }
            if parts[0].is_empty() || parts[1].is_empty() {
                return true;
            }
            let p0_chars: Vec<char> = parts[0].chars().collect();
            let p1_chars: Vec<char> = parts[1].chars().collect();
            if p0_chars.is_empty() || p1_chars.is_empty() {
                return true;
            }
            let sugg1a: String = p0_chars[..p0_chars.len() - 1].iter().collect();
            let sugg1b: String = format!(
                "{}{}",
                p0_chars[p0_chars.len() - 1],
                parts[1]
            );
            if is_known_word_ctx(ctx, &sugg1a) && is_known_word_ctx(ctx, &sugg1b) {
                return true;
            }
            let sugg2a: String = format!("{}{}", parts[0], p1_chars[0]);
            let sugg2b: String = p1_chars[1..].iter().collect();
            return is_known_word_ctx(ctx, &sugg2a) && is_known_word_ctx(ctx, &sugg2b);
        }
        false
    }
}

// ---------------------------------------------------------------------------
// suppress misspelled suggestions
// ---------------------------------------------------------------------------

fn suppress_misspelled(ctx: &mut FilterCtx, replacements: Vec<String>) -> Option<JavaOutcome> {
    let suppress_match = ctx
        .arg("suppressMatch")
        .map(|x| !x.eq_ignore_ascii_case("false"))
        .unwrap_or(true);
    let suppress_postag = ctx.arg("SuppressPostag");

    let mut new_replacements = Vec::new();
    for replacement in &replacements {
        if !is_misspelled_word(ctx, replacement) {
            match &suppress_postag {
                Some(regex) => {
                    let re = Regex::new(regex.clone());
                    let matches = ctx
                        .tagger
                        .get_tags_with_options(replacement, Some(false), Some(false))
                        .any(|data| matches_full(&re, data.pos().as_str()));
                    if !matches {
                        new_replacements.push(replacement.clone());
                    }
                }
                None => new_replacements.push(replacement.clone()),
            }
        }
    }

    if new_replacements.is_empty() && suppress_match {
        None
    } else {
        keep(ctx, new_replacements)
    }
}

fn is_misspelled_word(ctx: &FilterCtx, s: &str) -> bool {
    s.split_whitespace()
        .any(|token| !is_known_word_ctx(ctx, token))
}

// ---------------------------------------------------------------------------
// find suggestions
// ---------------------------------------------------------------------------

const MAX_SUGGESTIONS: usize = 10;

fn find_suggestions(ctx: &mut FilterCtx, variant: FindSuggVariant) -> Option<JavaOutcome> {
    let word_from = ctx.arg("wordFrom")?;
    let desired_postag = ctx.arg("desiredPostag")?;
    let priority_postag = ctx.arg("priorityPostag");
    let remove_regex = ctx.arg("removeSuggestionsRegexp");
    let suppress_match = ctx
        .arg("suppressMatch")
        .map(|x| x.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    let diacritics_mode = ctx.arg("Mode").map(|x| x == "diacritics").unwrap_or(false);

    let word: String = if word_from == "inmarker" {
        let mut w = ctx.matched_text.replace(' ', "");
        if variant == FindSuggVariant::Ca {
            w = replace_ela_geminada(&w);
        }
        w
    } else {
        let pos: usize = word_from.parse().ok()?;
        let tokens = ctx.matched_token_refs();
        tokens.get(pos.checked_sub(1)?)?.word().as_str().to_string()
    };

    let _ = (&desired_postag, &priority_postag, &remove_regex);
    let desired_re = ctx.regex_arg("desiredPostag")?;
    let priority_re = ctx.regex_arg("priorityPostag");
    let remove_re = ctx.regex_arg("removeSuggestionsRegexp");

    let is_word_capitalized = is_capitalized(&word);
    let is_word_allupper = is_all_uppercase(&word);

    // If the original word (tagged standalone) meets the requirement:
    // in diacritics mode, suppress the match.
    let original_matches = ctx
        .tagger
        .get_tags_with_options(&word, Some(false), Some(false))
        .any(|data| matches_full_regex(&desired_re, data.pos().as_str()));
    if original_matches && diacritics_mode {
        return None;
    }

    let mut replacements: Vec<String> = Vec::new();
    let mut replacements2: Vec<String> = Vec::new();
    let mut used_lemmas: Vec<String> = Vec::new();

    let suggestions = spelling_suggestions(ctx, variant, &word);
    if std::env::var("NLPRULE_DEBUG_FILTER").is_ok() {
        eprintln!(
            "FindSuggestions: word={:?} candidates={:?}",
            word,
            &suggestions.iter().take(15).cloned().collect::<Vec<_>>()
        );
    }

    for suggestion in suggestions {
        if replacements.len() >= 2 * MAX_SUGGESTIONS {
            break;
        }
        let cleaned = clean_suggestion(variant, &suggestion);
        let analyzed: Vec<(String, String)> = ctx
            .tagger
            .get_tags_with_options(&cleaned, Some(false), Some(false))
            .map(|data| {
                (
                    data.lemma().as_str().to_string(),
                    data.pos().as_str().to_string(),
                )
            })
            .collect();

        if is_suggestion_exception(variant, &analyzed) {
            continue;
        }

        let mut used = false;
        if suggestion != word {
            let matches = analyzed
                .iter()
                .any(|(_, pos)| matches_full_regex(&desired_re, pos));
            if matches
                && !replacements.contains(&suggestion)
                && !replacements.contains(&suggestion.to_lowercase())
                && (!diacritics_mode
                    || remove_diacritics(&suggestion).to_lowercase()
                        == remove_diacritics(&word).to_lowercase())
                && remove_re
                    .as_ref()
                    .map(|re| !matches_full_regex(re, &suggestion))
                    .unwrap_or(true)
            {
                let mut replacement = suggestion.clone();
                if is_word_allupper {
                    replacement = replacement.to_uppercase();
                }
                if is_word_capitalized {
                    replacement = uppercase_first(&replacement);
                }
                let has_priority = priority_re
                    .as_ref()
                    .map(|re| {
                        analyzed
                            .iter()
                            .any(|(_, pos)| matches_full_regex(re, pos))
                    })
                    .unwrap_or(false);
                if has_priority {
                    replacements.insert(0, replacement);
                } else {
                    replacements.push(replacement);
                }
                used = true;
            }
        }

        // try with the synthesizer
        if !used {
            if let Some(synth) = ctx.synth {
                if !synth.is_empty() {
                    let mut synth_forms: Vec<String> = Vec::new();
                    for (lemma, _) in &analyzed {
                        if used_lemmas.contains(lemma) {
                            continue;
                        }
                        used_lemmas.push(lemma.clone());
                        for form in synth.synthesize_regex(lemma, &desired_re) {
                            if !synth_forms.contains(&form) {
                                synth_forms.push(form);
                            }
                        }
                    }
                    for mut form in synth_forms {
                        if is_word_allupper {
                            form = form.to_uppercase();
                        }
                        if is_word_capitalized {
                            form = uppercase_first(&form);
                        }
                        replacements2.push(form);
                    }
                }
            }
        }
    }

    let match_contains_finished = ctx
        .replacements
        .iter()
        .any(|k| !k.to_lowercase().contains("{suggestion}"));

    if diacritics_mode && replacements.is_empty() && !match_contains_finished {
        return None;
    }
    if replacements.len() + replacements2.len() == 0 && suppress_match && !match_contains_finished {
        return None;
    }

    // build the definitive replacements
    let mut definitive: Vec<String> = Vec::new();
    let mut replacements_used = false;
    for s in &ctx.replacements {
        if s.contains("{suggestion}") || s.contains("{Suggestion}") || s.contains("{SUGGESTION}") {
            replacements_used = true;
            for s2 in &replacements {
                if definitive.len() >= MAX_SUGGESTIONS {
                    break;
                }
                let new = if s.contains("{suggestion}") {
                    s.replace("{suggestion}", s2)
                } else if s.contains("{Suggestion}") {
                    s.replace("{Suggestion}", &uppercase_first(s2))
                } else {
                    s.replace("{SUGGESTION}", &s2.to_uppercase())
                };
                if !definitive.contains(&new) {
                    definitive.push(new);
                }
            }
        } else if !definitive.contains(s) {
            definitive.push(s.clone());
        }
    }
    if !replacements_used {
        if replacements.is_empty() {
            replacements2.sort_by_key(|w| {
                let d = osa_distance(&word.to_lowercase(), &w.to_lowercase());
                if d > 4 {
                    8
                } else {
                    d
                }
            });
            for replacement in &replacements2 {
                if !replacements.contains(replacement) && !definitive.contains(replacement) {
                    replacements.push(replacement.clone());
                }
            }
        }
        for replacement in &replacements {
            if definitive.len() >= MAX_SUGGESTIONS {
                break;
            }
            if !definitive.contains(replacement) {
                definitive.push(replacement.clone());
            }
        }
    }

    definitive.retain(|x| x != &ctx.matched_text);
    definitive.dedup();

    if definitive.is_empty() {
        return None;
    }

    keep(ctx, definitive)
}

fn replace_ela_geminada(word: &str) -> String {
    let chars: Vec<char> = word.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if i + 2 < chars.len()
            && chars[i].to_lowercase().next() == Some('l')
            && chars[i + 2].to_lowercase().next() == Some('l')
            && matches!(
                chars[i + 1],
                '.' | '•' | '⋅' | '∙' | '×' | '\u{F0D7}' | '-'
            )
        {
            out.push(chars[i]);
            out.push('·');
            out.push(chars[i + 2]);
            i += 3;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

fn clean_suggestion(variant: FindSuggVariant, s: &str) -> String {
    if variant == FindSuggVariant::Fr {
        let s = strip_french_clitic(s);
        s.split(' ').next().unwrap_or("").to_string()
    } else {
        s.to_string()
    }
}

fn strip_french_clitic(s: &str) -> String {
    let mut chars = s.chars();
    if let (Some(first), Some('\'')) = (chars.next(), chars.clone().next()) {
        if "smntl".contains(first.to_ascii_lowercase()) {
            return chars.as_str().to_string();
        }
    }
    for clitic in [
        "nous", "vous", "le", "la", "les", "me", "te", "se", "leur", "en", "y",
    ] {
        let with_space = format!("{} ", clitic);
        if s.len() >= with_space.len()
            && s.get(..with_space.len())
                .map(|prefix| prefix.eq_ignore_ascii_case(&with_space))
                .unwrap_or(false)
        {
            return String::from_utf8_lossy(&s.as_bytes()[with_space.len()..]).into_owned();
        }
    }
    s.to_string()
}

fn is_suggestion_exception(variant: FindSuggVariant, analyzed: &[(String, String)]) -> bool {
    if variant == FindSuggVariant::Ca {
        const IGNORE: [&str; 4] = ["enterar", "sentar", "conseguir", "alcançar"];
        const ALLOW: [&str; 2] = ["enter", "sentir"];
        analyzed.iter().any(|(lemma, _)| IGNORE.contains(&lemma.as_str()))
            && !analyzed
                .iter()
                .any(|(lemma, _)| ALLOW.contains(&lemma.as_str()))
    } else {
        false
    }
}

/// `StringTools.makeWrong`: mangle the first available vowel so the speller
/// treats a dictionary word as misspelled and still suggests neighbors.
fn make_wrong(s: &str) -> String {
    for (plain, wrong) in [
        ('a', 'ä'),
        ('e', 'ë'),
        ('i', 'ï'),
        ('o', 'ö'),
        ('u', 'ù'),
    ] {
        if s.contains(plain) {
            return s.replace(plain, &wrong.to_string());
        }
    }
    s.to_string()
}

/// `getSpellingSuggestions`: near words from the speller dictionary.
fn spelling_suggestions(ctx: &FilterCtx, variant: FindSuggVariant, word: &str) -> Vec<String> {
    let speller = match ctx.data.speller.as_ref() {
        Some(s) if !s.is_empty() => s,
        _ => return Vec::new(),
    };

    if variant == FindSuggVariant::Fr {
        // LT tags the source token first: if it has readings, the word is
        // mangled so the speller rule still produces suggestions
        let is_tagged = ctx
            .tagger
            .get_tags_with_options(word, Some(false), Some(false))
            .next()
            .is_some();
        let base = if is_tagged {
            make_wrong(word)
        } else {
            word.to_string()
        };
        let mut words_to_check = vec![base.clone()];
        if let Some(stripped) = base.strip_suffix('s') {
            words_to_check.push(stripped.to_string());
        }
        if base.ends_with(['a', 'e', 'i', 'o', 'u', 'é']) {
            words_to_check.push(format!("{}s", base));
        }
        let mut out = Vec::new();
        for w in words_to_check {
            // the French speller rule uses edit distance 1
            out.extend(speller.find_similar_ignoring_diacritics(&w, 1));
        }
        out
    } else {
        speller.find_similar(word, 2)
    }
}

// ---------------------------------------------------------------------------
// advanced synthesizer
// ---------------------------------------------------------------------------

fn advanced_synthesizer(
    ctx: &mut FilterCtx,
    replacements: Vec<String>,
) -> Option<JavaOutcome> {
    let postag_select = ctx.arg("postagSelect")?;
    let lemma_select = ctx.arg("lemmaSelect")?;
    let postag_from_str = ctx.arg("postagFrom")?;
    let lemma_from_str = ctx.arg("lemmaFrom")?;
    let new_lemma = ctx.arg("newLemma").unwrap_or_default();
    let postag_replace = ctx.arg("postagReplace");

    let tokens: Vec<(Span, &str, bool, Vec<(String, String)>)> = {
        let raw = ctx.matched_token_refs();
        raw.iter()
            .map(|t| {
                (
                    t.span().clone(),
                    t.word().as_str(),
                    t.has_space_before(),
                    t.word()
                        .tags()
                        .iter()
                        .map(|d| {
                            (
                                d.lemma().as_str().to_string(),
                                d.pos().as_str().to_string(),
                            )
                        })
                        .collect(),
                )
            })
            .collect()
    };

    let resolve_pos = |spec: &str, span: &Span| -> Option<usize> {
        if spec.starts_with("marker") {
            let mut pos = 0;
            while pos < tokens.len() && tokens[pos].0.byte().start < span.byte().start {
                pos += 1;
            }
            pos += 1;
            if spec.len() > "marker".len() {
                let off: usize = spec.trim_start_matches("marker").parse().ok()?;
                pos += off;
            }
            Some(pos)
        } else {
            spec.parse().ok()
        }
    };

    let span = ctx.span.clone();
    let postag_from = resolve_pos(&postag_from_str, &span)?;
    let lemma_from = resolve_pos(&lemma_from_str, &span)?;
    if postag_from < 1 || postag_from > tokens.len() || lemma_from < 1 || lemma_from > tokens.len()
    {
        return Some(JavaOutcome {
            span,
            replacements,
            message: None,
        });
    }

    let _ = (&postag_select, &lemma_select);
    let lemma_re = ctx.regex_arg("lemmaSelect")?;
    let postag_re = ctx.regex_arg("postagSelect")?;

    let analyzed_of = |idx: usize| -> Vec<(String, String)> {
        tokens[idx - 1].3.clone()
    };

    let pick = |analyzed: &[(String, String)], re: &Regex| -> Option<(String, String)> {
        analyzed
            .iter()
            .find(|(_, pos)| re.is_match(pos))
            .cloned()
            .or_else(|| analyzed.first().cloned())
    };

    let lemma_analyzed = analyzed_of(lemma_from);
    let (original_lemma, original_postag) = pick(&lemma_analyzed, &lemma_re)?;
    let postag_analyzed = analyzed_of(postag_from);
    let (_, mut desired_postag) = pick(&postag_analyzed, &postag_re)?;

    let desired_lemma = if new_lemma.is_empty() {
        original_lemma.clone()
    } else {
        new_lemma
    };

    if let Some(replace) = &postag_replace {
        desired_postag = composite_postag(
            &lemma_re,
            &postag_re,
            &original_postag,
            &desired_postag,
            replace,
        );
    }

    let synth = ctx.synth?;
    let synth_re = Regex::new(desired_postag.clone());
    if synth_re.try_compile().is_err() {
        return Some(JavaOutcome {
            span,
            replacements,
            message: None,
        });
    }
    let forms = synth.synthesize_regex(&desired_lemma, &synth_re);
    if forms.is_empty() {
        // Java returns the original match unchanged
        return Some(JavaOutcome {
            span,
            replacements,
            message: None,
        });
    }

    let source_word = tokens[lemma_from - 1].1;
    let is_cap = is_capitalized(source_word);
    let is_upper = is_all_uppercase(source_word);

    let mut out: Vec<String> = Vec::new();
    let mut suggestion_used = false;
    for r in &replacements {
        for form in &forms {
            let mut form = form.clone();
            if is_cap {
                form = uppercase_first(&form);
            }
            if is_upper {
                form = form.to_uppercase();
            }
            if r.contains("{suggestion}")
                || r.contains("{Suggestion}")
                || r.contains("{SUGGESTION}")
            {
                suggestion_used = true;
            }
            let complete = r
                .replace("{suggestion}", &form)
                .replace("{Suggestion}", &uppercase_first(&form))
                .replace("{SUGGESTION}", &form.to_uppercase());
            if !out.contains(&complete) {
                out.push(complete);
            }
        }
    }
    if !suggestion_used {
        out.extend(forms.iter().cloned());
    }
    // adapt the case of suggestions to the matched text (Language.adaptSuggestion)
    let matched = ctx.matched_text.clone();
    let out: Vec<String> = out
        .into_iter()
        .map(|x| adapt_case_str(&x, &matched))
        .collect();

    Some(JavaOutcome {
        span,
        replacements: out,
        message: None,
    })
}

fn adapt_case_str(input: &str, sample: &str) -> String {
    let first_upper = sample
        .chars()
        .next()
        .map(|c| c.is_uppercase())
        .unwrap_or(false);
    if first_upper {
        let has_lower = sample.chars().any(|c| c.is_lowercase());
        let count = sample.chars().count();
        if count > 1 && !has_lower {
            input.to_uppercase()
        } else {
            uppercase_first(input)
        }
    } else {
        input.to_string()
    }
}

/// `getCompositePostag`: replace `\aN` / `\bN` group references.
fn composite_postag(
    lemma_re: &Regex,
    postag_re: &Regex,
    original_postag: &str,
    desired_postag: &str,
    postag_replace: &str,
) -> String {
    let mut result = postag_replace.to_string();
    if let (Some(caps_a), Some(caps_b)) = (
        lemma_re.captures(original_postag),
        postag_re.captures(desired_postag),
    ) {
        let groups_a = lemma_re.captures_len();
        for i in 1..groups_a {
            if let Some(g) = caps_a.get(i) {
                result = result.replace(&format!("\\a{}", i), g.as_str());
            }
        }
        let groups_b = postag_re.captures_len();
        for i in 1..groups_b {
            if let Some(g) = caps_b.get(i) {
                result = result.replace(&format!("\\b{}", i), g.as_str());
            }
        }
    }
    result
}

// ---------------------------------------------------------------------------
// partial pos tag
// ---------------------------------------------------------------------------

fn partial_pos_tag(ctx: &mut FilterCtx, replacements: Vec<String>) -> Option<JavaOutcome> {
    let no: usize = ctx.arg("no")?.parse().ok()?;
    let regexp = ctx.arg("regexp")?;
    let required = ctx.arg("postag_regexp")?;
    let negate_pos = ctx.args_contains("negate_pos");
    let two_groups = ctx.args_contains("two_groups_regexp");
    let prefix = ctx.arg("prefix").unwrap_or_default();
    let suffix = ctx.arg("suffix").unwrap_or_default();

    let tokens = ctx.matched_token_refs();
    let token = tokens.get(no.checked_sub(1)?)?;
    let token_str = format!("{}{}{}", prefix, token.word().as_str(), suffix);

    let re = ctx.regex_arg("regexp")?;
    let caps = re.captures(&token_str)?;

    let partial = if two_groups {
        format!(
            "{}{}",
            caps.get(1).map(|x| x.as_str()).unwrap_or(""),
            caps.get(2).map(|x| x.as_str()).unwrap_or("")
        )
    } else {
        caps.get(1).map(|x| x.as_str()).unwrap_or("").to_string()
    };

    let required_re = ctx.regex_arg("postag_regexp")?;

    let mut postag_count = 0;
    let mut any_match = false;
    let mut negated_reject = false;
    for data in ctx
        .tagger
        .get_tags_with_options(&partial, Some(false), Some(false))
    {
        let pos = data.pos().as_str();
        if pos != "UNKNOWN" {
            postag_count += 1;
            if required_re.is_match(pos) {
                if negate_pos {
                    negated_reject = true;
                    break;
                }
                any_match = true;
            }
        }
    }

    if negated_reject {
        return None;
    }
    if any_match || (negate_pos && postag_count > 0) {
        keep(ctx, replacements)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// add commas
// ---------------------------------------------------------------------------

fn add_commas(ctx: &mut FilterCtx) -> Option<JavaOutcome> {
    let suggest_semicolon = ctx
        .arg("suggestSemicolon")
        .map(|x| x.eq_ignore_ascii_case("true"))
        .unwrap_or(false);

    let tokens = ctx.sentence_tokens();
    let mut postag_from = 1;
    while postag_from < tokens.len()
        && tokens[postag_from].span().byte().start < ctx.span.byte().start
    {
        postag_from += 1;
    }
    let mut postag_to = postag_from;
    while postag_to < tokens.len() && tokens[postag_to].span().byte().end < ctx.span.byte().end {
        postag_to += 1;
    }

    let is_punct = |s: &str| {
        s.chars().count() == 1
            && s.chars()
                .next()
                .map(|c| c.is_ascii_punctuation())
                .unwrap_or(false)
    };
    let opening_quote =
        |s: &str| s.chars().count() == 1 && "«“\"‘'„¿¡".contains(s.chars().next().unwrap());

    let before_ok = postag_from == 1
        || is_punct(tokens[postag_from - 1].word().as_str())
        || is_capitalized(tokens[postag_from].word().as_str());
    let after_ok = postag_to + 1 <= tokens.len() - 1
        && is_punct(tokens[postag_to + 1].word().as_str())
        && !(tokens[postag_to + 1].has_space_before()
            && opening_quote(tokens[postag_to + 1].word().as_str()));

    if before_ok && after_ok {
        return None;
    }

    let matched_slice = ctx.sentence.slice(ctx.span.clone()).to_string();

    if suggest_semicolon && tokens[postag_from - 1].word().as_str() == "," && !after_ok {
        let span = Span::from_positions(
            tokens[postag_from - 1].span().start(),
            tokens[postag_to].span().end(),
        );
        Some(JavaOutcome {
            span,
            replacements: vec![
                format!("; {},", matched_slice),
                format!(", {},", matched_slice),
            ],
            message: None,
        })
    } else if before_ok && !after_ok {
        let span = Span::from_positions(tokens[postag_to].span().start(), ctx.span.end());
        Some(JavaOutcome {
            span,
            replacements: vec![format!("{},", tokens[postag_to].word().as_str())],
            message: None,
        })
    } else if !before_ok && after_ok {
        let mut start = tokens[postag_from].span().start();
        if tokens[postag_from].has_space_before() {
            start = Position {
                byte: start.byte.saturating_sub(1),
                char: start.char.saturating_sub(1),
            };
        }
        let span = Span::from_positions(start, tokens[postag_from].span().end());
        Some(JavaOutcome {
            span,
            replacements: vec![format!(", {}", tokens[postag_from].word().as_str())],
            message: None,
        })
    } else {
        let mut start = tokens[postag_from].span().start();
        if tokens[postag_from].has_space_before() {
            start = Position {
                byte: start.byte.saturating_sub(1),
                char: start.char.saturating_sub(1),
            };
        }
        let span = Span::from_positions(start, tokens[postag_to].span().end());
        Some(JavaOutcome {
            span,
            replacements: vec![format!(", {},", matched_slice)],
            message: None,
        })
    }
}

// ---------------------------------------------------------------------------
// convert to sentence case
// ---------------------------------------------------------------------------

fn convert_to_sentence_case(ctx: &mut FilterCtx) -> Option<JavaOutcome> {
    let tokens = ctx.sentence_tokens();
    let pattern_token_pos = ctx.pattern_token_pos();

    let mut replacement = String::new();
    let mut original = String::new();
    let mut first_done = false;

    for (i, token) in tokens.iter().enumerate().skip(pattern_token_pos) {
        if token.span().byte().start < ctx.span.byte().start
            || token.span().byte().end > ctx.span.byte().end
        {
            continue;
        }
        let token_str = token.word().as_str();
        let mut normalized = token_str.to_lowercase();

        if let Some(next) = tokens.get(i + 1) {
            if next.word().as_str() == "." {
                if normalized.chars().count() == 1 {
                    normalized = normalized.to_uppercase();
                } else if normalized == "corp" {
                    normalized = "Corp".to_string();
                }
            }
        }

        let token_capitalized = uppercase_first(&normalized);
        if !first_done
            && !is_punct_single(token_str)
            && !token_str.is_empty()
        {
            first_done = true;
            replacement.push_str(&token_capitalized);
            original.push_str(token_str);
        } else {
            if token.has_space_before() {
                replacement.push(' ');
                original.push(' ');
            }
            replacement.push_str(&normalized);
            original.push_str(token_str);
        }
    }

    if replacement == original {
        return None;
    }

    keep(ctx, vec![replacement])
}

fn is_punct_single(s: &str) -> bool {
    s.chars().count() == 1
        && s.chars()
            .next()
            .map(|c| c.is_ascii_punctuation())
            .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// confusion check / diacritics check
// ---------------------------------------------------------------------------

fn confusion_check(
    ctx: &mut FilterCtx,
    replacements: Vec<String>,
    diacritics: bool,
) -> Option<JavaOutcome> {
    let postag = ctx.arg("postag")?;
    let original_form = ctx.arg("form")?;
    let is_all_upper = is_all_uppercase(&original_form);
    let is_cap = is_capitalized(&original_form);
    let form = original_form.to_lowercase();

    let gendernumber_from = ctx.arg("gendernumberFrom");

    let mut desired: Option<&'static str> = None;
    if let Some(gn) = &gendernumber_from {
        let i: usize = gn.parse().ok()?;
        let tokens = ctx.matched_token_refs();
        let atr = tokens.get(i.checked_sub(1)?)?;
        let tag_matches = |re: &str| {
            let regex = Regex::new(re.to_string());
            atr.word()
                .tags()
                .iter()
                .any(|d| matches_full(&regex, d.pos().as_str()))
        };
        desired = if tag_matches("[NAPD].+MS.*|V.P..SM.") {
            Some("MS")
        } else if tag_matches("[NAPD].+MP.*|V.P..PM.") {
            Some("MP")
        } else if tag_matches("[NAPD].+FS.*|V.P..SF.") {
            Some("FS")
        } else if tag_matches("[NAPD].+FP.*|V.P..PF.") {
            Some("FP")
        } else if tag_matches("[NAPD].+CP.*|V.P..P.") {
            Some("CP")
        } else if tag_matches("[NAPD].+CS.*|V.P..S.") {
            Some("CS")
        } else {
            None
        };
    }

    let entry = ctx
        .data
        .confusion_pairs
        .as_ref()
        .and_then(|m| m.get(&form))?;

    let postag_re = Regex::new(postag.clone());
    if !entry.iter().any(|(_, pos)| matches_full(&postag_re, pos)) {
        return None;
    }

    let replacement = if let Some(gender) = desired {
        let first = entry.first()?;
        let pattern = match gender {
            "MS" => "NC[MC][SN]000|A..[MC][SN].|V.P..SM",
            "MP" => "NC[MC][PN]000|A..[MC][PN].|V.P..PM",
            "FS" => "NC[FC][SN]000|A..[FC][SN].|V.P..SF",
            "FP" => "NC[FC][PN]000|A..[FC][PN].|V.P..PF",
            "CP" => "NC[MFC][PN]000|A..[MFC][PN].|V.P..P.",
            "CS" => "NC[MFC][SN]000|A..[MFC][SN].|V.P..S.",
            _ => return None,
        };
        let pattern_re = Regex::new(pattern.to_string());
        if !matches_full(&pattern_re, &first.1) {
            return None;
        }
        first.0.clone()
    } else if gendernumber_from.is_none() {
        entry.first()?.0.clone()
    } else {
        return None;
    };

    let mut message = ctx.message.clone();
    let replaces_accent = crate::rule::filter_data::has_diacritics(&replacement)
        && !crate::rule::filter_data::has_diacritics(&form);
    if !replaces_accent {
        if diacritics {
            message = message.replace("s'escriu amb accent", "s'escriu d'una altra manera");
        } else {
            message = message.replace("se escribe con tilde", "se escribe de otra manera");
        }
    }

    let mut replacement = replacement;
    if is_all_upper {
        replacement = replacement.to_uppercase();
    }
    if is_cap {
        replacement = uppercase_first(&replacement);
    }

    let out: Vec<String> = replacements
        .iter()
        .map(|sugg| {
            sugg.replace("{suggestion}", &replacement)
                .replace("{Suggestion}", &uppercase_first(&replacement))
                .replace("{SUGGESTION}", &replacement.to_uppercase())
        })
        .collect();

    Some(JavaOutcome {
        span: ctx.span.clone(),
        replacements: out,
        message: Some(message),
    })
}

// ---------------------------------------------------------------------------
// pt: regular/irregular participle
// ---------------------------------------------------------------------------

fn regular_irregular_participle(
    ctx: &mut FilterCtx,
    replacements: Vec<String>,
) -> Option<JavaOutcome> {
    let direction = ctx.arg("direction")?;
    let synth = ctx.synth?;

    let tokens = ctx.sentence_tokens();
    let atr = tokens
        .iter()
        .find(|t| t.span().byte().start == ctx.span.byte().start)?;

    let has_vmp = atr
        .word()
        .tags()
        .iter()
        .any(|d| d.pos().as_str().starts_with("VMP"));
    if !has_vmp {
        return None;
    }

    let mut selected: Option<(String, String)> = None;
    for d in atr.word().tags() {
        let pos = d.pos().as_str();
        if pos.starts_with("VMP") {
            selected = Some((d.lemma().as_str().to_string(), pos.to_string()));
        }
    }
    let (lemma, mut desired) = selected?;

    if desired.ends_with('C') {
        desired = format!("{}[MC]", &desired[..desired.len() - 1]);
    } else {
        let last = desired.chars().last()?;
        desired = format!("{}[{}C]", &desired[..desired.len() - 1], last);
    }

    let re = Regex::new(desired.clone());
    let participles = synth.synthesize_regex(&lemma, &re);
    if participles.len() <= 1 {
        return None;
    }

    let is_regular = |p: &str| {
        let lp = p.to_lowercase();
        lp.ends_with("do") || lp.ends_with("dos") || lp.ends_with("da") || lp.ends_with("das")
    };

    let atr_text = atr.word().as_str();
    let replacement = if direction.eq_ignore_ascii_case("RegularToIrregular") && is_regular(atr_text)
    {
        if !is_regular(&participles[0]) {
            participles[0].clone()
        } else if !is_regular(&participles[1]) {
            participles[1].clone()
        } else {
            return None;
        }
    } else if direction.eq_ignore_ascii_case("IrregularToRegular") && !is_regular(atr_text) {
        if is_regular(&participles[0]) {
            participles[0].clone()
        } else if is_regular(&participles[1]) {
            participles[1].clone()
        } else {
            return None;
        }
    } else {
        return None;
    };

    let out: Vec<String> = replacements
        .iter()
        .map(|sugg| {
            sugg.replace("{suggestion}", &replacement)
                .replace("{Suggestion}", &uppercase_first(&replacement))
                .replace("{SUGGESTION}", &replacement.to_uppercase())
        })
        .collect();

    keep(ctx, out)
}
