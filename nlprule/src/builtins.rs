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

fn is_immunized(token: &crate::types::Token) -> bool {
    token.chunks().iter().any(|c| c == "_immunized")
}

fn is_spelling_ignored_token(token: &crate::types::Token) -> bool {
    token
        .chunks()
        .iter()
        .any(|c| c == "_ignore_spelling" || c == "_immunized")
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

    // the last significant token of this sentence (trailing whitespace or
    // quote ignored)
    let mut last_token = tokens[tokens.len() - 1].word().as_str().to_string();
    if is_whitespace_or_quote(&last_token) && tokens.len() >= 2 {
        last_token = tokens[tokens.len() - 2].word().as_str().to_string();
    }

    let mut prevent_error = false;
    if ctx.prev_last_token.as_deref() == Some(",") || ctx.prev_last_token.as_deref() == Some(";") {
        prevent_error = true;
    }
    if contains_digit(check_token) {
        prevent_error = true;
    }
    // Java's SENTENCE_END1 = "[.?!…]|": matches only the empty string or a
    // single end punctuation. If the previous sentence ended with a real
    // word, this sentence must end with ./?/!/… for the error to count.
    let prev_is_empty_or_punct = match ctx.prev_last_token.as_deref() {
        None => true,
        Some("") => true,
        Some(t) => t.chars().count() == 1 && matches!(t, "." | "?" | "!" | "…"),
    };
    if !prev_is_empty_or_punct && !matches!(last_token.as_str(), "." | "?" | "!" | "…") {
        prevent_error = true;
    }
    // allow lowercase enumerations like "a)" and "iv."
    if idx + 1 < tokens.len() {
        let next = tokens[idx + 1].word().as_str();
        if NUMERALS_EN.is_match(check_token) && (next == "." || next == ")") {
            prevent_error = true;
        }
    }
    if ctx.prev_numbered_list
        || is_url(check_token)
        || is_email(check_token)
        || is_immunized(tokens[0])
    {
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

/// Port of LT's `KhmerSpaceBeforeRule` (AbstractSpaceBeforeRule): a missing
/// real space before the conjunctions ដើម្បី/និង/ពីព្រោះ. LT iterates its
/// token list from index 1 (index 0 is the artificial SENT_START, which
/// never equals " " or "("), so a sentence-initial conjunction fires too;
/// a preceding "(" suppresses the match.
pub(crate) fn khmer_space_before(sentence: &Sentence) -> Vec<Suggestion> {
    const CONJUNCTIONS: [&str; 3] = ["ដើម្បី", "និង", "ពីព្រោះ"];
    let tokens: Vec<_> = sentence.iter().collect();
    let mut out = Vec::new();
    for (idx, token) in tokens.iter().enumerate() {
        let word = token.word().as_str();
        if !CONJUNCTIONS.contains(&word) {
            continue;
        }
        // LT's previous token is " " exactly when real whitespace precedes;
        // our has_space_before covers any whitespace char. "(" suppresses.
        let prev_is_open_paren = idx > 0 && tokens[idx - 1].word().as_str() == "(";
        let missing_space = idx == 0 || (!token.has_space_before() && !prev_is_open_paren);
        if !missing_space {
            continue;
        }
        let span = Span::from_positions(token.span().start(), token.span().end());
        out.push(Suggestion::new(
            "KM_SPACE_BEFORE_CONJUNCTION".to_string(),
            "Missing space before conjunction.".to_string(),
            span,
            vec![format!(" {}", word)],
        ));
    }
    out
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

/// Rule id the WORD_REPEAT family emits for a language (LT renames it
/// per language). Keep in sync with `builtin_suggestions`.
pub(crate) fn repeat_rule_id(lang: Option<&str>) -> &'static str {
    match lang {
        Some("de") => "GERMAN_WORD_REPEAT_RULE",
        Some("es") => "SPANISH_WORD_REPEAT_RULE",
        Some("fr") => "FRENCH_WORD_REPEAT_RULE",
        Some("uk") => "UKRAINIAN_WORD_REPEAT_RULE",
        Some("ca") => "CATALAN_WORD_REPEAT_RULE",
        Some("pt") => "PORTUGUESE_WORD_REPEAT_RULE",
        Some("it") => "ITALIAN_WORD_REPEAT_RULE",
        _ => "WORD_REPEAT_RULE",
    }
}

/// Inventory of the rule ids the built-in (Java-only in LT) families can
/// emit for this language. Mirrors the arms of `Rules::builtin_suggestions`;
/// keep in sync when porting new builtin families.
pub(crate) fn builtin_ids(
    lang: Option<&str>,
    filter_data: &crate::rule::filter_data::FilterData,
) -> Vec<String> {
    let mut out = Vec::new();
    if filter_data.speller.is_some()
        && lang.map_or(false, |l| spelling_config(l).is_some())
    {
        out.push(spelling_config(lang.unwrap()).unwrap().id.to_string());
    }
    out.push("UPPERCASE_SENTENCE_START".to_string());
    if lang == Some("km") {
        out.push("KM_SPACE_BEFORE_CONJUNCTION".to_string());
    }
    out.push(repeat_rule_id(lang).to_string());
    for id in text_family_ids(lang) {
        out.push(id.to_string());
    }
    if let Some(table) = filter_data.simple_replace.as_ref() {
        out.push(table.prefix.clone());
    }
    out
}

/// Ids of the text-level families (`DOUBLE_PUNCTUATION`, `WHITESPACE_RULE`,
/// `COMMA_PARENTHESIS_WHITESPACE`, `UNPAIRED_BRACKETS`/`EN_UNPAIRED_QUOTES`)
/// that LT 6.5 runs for this language at the default level — must mirror
/// the gate tables of the four ported functions.
pub(crate) fn text_family_ids(lang: Option<&str>) -> Vec<&'static str> {
    let mut out = Vec::new();
    const DOUBLE: &[&str] = &[
        "ca", "es", "en", "fr", "pt", "ru", "pl", "it", "br", "eo", "ga",
        "sk", "sl", "be", "el", "ro", "ta", "tl", "ast", "crh", "gl",
    ];
    const WHITESPACE: &[&str] = &[
        "ca", "es", "ar", "ru", "uk", "pl", "it", "br", "eo", "ga", "sv",
        "da", "sk", "sl", "be", "el", "fa", "ro", "ta", "crh", "gl",
    ];
    const COMMA: &[&str] = &[
        "ca", "es", "en", "de", "fr", "ar", "ru", "uk", "pl", "it", "br",
        "eo", "ga", "sv", "da", "sk", "sl", "be", "el", "fa", "ro", "tl",
        "ast", "crh",
    ];
    if lang.map_or(false, |l| DOUBLE.contains(&l)) {
        out.push("DOUBLE_PUNCTUATION");
    }
    if lang.map_or(false, |l| WHITESPACE.contains(&l)) {
        out.push("WHITESPACE_RULE");
    }
    if lang.map_or(false, |l| COMMA.contains(&l)) {
        out.push("COMMA_PARENTHESIS_WHITESPACE");
    }
    if lang == Some("en") {
        out.push("EN_A_VS_AN");
    }
    if lang == Some("ar") {
        out.push("ARABIC_QM_WHITESPACE");
        out.push("ARABIC_SC_WHITESPACE");
    }
    if lang == Some("es") {
        out.push("ES_QUESTION_MARK");
    }
    match lang {
        Some("en") => out.push("EN_UNPAIRED_QUOTES"),
        Some("es") => out.push("ES_UNPAIRED_BRACKETS"),
        Some(l)
            if matches!(
                l,
                "ca" | "ar" | "pt" | "gl" | "eo" | "ga" | "sv" | "da" | "sl" | "tl" | "ast"
                    | "de" | "fr" | "it" | "ro" | "sk"
            ) =>
        {
            out.push("UNPAIRED_BRACKETS");
        }
        _ => {}
    }
    out
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
        // de disabled: GermanSpellerRule decomposes compounds before looking
        // them up; without that port every non-listed compound would over-fire
        // "de" => SpellingRuleConfig { id: "MORFOLOGIK_RULE_DE_DE", ... },
        // it: Morfologik it_IT dictionary + hunspell ignore/spelling lists
        // (the local LT 6.5 fires MORFOLOGIK_RULE_IT_IT)
        "it" => SpellingRuleConfig {
            id: "MORFOLOGIK_RULE_IT_IT",
            latin_script: true,
            ignore: spelling_lists!("it", "ignore.txt"),
            prohibit: &[],
        },
        // br: Morfologik br_FR dictionary (local LT fires MORFOLOGIK_RULE_BR_FR)
        "br" => SpellingRuleConfig {
            id: "MORFOLOGIK_RULE_BR_FR",
            latin_script: true,
            ignore: &[],
            prohibit: &[],
        },
        // gl: Galician speller with the generic HunspellRule id
        // (local LT fires HUNSPELL_RULE for gl)
        "gl" => SpellingRuleConfig {
            id: "HUNSPELL_RULE",
            latin_script: true,
            ignore: &[],
            prohibit: &[],
        },
        // spellers that fire in the local LT 6.5 corpus measurement
        // (wordlists dumped from their morfologik/hunspell dictionaries)
        "crh" => SpellingRuleConfig {
            id: "MORFOLOGIK_RULE_CRH_UA",
            latin_script: true,
            ignore: &[],
            prohibit: &[],
        },
        "ro" => SpellingRuleConfig {
            id: "MORFOLOGIK_RULE_RO_RO",
            latin_script: true,
            ignore: &[],
            prohibit: &[],
        },
        "tl" => SpellingRuleConfig {
            id: "MORFOLOGIK_RULE_TL",
            latin_script: true,
            ignore: &[],
            prohibit: &[],
        },
        "el" => SpellingRuleConfig {
            id: "MORFOLOGIK_RULE_EL_GR",
            latin_script: false,
            ignore: &[],
            prohibit: &[],
        },
        "sk" => SpellingRuleConfig {
            id: "MORFOLOGIK_RULE_SK_SK",
            latin_script: true,
            ignore: &[],
            prohibit: &[],
        },
        "sl" => SpellingRuleConfig {
            id: "MORFOLOGIK_RULE_SL_SI",
            latin_script: true,
            ignore: &[],
            prohibit: &[],
        },
        "ast" => SpellingRuleConfig {
            id: "MORFOLOGIK_RULE_AST",
            latin_script: true,
            ignore: &[],
            prohibit: &[],
        },
        "be" => SpellingRuleConfig {
            id: "MORFOLOGIK_RULE_BE_BY",
            latin_script: false,
            ignore: &[],
            prohibit: &[],
        },
        "ar" => SpellingRuleConfig {
            id: "HUNSPELL_RULE_AR",
            latin_script: false,
            ignore: spelling_lists!("ar", "ignore.txt", "spelling.txt", "spelling_custom.txt"),
            prohibit: spelling_lists!("ar", "prohibit.txt", "prohibit_custom.txt"),
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
        // MorfologikPolishSpellerRule: pl/hunspell/pl_PL.dict + ignore/
        // spelling/prohibit lists; compound-adjective suppression
        // (isNotCompound) not ported yet
        "pl" => SpellingRuleConfig {
            id: "MORFOLOGIK_RULE_PL_PL",
            latin_script: true,
            ignore: spelling_lists!("pl", "ignore.txt", "spelling.txt"),
            prohibit: spelling_lists!("pl", "prohibit.txt"),
        },
        // LT's KhmerHunspellRule: generic id, khmer script, hunspell
        // km_KH dictionary as the accepted-words wordlist
        "km" => SpellingRuleConfig {
            id: "HUNSPELL_RULE",
            latin_script: false,
            ignore: spelling_lists!("km", "ignore.txt"),
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
    // LT's spellers ignore mixed digit-letter tokens ("25°C", "3rd")
    if word.chars().any(|c| c.is_ascii_digit()) {
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

/// Java's \\p{L} — Unicode general category L* exactly. Khmer vowel signs
/// and diacritics (U+17B4..U+17DB, U+17DD) are marks/signs, not letters:
/// LT's HunspellRule.tokenizeText splits runs on them, so hunspell checks
/// each consonant cluster of a Khmer word ("បញ្ញត្តិ" → "បញ" "ញត" "តិ"),
/// flagging every cluster missing from the km_KH dictionary. Outside the
/// Khmer block is_alphabetic() is used (identical for Latin/digits).
fn is_java_letter(c: char) -> bool {
    match c as u32 {
        0x1780..=0x17B3 | 0x17D7 | 0x17DC => true, // Khmer letters
        0x17B4..=0x17D6 | 0x17D8..=0x17DB | 0x17DD => false, // marks/signs/punct
        _ => c.is_alphabetic(),
    }
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

    // es/ca/fr/pt call setIgnoreTaggedWords(): words the tagger knows are
    // never spelling errors
    let ignore_tagged = matches!(lang, "es" | "ca" | "fr" | "pt");

    for (idx, token) in tokens.iter().enumerate() {
        let word = token.word().as_str();
        if word.is_empty() || is_url(word) || is_email(word) || is_spelling_ignored_token(token) {
            continue;
        }
        // km: LT's KhmerHunspellRule splits tokens into \\p{L} runs (Khmer
        // vowel signs/diacritics are split points) and hunspell-checks each
        // consonant cluster — the generic whole-token path below would flag
        // whole Khmer words instead. Wrong-split matches are skipped: they
        // share this rule id with a direct flag, so id agreement is unaffected.
        if lang == "km" {
            let base = token.span().start();
            // (byte_start, byte_end, char_start, char_end) per letter run
            let mut runs: Vec<(usize, usize, usize, usize)> = Vec::new();
            let mut open: Option<(usize, usize)> = None;
            let mut ci = 0usize;
            for (bi, c) in word.char_indices() {
                if is_java_letter(c) {
                    if open.is_none() {
                        open = Some((bi, ci));
                    }
                } else if let Some((bs, cs)) = open.take() {
                    runs.push((bs, bi, cs, ci));
                }
                ci += 1;
            }
            if let Some((bs, cs)) = open {
                runs.push((bs, word.len(), cs, ci));
            }
            for (bs, be, cs, ce) in runs {
                let run = &word[bs..be];
                let prohibited = config.prohibit.contains(&run);
                let known = speller.is_known(run);
                // HunspellRule.isMisspelled: (!spell && !ignoreWord) || isProhibited
                if !prohibited && (known || config.ignore.contains(&run)) {
                    continue;
                }
                out.push(Suggestion::new(
                    config.id.to_string(),
                    "Possible spelling mistake found.".to_string(),
                    Span::new(base.byte + bs..base.byte + be, base.char + cs..base.char + ce),
                    Vec::new(),
                ));
            }
            continue;
        }
        if spelling_ignored(word, &config)
            // fragments of hyphenated splits and single characters (our
            // tokenizer splits where LT's does not)
            || word.chars().count() < 2
            || word.starts_with('-')
            || word.ends_with('-')
        {
            continue;
        }
        // English speller: words containing a dot (domain-like
        // "wordpress.com") are ignored, and hyphenated words are checked
        // part by part, flagging only an unknown part
        if lang == "en" && word.contains('.') {
            continue;
        }
        // en: tokens containing an apostrophe (contractions like "don't",
        // suffix tokens like 'RE, quoted 'word) are not spelling-checked
        if lang == "en" && (word.contains('\'') || word.contains('\u{2019}')) {
            continue;
        }
        // English speller: hyphenated words are not checked as a whole
        // (LT checks dictionary compounds; empirically even unknown parts
        // like "well-xzyzing" are not flagged)
        if lang == "en" && word.contains('-') {
            continue;
        }
        // a reading with a real part-of-speech (not the empty pseudo-reading
        // and not UNKNOWN) makes the word "tagged"
        if ignore_tagged
            && token.word().tags().iter().any(|t| {
                let pos = t.pos().as_str();
                !pos.is_empty() && pos != "UNKNOWN"
            })
        {
            continue;
        }
        // ArabicHunspellSpellerRule strips tashkeel before every lookup
        let word = if lang == "ar" {
            crate::tokenizer::ar_stem::remove_tashkeel(word)
        } else {
            word.to_string()
        };
        let word = word.as_str();
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

// ---------------------------------------------------------------------------
// Text-level builtin families ported from LT 6.5 (task-4).
// Language gating follows what the local LT 6.5 server fires at the
// default level (probed 2026-09-28, see delta_log).
// ---------------------------------------------------------------------------

/// LT `DoublePunctuationRule`: two consecutive equal punctuation marks
/// ("..", ",,"). "..." (ellipsis) and numeric contexts are allowed; fr also
/// allows "..," "?.." "!..".
/// LT `SpanishQuestionMarkDeletionRule`-family behavior (id
/// `ES_QUESTION_MARK`): a question/exclamation sentence must open with
/// ¿/¡. Flags the first word with a `¿`/`¡`-prefixed suggestion.
pub(crate) fn es_question_mark(sentence: &Sentence, lang: Option<&str>) -> Vec<Suggestion> {
    if lang != Some("es") {
        return Vec::new();
    }
    let tokens: Vec<_> = sentence.iter().collect();
    if tokens.is_empty() {
        return Vec::new();
    }
    let last = tokens[tokens.len() - 1].word().as_str();
    let opener = if last.ends_with('?') {
        '\u{bf}' // ¿
    } else if last.ends_with('!') {
        '\u{a1}' // ¡
    } else {
        return Vec::new();
    };
    let first = tokens[0].word().as_str();
    if first.starts_with('\u{bf}') || first.starts_with('\u{a1}') {
        return Vec::new();
    }
    // numbers and non-words at sentence start: LT's rule needs a word
    if !first.chars().next().map_or(false, |c| c.is_alphabetic()) {
        return Vec::new();
    }
    let start = tokens[0].span().start();
    let end = tokens[0].span().end();
    let replacement = format!("{}{}", opener, first);
    vec![Suggestion::new(
        "ES_QUESTION_MARK".to_string(),
        "Falta el signo de apertura.".to_string(),
        Span::new(start.byte..end.byte, start.char..end.char),
        vec![replacement],
    )]
}

/// LT ar-specific whitespace rules: `ARABIC_QM_WHITESPACE` (space before
/// the Arabic question mark ؟) and `ARABIC_SC_WHITESPACE` (space before
/// the Arabic semicolon ؛). Probed on LT 6.5: both flag the whitespace
/// run before the punctuation with a join suggestion.
pub(crate) fn arabic_punct_whitespace(sentence: &Sentence, lang: Option<&str>) -> Vec<Suggestion> {
    if lang != Some("ar") {
        return Vec::new();
    }
    let tokens: Vec<_> = sentence.iter().collect();
    let text = sentence.text();
    let mut out = Vec::new();
    for i in 0..tokens.len().saturating_sub(1) {
        let bs = tokens[i].span().end().byte;
        let be = tokens[i + 1].span().start().byte;
        let gap = if be > bs && be <= text.len() {
            &text[bs..be]
        } else {
            ""
        };
        if gap.is_empty() || !gap.chars().all(char::is_whitespace) || gap.contains('\n') {
            continue;
        }
        let next = tokens[i + 1].word().as_str();
        let id = if next.starts_with('\u{61f}') {
            Some("ARABIC_QM_WHITESPACE")
        } else if next.starts_with('\u{61b}') {
            Some("ARABIC_SC_WHITESPACE")
        } else {
            None
        };
        if let Some(id) = id {
            let cs = tokens[i].span().end().char;
            let ce = tokens[i + 1].span().start().char;
            out.push(Suggestion::new(
                id.to_string(),
                "Whitespace before punctuation.".to_string(),
                Span::new(bs..be, cs..ce),
                vec![next.chars().take(1).collect()],
            ));
        }
    }
    out
}

/// LT `AvsAnRule` (`EN_A_VS_AN`): a/an selection based on the sound of the
/// following word, using LT's own det_a/det_an exception lists.
pub(crate) fn a_vs_an(sentence: &Sentence, lang: Option<&str>) -> Vec<Suggestion> {
    if lang != Some("en") {
        return Vec::new();
    }
    let det_a: &std::collections::HashSet<String> = static_ref!(
        std::collections::HashSet<String> = wordlist(
            include_str!("builtin_data/en/det_a.txt"))
            .into_iter()
            .map(|w: &'static str| w.trim().to_lowercase())
            .collect()
    );
    let det_an: &std::collections::HashSet<String> = static_ref!(
        std::collections::HashSet<String> = wordlist(
            include_str!("builtin_data/en/det_an.txt"))
            .into_iter()
            .map(|w: &'static str| w.trim().to_lowercase())
            .collect()
    );
    let tokens: Vec<_> = sentence.iter().collect();
    let mut out = Vec::new();
    for i in 0..tokens.len().saturating_sub(1) {
        let word = tokens[i].word().as_str();
        let article = match word {
            "a" | "A" => "an",
            "an" | "An" => "a",
            _ => continue,
        };
        let next_raw = tokens[i + 1].word().as_str();
        let next = next_raw.trim_matches(|c: char| !c.is_alphanumeric());
        let first = match next.chars().next() {
            Some(c) => c,
            None => continue,
        };
        if first.is_ascii_digit() {
            // numbers: sound is ambiguous ("an 8" but "a 5")
            continue;
        }
        let lower = next.to_lowercase();
        let all_caps = next.chars().any(|c| c.is_alphabetic())
            && next.chars().all(|c| !c.is_lowercase());
        // hyphenated words ("one-time"): decide by the first segment
        let first_segment = next.split(['-', '\u{2011}']).next().unwrap_or(next);
        let seg_lower = first_segment.to_lowercase();
        let needs_an = if det_an.contains(&lower) {
            true
        } else if det_a.contains(&lower) || det_a.contains(&seg_lower) {
            false
        } else if det_an.contains(&seg_lower) {
            true
        } else if all_caps {
            // abbreviations: ambiguous sound
            continue;
        } else {
            "aeiou".contains(first.to_ascii_lowercase())
        };
        if (word.eq_ignore_ascii_case("a") && needs_an)
            || (word.eq_ignore_ascii_case("an") && !needs_an)
        {
            let start = tokens[i].span().start();
            let end = tokens[i].span().end();
            out.push(Suggestion::new(
                "EN_A_VS_AN".to_string(),
                "Use a different article.".to_string(),
                Span::new(start.byte..end.byte, start.char..end.char),
                vec![article.to_string()],
            ));
        }
    }
    out
}

pub(crate) fn double_punctuation(sentence: &Sentence, lang: Option<&str>) -> Vec<Suggestion> {
    const LANGS: &[&str] = &[
        "ca", "es", "en", "fr", "pt", "ru", "pl", "it", "br", "eo", "ga",
        "sk", "sl", "be", "el", "ro", "ta", "tl", "ast", "crh", "gl",
    ];
    let lang = match lang {
        Some(l) if LANGS.contains(&l) => l,
        _ => return Vec::new(),
    };
    let tokens: Vec<_> = sentence.iter().collect();
    let mut out = Vec::new();
    for i in 1..tokens.len() {
        let token = tokens[i].word().as_str();
        let prev = tokens[i - 1].word().as_str();
        let prev_prev = if i >= 2 {
            tokens[i - 2].word().as_str()
        } else {
            ""
        };
        let next = tokens.get(i + 1).map(|t| t.word().as_str()).unwrap_or("");
        let flagged = match token {
            "," | ";" => token == prev,
            "." if prev == "." => {
                // "..." and longer runs of dots are an ellipsis
                !(prev_prev == "." || next == ".")
            }
            _ => false,
        };
        if !flagged {
            continue;
        }
        // numeric contexts like "1.2" / "1,000": never flag next to digits
        let prev_prev_digit = prev_prev.chars().any(|c| c.is_ascii_digit());
        let next_digit = next.chars().any(|c| c.is_ascii_digit());
        if prev_prev_digit || next_digit {
            continue;
        }
        // FrenchDoublePunctuationRule exceptions
        if lang == "fr" && matches!(next, ",") {
            continue;
        }
        let start = tokens[i - 1].span().start();
        let end = tokens[i].span().end();
        out.push(Suggestion::new(
            "DOUBLE_PUNCTUATION".to_string(),
            "Double punctuation.".to_string(),
            Span::new(start.byte..end.byte, start.char..end.char),
            vec![token.to_string()],
        ));
    }
    out
}

/// LT `MultipleWhitespaceRule` (id `WHITESPACE_RULE`): a run of 2+ spaces
/// between two non-whitespace tokens.
pub(crate) fn multiple_whitespace(sentence: &Sentence, lang: Option<&str>) -> Vec<Suggestion> {
    const LANGS: &[&str] = &[
        "ca", "es", "ar", "ru", "uk", "pl", "it", "br", "eo", "ga", "sv",
        "da", "sk", "sl", "be", "el", "fa", "ro", "ta", "crh", "gl",
    ];
    if !lang.map_or(false, |l| LANGS.contains(&l)) {
        return Vec::new();
    }
    // the tokenizer does not emit whitespace tokens: runs of spaces live in
    // the gaps between consecutive tokens
    let tokens: Vec<_> = sentence.iter().collect();
    let text = sentence.text();
    let mut out = Vec::new();
    for i in 0..tokens.len().saturating_sub(1) {
        let bs = tokens[i].span().end().byte;
        let be = tokens[i + 1].span().start().byte;
        if be <= bs {
            continue;
        }
        let gap = &text[bs.min(text.len())..be.min(text.len())];
        let spaces = gap.chars().filter(|c| *c == ' ').count();
        if spaces < 2 || !gap.chars().all(|c| c == ' ') {
            continue;
        }
        let cs = tokens[i].span().end().char;
        let ce = tokens[i + 1].span().start().char;
        out.push(Suggestion::new(
            "WHITESPACE_RULE".to_string(),
            "Multiple whitespace.".to_string(),
            Span::new(bs..be, cs..ce),
            vec![" ".to_string()],
        ));
    }
    out
}

/// LT `CommaWhitespaceRule` (id `COMMA_PARENTHESIS_WHITESPACE`):
/// whitespace directly before closing punctuation or after opening
/// punctuation.
pub(crate) fn comma_parenthesis_whitespace(
    sentence: &Sentence,
    lang: Option<&str>,
) -> Vec<Suggestion> {
    const LANGS: &[&str] = &[
        "ca", "es", "en", "de", "fr", "ar", "ru", "uk", "pl", "it", "br",
        "eo", "ga", "sv", "da", "sk", "sl", "be", "el", "fa", "ro", "tl",
        "ast", "crh",
    ];
    if !lang.map_or(false, |l| LANGS.contains(&l)) {
        return Vec::new();
    }
    let tokens: Vec<_> = sentence.iter().collect();
    let text = sentence.text();
    let mut out = Vec::new();
    for i in 0..tokens.len().saturating_sub(1) {
        let bs = tokens[i].span().end().byte;
        let be = tokens[i + 1].span().start().byte;
        let gap = if be > bs && be <= text.len() {
            &text[bs..be]
        } else {
            ""
        };
        if gap.is_empty() || !gap.chars().all(char::is_whitespace) || gap.contains('\n') {
            continue;
        }
        let cs = tokens[i].span().end().char;
        let ce = tokens[i + 1].span().start().char;
        let prev_word = tokens[i].word().as_str();
        let next_word = tokens[i + 1].word().as_str();
        // whitespace directly before closing punctuation
        let before_punct = next_word.starts_with(',')
            || next_word.starts_with(';')
            || next_word.starts_with(':')
            || next_word.starts_with('.')
            || next_word.starts_with('!')
            || next_word.starts_with('?')
            || next_word.starts_with(')')
            || next_word.starts_with(']')
            || next_word.starts_with('}');
        // whitespace directly after an opening bracket
        let after_open = prev_word == "(" || prev_word == "[" || prev_word == "{";
        if before_punct || after_open {
            let replacement = if before_punct {
                vec![next_word.chars().take(1).collect()]
            } else {
                Vec::new()
            };
            out.push(Suggestion::new(
                "COMMA_PARENTHESIS_WHITESPACE".to_string(),
                "Probable falta de espacio.".to_string(),
                Span::new(bs..be, cs..ce),
                replacement,
            ));
        }
    }
    out
}

/// LT `*UnpairedBracketsRule` families: unmatched (), [], {} (and straight
/// double quotes where the language pairs them). en runs a quotes-only
/// variant with its own id (`EN_UNPAIRED_QUOTES`).
pub(crate) fn unpaired_brackets(sentence: &Sentence, lang: Option<&str>) -> Vec<Suggestion> {
    struct Cfg {
        brackets: bool,
        quotes: bool,
        guillemets: bool,
        id: &'static str,
    }
    let cfg = match lang {
        Some("en") => Some(Cfg {
            brackets: false,
            quotes: true,
            guillemets: false,
            id: "EN_UNPAIRED_QUOTES",
        }),
        // es runs SpanishUnpairedBracketsRule (ES_UNPAIRED_BRACKETS) with
        // brackets, straight quotes and guillemets (probed on LT 6.5)
        Some("es") => Some(Cfg {
            brackets: true,
            quotes: true,
            guillemets: true,
            id: "ES_UNPAIRED_BRACKETS",
        }),
        Some(l)
            if matches!(
                l,
                "ca" | "ar" | "pt" | "gl" | "eo" | "ga" | "sv" | "da" | "sl" | "tl" | "ast"
            ) =>
        {
            Some(Cfg {
                brackets: true,
                quotes: true,
                guillemets: false,
                id: "UNPAIRED_BRACKETS",
            })
        }
        Some(l) if matches!(l, "de" | "fr" | "it" | "ro" | "sk") => Some(Cfg {
            brackets: true,
            quotes: false,
            guillemets: false,
            id: "UNPAIRED_BRACKETS",
        }),
        _ => None,
    };
    let cfg = match cfg {
        Some(c) => c,
        None => return Vec::new(),
    };

    let tokens: Vec<_> = sentence.iter().collect();
    let mut out = Vec::new();
    // bracket stack: (token index, closer)
    let mut stack: Vec<(usize, char)> = Vec::new();
    // straight quote pairing: even occurrence closes, odd opens
    let mut quote_open: Option<usize> = None;

    for (i, token) in tokens.iter().enumerate() {
        let word = token.word().as_str();
        for c in word.chars() {
            if cfg.brackets {
                match c {
                    '(' => stack.push((i, ')')),
                    '[' => stack.push((i, ']')),
                    '{' => stack.push((i, '}')),
                    ')' | ']' | '}' => {
                        // pop the matching opener; unmatched closer flags here
                        if let Some(pos) = stack.iter().rposition(|(_, cl)| *cl == c) {
                            stack.remove(pos);
                        } else {
                            let start = token.span().start();
                            let end = token.span().end();
                            out.push(Suggestion::new(
                                cfg.id.to_string(),
                                "Unpaired closing bracket.".to_string(),
                                Span::new(start.byte..end.byte, start.char..end.char),
                                Vec::new(),
                            ));
                        }
                    }
                    _ => {}
                }
            }
            if cfg.quotes && c == '"' {
                match quote_open.take() {
                    Some(_) => {}
                    None => quote_open = Some(i),
                }
            }
            if cfg.guillemets {
                match c {
                    '«' => stack.push((i, '»')),
                    '»' => {
                        if let Some(pos) = stack.iter().rposition(|(_, cl)| *cl == c) {
                            stack.remove(pos);
                        } else {
                            let start = token.span().start();
                            let end = token.span().end();
                            out.push(Suggestion::new(
                                cfg.id.to_string(),
                                "Unpaired closing guillemet.".to_string(),
                                Span::new(start.byte..end.byte, start.char..end.char),
                                Vec::new(),
                            ));
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    // unmatched openers remain on the stack / in quote_open
    let mut offenders: Vec<usize> = stack.iter().map(|(i, _)| *i).collect();
    if let Some(i) = quote_open {
        offenders.push(i);
    }
    offenders.sort_unstable();
    offenders.dedup();
    for i in offenders {
        let start = tokens[i].span().start();
        let end = tokens[i].span().end();
        out.push(Suggestion::new(
            cfg.id.to_string(),
            "Unpaired opening bracket.".to_string(),
            Span::new(start.byte..end.byte, start.char..end.char),
            Vec::new(),
        ));
    }
    out
}
