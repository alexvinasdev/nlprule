//! Ports of LanguageTool's Java-only built-in rules that have no XML
//! representation. Currently: `UppercaseSentenceStartRule` (LT 6.5) and its
//! Ukrainian subclass `UkrainianUppercaseSentenceStartRule`.

use crate::types::{Sentence, Span, Suggestion};
use crate::utils::regex::Regex;
use once_cell::sync::Lazy;

/// Cross-sentence context of the text-level built-ins, mirroring the state
/// `UppercaseSentenceStartRule` carries while iterating over the sentences of
/// a text (`lastParagraphString`, `isPrevSentenceNumberedList`).
pub(crate) struct TextRuleContext {
    /// This sentence is the only sentence of the checked text
    /// (LT skips single-word one-sentence texts).
    pub is_only_sentence: bool,
    /// The last significant token of the previous sentence, if any.
    pub prev_last_token: Option<String>,
    /// The previous sentence looked like a numbered list item ("1. foo").
    pub prev_numbered_list: bool,
}

impl Default for TextRuleContext {
    fn default() -> Self {
        TextRuleContext {
            is_only_sentence: false,
            prev_last_token: None,
            prev_numbered_list: false,
        }
    }
}

// Java's Pattern.matches() is a full match; the anchors keep onig equivalent.
static NUMERALS_EN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\A(?:[a-z]|m{0,4}(?:c[md]|d?c{0,3})(?:x[cl]|l?x{0,3})(?:i[xv]|v?i{0,3}))\z".to_string())
});
static ONLY_LOWERCASE_START: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\A[a-z][A-Z].*\z".to_string()));
static CAMEL_CASE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\A[a-z]+[A-Z][A-Za-z]+\z".to_string()));
static NO_PROTOCOL_URL: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"\A(?:[a-zA-Z0-9][a-zA-Z0-9-]+\.)?[a-zA-Z0-9][a-zA-Z0-9-]+\.[a-zA-Z0-9][a-zA-Z0-9-]+/.*\z".to_string(),
    )
});
static E_MAIL: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r#"(?<!:)\A@?\b[a-zA-Z0-9.!#$%&'*+/=?^_`{|}~-]+@((\[[0-9]{1,3}\.[0-9]{1,3}\.[0-9]{1,3}\.[0-9]{1,3}\])|(([a-zA-Z\-0-9]+\.)+[a-zA-Z]{2,}))\b\z"#.to_string(),
    )
});
static DIGIT_DOT: Lazy<Regex> = Lazy::new(|| Regex::new(r"\A\d+\. .*\z".to_string()));
static LINEBREAK_DIGIT_DOT: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\A.*\n\d+\. \z".to_string()));

static EXCEPTIONS: Lazy<[&'static str; 7]> =
    Lazy::new(|| ["n", "w", "x86", "ⓒ", "ø", "cc", "pH"]);

/// Whether the text starts with "1. " (numbered list detection for the next sentence).
pub(crate) fn is_numbered_list_item(sentence_text: &str) -> bool {
    DIGIT_DOT.is_match(sentence_text) || LINEBREAK_DIGIT_DOT.is_match(sentence_text)
}

/// The last significant (non trailing whitespace/quote) token of a sentence,
/// as `UppercaseSentenceStartRule` tracks between sentences.
pub(crate) fn last_significant_token(sentence: &Sentence) -> Option<String> {
    let tokens: Vec<_> = sentence.iter().collect();
    if tokens.is_empty() {
        return None;
    }
    let last = tokens[tokens.len() - 1].word().as_str().to_string();
    if is_whitespace_or_quote(&last) && tokens.len() >= 2 {
        Some(tokens[tokens.len() - 2].word().as_str().to_string())
    } else {
        Some(last)
    }
}

fn is_whitespace_or_quote(token: &str) -> bool {
    // Java WHITESPACE_OR_QUOTE is a single-char class used with matches()
    let mut chars = token.chars();
    match chars.next() {
        Some(c) => chars.next().is_none() && is_ws_or_quote_char(c),
        None => false,
    }
}

fn is_ws_or_quote_char(c: char) -> bool {
    matches!(
        c,
        ' ' | '"' | '\'' | '„' | '«' | '»' | '‘' | '’' | '“' | '”' | '\n'
    )
}

fn is_quote_start(token: &str, lang: Option<&str>) -> bool {
    let base = [
        "\"", "'", "„", "»", "«", "“", "‘", "¡", "¿",
    ];
    if lang == Some("pt") {
        // pt-BR uses dashes to introduce dialogue
        matches!(token, "\"" | "'" | "„" | "»" | "«" | "“" | "‘" | "¡" | "¿" | "-" | "–" | "—")
    } else {
        base.contains(&token)
    }
}

fn is_dutch_initial(token: &str) -> bool {
    matches!(token, "k" | "m" | "n" | "r" | "s" | "t")
}

fn is_url(token: &str) -> bool {
    for protocol in ["http", "https", "ftp"] {
        if token.starts_with(&format!("{protocol}://")) {
            return true;
        }
    }
    token.starts_with("www.") || NO_PROTOCOL_URL.is_match(token)
}

fn is_email(token: &str) -> bool {
    E_MAIL.is_match(token)
}

fn contains_digit(token: &str) -> bool {
    token.chars().any(|c| c.is_ascii_digit())
}

fn is_ukrainian_list_letter(token: &str) -> bool {
    let mut chars = token.chars();
    match chars.next() {
        Some(c) => {
            chars.next().is_none() && matches!(c, 'а'..='я' | 'і' | 'ї' | 'є' | 'ґ')
        }
        None => false,
    }
}

/// Port of `StringTools.uppercaseFirstChar` (the one-argument version; LT's
/// rule does not use the Dutch IJ variant).
fn uppercase_first_char(s: &str) -> String {
    if s.is_empty() {
        return s.to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    if chars.len() == 1 {
        // Java: str.toUpperCase(Locale.ENGLISH) - a full string uppercasing
        return chars[0].to_uppercase().collect();
    }
    let len = chars.len() - 1;
    let mut pos = 0;
    while len > pos && !chars[pos].is_alphanumeric() {
        pos += 1;
    }
    let first = chars[pos];
    // Java's Character.toUpperCase is a single-character mapping: characters
    // whose uppercase form is multi-char (e.g. ß -> SS) are returned unchanged
    let upper = {
        let mut it = first.to_uppercase();
        let len = it.len();
        match (len, it.next()) {
            (1, Some(c)) => c,
            _ => first,
        }
    };
    let mut out: String = chars[..pos].iter().collect();
    out.push(upper);
    out.extend(chars[pos + 1..].iter());
    out
}

/// Port of `UppercaseSentenceStartRule.match` for a single sentence with the
/// text-level state of the previous sentence.
pub(crate) fn uppercase_sentence_start(
    sentence: &Sentence,
    lang: Option<&str>,
    ctx: &TextRuleContext,
) -> Vec<Suggestion> {
    let tokens: Vec<_> = sentence.iter().collect();
    // LT bails out if there is no real token next to SENT_START
    if tokens.is_empty() {
        return Vec::new();
    }
    // Not useful to complain about a single-word single-sentence text
    if ctx.is_only_sentence && tokens.len() == 1 {
        return Vec::new();
    }

    let first = tokens[0].word().as_str();
    let second = tokens.get(1).map(|t| t.word().as_str());

    // real-token index LT checks (its matchTokenPos minus the SENT_START token)
    let mut idx = 0usize;
    // ignore leading quote characters (tokens.length >= 3 there => 2 real tokens)
    if tokens.len() >= 2 && is_quote_start(first, lang) {
        idx = 1;
    }
    // 't kofschip-style Dutch contractions: "'k heb..." -> check the third token
    if lang == Some("nl")
        && tokens.len() > 2
        && first == "'"
        && second.map_or(false, is_dutch_initial)
    {
        idx = 2;
    }

    let check_token = tokens[idx].word().as_str();

    // UkrainianUppercaseSentenceStartRule: lists like "а) б) в)"
    if lang == Some("uk") && idx == 0 && tokens.len() > 1 {
        if is_ukrainian_list_letter(check_token) && tokens[1].word().as_str() == ")" {
            return Vec::new();
        }
    }

    let mut prevent_error = false;
    if ctx.prev_last_token.as_deref() == Some(",") || ctx.prev_last_token.as_deref() == Some(";") {
        prevent_error = true;
    }
    if contains_digit(check_token) {
        prevent_error = true;
    }
    // allow lowercase enumerations like "a)" and "iv."
    if idx + 1 < tokens.len() {
        let next = tokens[idx + 1].word().as_str();
        if NUMERALS_EN.is_match(check_token) && (next == "." || next == ")") {
            prevent_error = true;
        }
    }
    if ctx.prev_numbered_list || is_url(check_token) || is_email(check_token) {
        prevent_error = true;
    }

    if !check_token.is_empty() {
        let first_char = check_token.chars().next().unwrap();
        let capitalized = uppercase_first_char(check_token);
        if capitalized != check_token
            && !prevent_error
            && first_char.is_lowercase()
            && !ONLY_LOWERCASE_START.is_match(check_token)
            && !EXCEPTIONS.contains(&check_token)
            && !CAMEL_CASE.is_match(check_token)
        {
            let span = Span::from_positions(tokens[idx].span().start(), tokens[idx].span().end());
            return vec![Suggestion::new(
                "UPPERCASE_SENTENCE_START".to_string(),
                "This sentence does not start with an uppercase letter.".to_string(),
                span,
                vec![capitalized],
            )];
        }
    }

    Vec::new()
}

// ---------------------------------------------------------------------------
// MorfologikSpellerRule / SpellingCheckRule port (LT 6.5)
// ---------------------------------------------------------------------------

/// A word list embedded from `configs/speller/<lang>/`.
fn wordlist(text: &'static str) -> Vec<&'static str> {
    text.lines()
        .map(|l| l.split('#').next().unwrap_or("").trim())
        .filter(|l| !l.is_empty())
        .collect()
}

macro_rules! spelling_lists {
    ($lang:literal, $file:literal) => {
        static_ref! {
            Vec<&'static str> = wordlist(include_str!(concat!("./speller_data/", $lang, "/", $file)))
        }
    };
    ($lang:literal, $file:literal, $($rest:literal),+) => {
        static_ref! {
            Vec<&'static str> = wordlist(concat!(
                include_str!(concat!("./speller_data/", $lang, "/", $file)),
                "\n",
                $(include_str!(concat!("./speller_data/", $lang, "/", $rest)), "\n"),+
            ))
        }
    };
}

// include_str with a trailing "\n" appended so concatenated files don't bleed
// their last word together; `wordlist` strips it again via line splitting.
macro_rules! static_ref {
    ($ty:ty = $expr:expr) => {{
        static LIST: Lazy<$ty> = Lazy::new(|| $expr);
        &*LIST
    }};
}

struct SpellingRuleConfig {
    id: &'static str,
    latin_script: bool,
    /// wordsToBeIgnored: ignore.txt + spelling.txt + spelling_custom.txt
    ignore: &'static [&'static str],
    /// wordsToBeProhibited: prohibit.txt + prohibit_custom.txt
    prohibit: &'static [&'static str],
}

fn spelling_config(lang: &str) -> Option<SpellingRuleConfig> {
    Some(match lang {
        "es" => SpellingRuleConfig {
            id: "MORFOLOGIK_RULE_ES",
            latin_script: true,
            ignore: spelling_lists!("es", "ignore.txt", "spelling.txt"),
            prohibit: spelling_lists!("es", "prohibit.txt"),
        },
        "ca" => SpellingRuleConfig {
            id: "MORFOLOGIK_RULE_CA_ES",
            latin_script: true,
            ignore: spelling_lists!("ca", "ignore.txt"),
            prohibit: spelling_lists!("ca", "prohibit.txt"),
        },
        "fr" => SpellingRuleConfig {
            id: "FR_SPELLING_RULE",
            latin_script: true,
            ignore: spelling_lists!("fr", "ignore.txt", "spelling.txt", "spelling_custom.txt"),
            prohibit: spelling_lists!("fr", "prohibit.txt"),
        },
        "nl" => SpellingRuleConfig {
            id: "MORFOLOGIK_RULE_NL_NL",
            latin_script: true,
            ignore: &[],
            prohibit: &[],
        },
        "de" => SpellingRuleConfig {
            id: "MORFOLOGIK_RULE_DE_DE",
            latin_script: true,
            ignore: spelling_lists!("de", "ignore.txt", "spelling.txt", "spelling_custom.txt"),
            prohibit: spelling_lists!("de", "prohibit.txt", "prohibit_custom.txt"),
        },
        "en" => SpellingRuleConfig {
            id: "MORFOLOGIK_RULE_EN_US",
            latin_script: true,
            ignore: spelling_lists!(
                "en",
                "ignore.txt",
                "spelling.txt",
                "spelling_custom.txt",
                "spelling_en-US.txt"
            ),
            prohibit: spelling_lists!("en", "prohibit.txt", "prohibit_custom.txt"),
        },
        "pt" => SpellingRuleConfig {
            id: "MORFOLOGIK_RULE_PT",
            latin_script: true,
            ignore: &[],
            prohibit: &[],
        },
        "ru" => SpellingRuleConfig {
            id: "MORFOLOGIK_RULE_RU_RU",
            latin_script: false,
            ignore: spelling_lists!("ru", "ignore.txt", "spelling.txt"),
            prohibit: spelling_lists!("ru", "prohibit.txt"),
        },
        "uk" => SpellingRuleConfig {
            id: "MORFOLOGIK_RULE_UK_UA",
            latin_script: false,
            ignore: spelling_lists!("uk", "ignore.txt", "spelling.txt"),
            prohibit: &[],
        },
        _ => return None,
    })
}

/// Port of `SpellingCheckRule.ignoreWord` + `MorfologikSpellerRule.canBeIgnored`.
fn spelling_ignored(word: &str, config: &SpellingRuleConfig) -> bool {
    if word.chars().count() > 200 {
        return true;
    }
    // Tokens with no letters cannot have spelling errors
    let has_letter = if config.latin_script {
        word.chars().any(|c| c.is_alphabetic() && is_latin(c))
    } else {
        word.chars().any(char::is_alphabetic)
    };
    if !has_letter {
        return true;
    }
    let is_ignored = |w: &str| config.ignore.contains(&w);
    if word.ends_with('.') && !is_ignored(word) {
        return is_ignored(word.strip_suffix('.').unwrap_or(word));
    }
    is_ignored(word)
}

fn is_latin(c: char) -> bool {
    // approximate Java's \p{script=latin}
    let cp = c as u32;
    (0x41..=0x5A).contains(&cp)
        || (0x61..=0x7A).contains(&cp)
        || ((0xC0..=0xFF).contains(&cp) && cp != 0xD7 && cp != 0xF7)
        || (0x100..=0x24F).contains(&cp)
        || (0x1E00..=0x1EFF).contains(&cp)
}

/// Port of `MorfologikSpellerRule.match` (simplified: plain whole-token lookup,
/// no wrong-split detection, no compound handling).
pub(crate) fn morfologik_spelling(
    sentence: &Sentence,
    lang: Option<&str>,
    speller: Option<&crate::rule::filter_data::SpellerDict>,
) -> Vec<Suggestion> {
    let speller = match speller {
        Some(s) if !s.is_empty() => s,
        _ => return Vec::new(),
    };
    let lang = match lang {
        Some(l) => l,
        None => return Vec::new(),
    };
    let config = match spelling_config(lang) {
        Some(c) => c,
        None => return Vec::new(),
    };

    let tokens: Vec<_> = sentence.iter().collect();
    let mut out = Vec::new();

    for (idx, token) in tokens.iter().enumerate() {
        let word = token.word().as_str();
        if word.is_empty()
            || is_url(word)
            || is_email(word)
            || spelling_ignored(word, &config)
        {
            continue;
        }
        let prohibited = config.prohibit.contains(&word);
        if speller.is_known(word) && !prohibited {
            continue;
        }
        let span = Span::from_positions(token.span().start(), token.span().end());

        // suggestion candidates by increasing edit distance, mirroring
        // speller1/2/3 tiers of MorfologikSpellerRule.calcSpellerSuggestions
        let char_len = word.chars().count();
        let max_distance = if char_len >= 5 {
            3
        } else if char_len >= 3 {
            2
        } else {
            1
        };
        let mut replacements: Vec<String> = Vec::new();
        for candidate in speller.find_similar(word, max_distance) {
            if config.prohibit.contains(&candidate.as_str()) {
                continue;
            }
            if !replacements.contains(&candidate) {
                replacements.push(candidate);
            }
        }
        // Capitalize the word at the sentence start (if it is not the last
        // token), like MorfologikSpellerRule does
        let is_sentence_start = idx == 0 && idx < tokens.len() - 1;
        if is_sentence_start {
            for r in replacements.iter_mut() {
                if *r == r.to_lowercase() {
                    let capitalized = uppercase_first_char(r);
                    if capitalized != *r {
                        *r = capitalized;
                    }
                }
            }
        }

        out.push(Suggestion::new(
            config.id.to_string(),
            "Possible spelling mistake found.".to_string(),
            span,
            replacements,
        ));
    }

    out
}
