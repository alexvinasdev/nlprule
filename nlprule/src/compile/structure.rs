use fs_err::File;
use serde::Deserialize;
use std::io::BufReader;
use xml::reader::EventReader;

pub mod preprocess {
    use std::{borrow::Cow, str::FromStr};

    use lazy_static::lazy_static;
    use xml::{attribute::OwnedAttribute, reader::EventReader};
    use xml::{name::OwnedName, writer::EmitterConfig};

    use super::Category;

    pub fn sanitize(input: impl std::io::Read, whitespace_sensitive_tags: &[&str]) -> String {
        let mut sanitized = Vec::new();

        let mut writer = EmitterConfig::new()
            .perform_indent(true)
            .create_writer(&mut sanitized);

        let parser = EventReader::new(input);

        let events = parser
            .into_iter()
            .map(|x| x.expect("error reading XML"))
            .filter(|x| {
                // processing instructions break the writer and are useless to us
                !matches!(x, xml::reader::XmlEvent::ProcessingInstruction { .. })
                    // commented-out rules must not be parsed
                    && !matches!(x, xml::reader::XmlEvent::Comment { .. })
            })
            .collect::<Vec<_>>();

        let mut out_events: Vec<xml::writer::XmlEvent> = Vec::new();
        let mut parents: Vec<(&OwnedName, &Vec<OwnedAttribute>)> = Vec::new();

        for event in &events {
            match event {
                xml::reader::XmlEvent::StartElement {
                    name,
                    attributes,
                    namespace,
                } => {
                    let unify_index = parents
                        .iter()
                        .position(|(name, _)| name.local_name.as_str() == "unify");
                    let ignore_index = parents
                        .iter()
                        .position(|(name, _)| name.local_name.as_str() == "unify-ignore");

                    parents.push((name, attributes));

                    let unify = unify_index.map_or(false, |i| {
                        ignore_index.map_or(true, |ignore_i| i > ignore_i)
                    });

                    if unify && name.local_name == "token" {
                        lazy_static! {
                            static ref UNIFY_ATTRIBUTE: OwnedAttribute =
                                OwnedAttribute::new(OwnedName::from_str("unify").unwrap(), "yes",);
                        }
                        lazy_static! {
                            static ref UNIFY_NEGATE_ATTRIBUTE: OwnedAttribute = OwnedAttribute::new(
                                OwnedName::from_str("unify").unwrap(),
                                "negate",
                            );
                        }

                        // we push to parents after index is computed but it doesn't matter because the index stays the same
                        let negate = parents[unify_index.unwrap()]
                            .1
                            .iter()
                            .any(|x| x.name.local_name == "negate" && x.value == "yes");

                        out_events.push(xml::writer::XmlEvent::StartElement {
                            name: name.borrow(),
                            attributes: attributes
                                .iter()
                                .chain(if negate {
                                    vec![&*UNIFY_NEGATE_ATTRIBUTE]
                                } else {
                                    vec![&*UNIFY_ATTRIBUTE]
                                })
                                .map(|a| a.borrow())
                                .collect(),
                            namespace: Cow::Borrowed(namespace),
                        });
                        continue;
                    }

                    if ["unify", "unify-ignore"].contains(&name.local_name.as_str()) {
                        continue;
                    }
                }
                xml::reader::XmlEvent::EndElement { name, .. } => {
                    parents.pop();

                    if ["unify", "unify-ignore"].contains(&name.local_name.as_str()) {
                        continue;
                    }
                }
                xml::reader::XmlEvent::Characters(chars) => {
                    out_events.push(
                        xml::writer::XmlEvent::start_element("text")
                            .attr("text", chars)
                            .into(),
                    );
                    out_events.push(xml::writer::XmlEvent::end_element().into());
                    continue;
                }
                xml::reader::XmlEvent::Whitespace(whitespace) => {
                    let whitespace_sensitive = parents.iter().any(|(name, _)| {
                        whitespace_sensitive_tags.contains(&name.local_name.as_str())
                    });
                    if whitespace_sensitive {
                        out_events.push(
                            xml::writer::XmlEvent::start_element("text")
                                .attr("text", whitespace)
                                .into(),
                        );
                        out_events.push(xml::writer::XmlEvent::end_element().into());
                        continue;
                    }
                }
                _ => {}
            }

            if let Some(writer_event) = event.as_writer_event() {
                out_events.push(writer_event);
            }
        }

        for event in out_events {
            writer.write(event).expect("error writing to output XML");
        }

        std::str::from_utf8(&sanitized)
            .expect("invalid UTF-8")
            .to_string()
    }

    pub fn extract_rules(mut xml: impl std::io::Read) -> Vec<(String, Option<Category>)> {
        let mut string = String::new();
        xml.read_to_string(&mut string)
            .expect("error writing to string.");

        let document = roxmltree::Document::parse(&string).expect("error parsing XML");

        document
            .descendants()
            .filter(|x| {
                let name = x.tag_name().name();

                name == "unification"
                    || name == "rulegroup"
                    || (name == "rule"
                        && x.parent_element()
                            .expect("must have parent")
                            .tag_name()
                            .name()
                            != "rulegroup")
            })
            .map(|x| {
                let xml = string[x.range()].to_string();
                let parent = x.parent_element().expect("must have parent");

                let category = if parent.tag_name().name() == "category" {
                    Some(Category {
                        id: parent.attribute("id").unwrap().to_owned(),
                        name: parent.attribute("name").unwrap().to_owned(),
                        kind: parent.attribute("type").map(|x| x.to_owned()),
                        default: parent.attribute("default").map(|x| x.to_owned()),
                    })
                } else {
                    None
                };

                (xml, category)
            })
            .collect()
    }
}

#[derive(Debug, Clone)]
pub struct Group {
    pub id: String,
    pub name: String,
    pub default: Option<String>,
    /// Rule metadata tag (LT 6.x), e.g. "picky".
    pub tags: Option<String>,
    pub n: usize,
}

#[derive(Debug, Clone)]
pub struct Category {
    pub id: String,
    pub name: String,
    pub kind: Option<String>,
    pub default: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct XmlString {
    #[serde(default)]
    pub text: String,
}

impl std::ops::Deref for XmlString {
    type Target = String;

    fn deref(&self) -> &Self::Target {
        &self.text
    }
}

impl From<XmlString> for String {
    fn from(data: XmlString) -> String {
        data.text
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct XmlText {
    pub text: XmlString,
}

impl std::ops::Deref for XmlText {
    type Target = String;

    fn deref(&self) -> &Self::Target {
        &self.text
    }
}

impl From<XmlText> for String {
    fn from(data: XmlText) -> String {
        data.text.into()
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Match {
    pub no: String,
    pub postag: Option<String>,
    #[serde(rename = "postag_regexp")]
    pub postag_regex: Option<String>,
    pub postag_replace: Option<String>,
    pub text: Option<XmlString>,
    pub include_skipped: Option<String>,
    pub case_conversion: Option<String>,
    pub regexp_match: Option<String>,
    pub regexp_replace: Option<String>,
    /// Element content used as a static lemma, e.g. `<match no="1" postag="VBN">word</match>`.
    #[serde(rename = "$value")]
    pub content: Option<XmlString>,
    /// `setpos="yes"`: take over the POS of the matched token (LT 6.5);
    /// parsed, the default postag handling is a close approximation
    #[serde(default, rename = "setpos")]
    pub setpos: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "lowercase")]
pub enum SuggestionPart {
    Match(Match),
    Text(XmlString),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Suggestion {
    pub suppress_misspelled: Option<String>,
    /// `<suggestion></suggestion>` (no text) is valid - it suppresses any
    /// replacement
    #[serde(rename = "$value", default)]
    pub parts: Vec<SuggestionPart>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "lowercase")]
pub enum MessagePart {
    Suggestion(Suggestion),
    Text(XmlString),
    Match(Match),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Message {
    #[serde(rename = "$value")]
    pub parts: Vec<MessagePart>,
    pub suppress_misspelled: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExampleMarker {
    pub text: XmlString,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "lowercase")]
pub enum ExamplePart {
    Marker(ExampleMarker),
    Text(XmlString),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Example {
    pub reason: Option<String>,
    pub correction: Option<String>,
    #[serde(rename = "$value")]
    pub parts: Vec<ExamplePart>,
    #[serde(rename = "type")]
    pub kind: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Exception {
    pub case_sensitive: Option<String>,
    pub inflected: Option<String>,
    pub postag: Option<String>,
    pub postag_regexp: Option<String>,
    pub chunk: Option<String>,
    pub chunk_re: Option<String>,
    pub regexp: Option<String>,
    pub spacebefore: Option<String>,
    pub negate: Option<String>,
    pub negate_pos: Option<String>,
    pub scope: Option<String>,
    pub text: Option<XmlString>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "lowercase")]
#[serde(deny_unknown_fields)]
#[allow(clippy::large_enum_variant)]
pub enum TokenPart {
    Text(XmlString),
    Exception(Exception),
    #[serde(rename = "match")]
    Sub(Sub),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sub {
    pub no: String,
    /// `<match no="N" postag="..." postag_regexp="yes"/>` inside a pattern
    /// token: the current token must additionally match this postag (the
    /// text still comes from the referenced token).
    pub postag: Option<String>,
    pub postag_regexp: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Token {
    pub min: Option<String>,
    pub max: Option<String>,
    pub skip: Option<String>,
    /// Token type attribute (LT 6.x); accepted but unused.
    #[serde(rename = "type")]
    pub token_type: Option<String>,
    /// LT 6.x attributes; accepted but unused.
    pub raw_pos: Option<String>,
    pub setpos: Option<String>,
    pub unify: Option<String>,
    pub case_sensitive: Option<String>,
    pub inflected: Option<String>,
    pub postag: Option<String>,
    pub postag_regexp: Option<String>,
    pub chunk: Option<String>,
    pub chunk_re: Option<String>,
    pub regexp: Option<String>,
    pub spacebefore: Option<String>,
    pub negate: Option<String>,
    pub negate_pos: Option<String>,
    #[serde(rename = "$value")]
    pub parts: Option<Vec<TokenPart>>,
}

// NB: needlessly verbose, would be nicer with #[serde(flatten)] but blocked by https://github.com/RReverser/serde-xml-rs/issues/83
pub trait MatchAttributes {
    fn case_sensitive(&self) -> &Option<String>;
    fn inflected(&self) -> &Option<String>;
    fn postag(&self) -> &Option<String>;
    fn postag_regexp(&self) -> &Option<String>;
    fn chunk(&self) -> &Option<String>;
    fn chunk_re(&self) -> &Option<String>;
    fn regexp(&self) -> &Option<String>;
    fn spacebefore(&self) -> &Option<String>;
    fn negate(&self) -> &Option<String>;
    fn negate_pos(&self) -> &Option<String>;
}

macro_rules! impl_match_attributes {
    ($e:ty) => {
        impl MatchAttributes for $e {
            fn case_sensitive(&self) -> &Option<String> {
                &self.case_sensitive
            }

            fn inflected(&self) -> &Option<String> {
                &self.inflected
            }

            fn postag(&self) -> &Option<String> {
                &self.postag
            }

            fn postag_regexp(&self) -> &Option<String> {
                &self.postag_regexp
            }

            fn chunk(&self) -> &Option<String> {
                &self.chunk
            }

            fn chunk_re(&self) -> &Option<String> {
                &self.chunk_re
            }

            fn regexp(&self) -> &Option<String> {
                &self.regexp
            }

            fn spacebefore(&self) -> &Option<String> {
                &self.spacebefore
            }

            fn negate(&self) -> &Option<String> {
                &self.negate
            }

            fn negate_pos(&self) -> &Option<String> {
                &self.negate_pos
            }
        }
    };
}

impl_match_attributes!(&Exception);
impl_match_attributes!(&Token);

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TokenVector {
    #[serde(rename = "token")]
    pub tokens: Vec<Token>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Feature {
    pub id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "lowercase")]
#[serde(deny_unknown_fields)]
pub enum TokenCombination {
    Token(Token),
    Or(TokenVector),
    And(TokenVector),
    Feature(Feature),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PatternMarker {
    #[serde(rename = "$value")]
    pub tokens: Vec<TokenCombination>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "lowercase")]
#[serde(deny_unknown_fields)]
pub enum PatternPart {
    Token(Token),
    Marker(PatternMarker),
    Or(TokenVector),
    And(TokenVector),
    Feature(Feature),
    /// Documentation examples inside `pattern` / `antipattern` (LT 6.x); ignored.
    Example(Example),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pattern {
    pub case_sensitive: Option<String>,
    /// `raw_pos="yes"`: unify on the raw postag (LT 6.5); parsed, the unify
    /// engine treats features equivalently
    #[serde(default, rename = "raw_pos")]
    pub raw_pos: Option<String>,
    #[serde(rename = "$value")]
    pub parts: Vec<PatternPart>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Regex {
    pub text: XmlString,
    pub case_sensitive: Option<String>,
    pub mark: Option<String>,
    /// e.g. `type="exact"` (LT 6.5); parsed, semantics not differentiated
    #[serde(rename = "type")]
    pub kind: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub pattern: Option<Pattern>,
    #[serde(rename = "regexp")]
    pub regex: Option<Regex>,
    #[serde(rename = "antipattern")]
    pub antipatterns: Option<Vec<Pattern>>,
    /// The `<filter>` element can appear between the pattern and the message.
    pub filter: Option<Filter>,
    pub message: Message,
    #[serde(rename = "suggestion")]
    pub suggestions: Option<Vec<Suggestion>>,
    #[serde(rename = "example", default)]
    pub examples: Vec<Example>,
    pub id: Option<String>,
    pub name: Option<String>,
    pub short: Option<XmlText>,
    pub url: Option<XmlText>,
    pub default: Option<String>,
    /// Rule metadata tag (LT 6.x); accepted but unused.
    pub tags: Option<String>,
    /// Rule type attribute (LT 6.x, e.g. 'personal'); accepted but unused.
    #[serde(rename = "type")]
    pub rule_type: Option<String>,
    /// LT premium flag; accepted but unused.
    pub premium: Option<String>,
    /// LT 6.x tone tags; accepted but unused.
    pub tone_tags: Option<String>,
    /// LT 6.5 attributes parsed but not differentiated
    #[serde(default, rename = "min_prev_matches")]
    pub min_prev_matches: Option<String>,
    #[serde(default, rename = "distance_tokens")]
    pub distance_tokens: Option<String>,
    #[serde(default, rename = "is_goal_specific")]
    pub is_goal_specific: Option<String>,
    #[serde(rename = "__unused_unifications")]
    pub unifications: Option<Vec<Unification>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleGroup {
    pub id: String,
    #[serde(rename = "antipattern")]
    pub antipatterns: Option<Vec<Pattern>>,
    pub default: Option<String>,
    pub name: String,
    /// Rule type attribute (LT 6.x, e.g. 'typographical'); accepted but unused.
    #[serde(rename = "type")]
    pub rule_type: Option<String>,
    /// Rule metadata tag (LT 6.x); accepted but unused.
    pub tags: Option<String>,
    pub short: Option<XmlText>,
    pub url: Option<XmlText>,
    #[serde(default)]
    pub tone_tags: Option<String>,
    #[serde(default, rename = "min_prev_matches")]
    pub min_prev_matches: Option<String>,
    #[serde(rename = "rule")]
    pub rules: Vec<Rule>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "lowercase")]
#[serde(deny_unknown_fields)]
pub enum RuleContainer {
    Rule(Rule),
    RuleGroup(RuleGroup),
    Unification(Unification),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DisambiguationExample {
    #[serde(rename = "type")]
    pub kind: String,
    pub inputform: Option<String>,
    pub outputform: Option<String>,
    #[serde(rename = "$value")]
    pub parts: Vec<ExamplePart>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WordData {
    pub pos: Option<String>,
    pub text: Option<String>,
    pub lemma: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Filter {
    pub args: String,
    pub class: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DisambiguationMatch {
    pub no: usize,
    pub postag: Option<String>,
    pub postag_regexp: Option<String>,
    /// Case conversion of the referenced token's text; accepted but unused.
    pub case_conversion: Option<String>,
    /// Element content: the literal text the referenced token must have.
    #[serde(rename = "$value")]
    pub content: Option<XmlString>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum DisambiguationPart {
    #[serde(rename = "wd")]
    WordData(WordData),
    #[serde(rename = "match")]
    Match(DisambiguationMatch),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Disambiguation {
    pub postag: Option<String>,
    pub action: Option<String>,
    #[serde(rename = "$value")]
    pub word_datas: Option<Vec<DisambiguationPart>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DisambiguationRule {
    pub pattern: Pattern,
    /// `equivalence` elements (feature unification hints); parsed but unused.
    #[serde(rename = "equivalence", default)]
    pub equivalences: Option<Vec<serde::de::IgnoredAny>>,
    #[serde(rename = "antipattern")]
    pub antipatterns: Option<Vec<Pattern>>,
    #[serde(rename = "suggestion")]
    pub suggestions: Option<Vec<Suggestion>>,
    pub disambig: Disambiguation,
    #[serde(rename = "example")]
    pub examples: Option<Vec<DisambiguationExample>>,
    pub id: Option<String>,
    pub name: Option<String>,
    pub filter: Option<Filter>,
    #[serde(rename = "__unused_unifications")]
    pub unifications: Option<Vec<Unification>>,
    pub default: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DisambiguationRuleGroup {
    pub id: String,
    #[serde(rename = "antipattern")]
    pub antipatterns: Option<Vec<Pattern>>,
    pub name: String,
    #[serde(rename = "rule")]
    pub rules: Vec<DisambiguationRule>,
    pub default: Option<String>,
    /// Rule metadata tag (LT 6.x), e.g. "picky".
    #[serde(default)]
    pub tags: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EquivalenceToken {
    pub postag: String,
    pub postag_regexp: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Equivalence {
    pub token: EquivalenceToken,
    #[serde(rename = "type")]
    pub kind: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Unification {
    #[serde(rename = "equivalence")]
    pub equivalences: Vec<Equivalence>,
    pub feature: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "lowercase")]
#[serde(deny_unknown_fields)]
pub enum DisambiguationRuleContainer {
    Rule(DisambiguationRule),
    RuleGroup(DisambiguationRuleGroup),
    Unification(Unification),
}

macro_rules! flatten_group {
    ($rulegroup:expr, $category:expr) => {{
        let group_antipatterns = if let Some(antipatterns) = $rulegroup.antipatterns {
            antipatterns
        } else {
            Vec::new()
        };

        let group = Group {
            id: $rulegroup.id,
            default: $rulegroup.default,
            name: $rulegroup.name,
            tags: $rulegroup.tags,
            n: 0,
        };

        $rulegroup
            .rules
            .into_iter()
            .enumerate()
            .map(|(i, mut rule)| {
                if let Some(antipatterns) = &mut rule.antipatterns {
                    antipatterns.extend(group_antipatterns.clone());
                } else {
                    rule.antipatterns = Some(group_antipatterns.clone());
                }

                let mut group = group.clone();
                group.n = i;
                (rule, Some(group), $category.clone())
            })
            .collect::<Vec<_>>()
    }};
}

type GrammarRuleReading = (Rule, Option<Group>, Option<Category>);
type DisambiguationRuleReading = (DisambiguationRule, Option<Group>, Option<Category>);

pub fn read_rules<P: AsRef<std::path::Path>>(
    path: P,
) -> (
    Vec<Result<GrammarRuleReading, serde_xml_rs::Error>>,
    Vec<crate::rule::regex_rule::RegexRuleDef>,
) {
    let file = File::open(path.as_ref()).unwrap();
    let mut raw_xml = String::new();
    {
        use std::io::Read;
        File::open(path.as_ref())
            .unwrap()
            .read_to_string(&mut raw_xml)
            .unwrap();
    }
    let regex_defs = extract_regex_rule_defs(&raw_xml);

    let file = BufReader::new(file);

    let sanitized = preprocess::sanitize(file, &["suggestion"]);
    let rules = preprocess::extract_rules(sanitized.as_bytes());

    let mut unifications = Vec::new();

    let rules: Vec<_> = rules
        .into_iter()
        .map(|(xml, category)| {
            let mut out = Vec::new();

            let deseralized = RuleContainer::deserialize(&mut serde_xml_rs::Deserializer::new(
                EventReader::new(xml.as_bytes()),
            ));

            out.extend(match deseralized {
                Ok(rule_container) => match rule_container {
                    RuleContainer::Rule(rule) => {
                        vec![Ok((rule, None, category))]
                    }
                    RuleContainer::RuleGroup(rule_group) => flatten_group!(rule_group, category)
                        .into_iter()
                        .map(Ok)
                        .collect(),
                    RuleContainer::Unification(unification) => {
                        unifications.push(unification);

                        vec![]
                    }
                },
                Err(err) => {
                    log::warn!(
                        "rule chunk failed to deserialize: {} | chunk: {}",
                        err,
                        &xml.chars().take(2500).collect::<String>()
                    );
                    vec![Err(err)]
                }
            });
            out
        })
        .flatten()
        .collect();

    (
        rules
            .into_iter()
            .map(|result| match result {
                Ok(mut x) => {
                    x.0.unifications = Some(unifications.clone());

                    Ok(x)
                }
                Err(x) => Err(x),
            })
            .collect(),
        regex_defs,
    )
}

pub fn read_disambiguation_rules<P: AsRef<std::path::Path>>(
    path: P,
) -> Vec<Result<DisambiguationRuleReading, serde_xml_rs::Error>> {
    let file = File::open(path.as_ref()).unwrap();
    let file = BufReader::new(file);

    let sanitized = preprocess::sanitize(file, &[]);
    let rules = preprocess::extract_rules(sanitized.as_bytes());

    let mut unifications = Vec::new();

    let rules: Vec<_> = rules
        .into_iter()
        .map(|(xml, _)| {
            let mut out = Vec::new();

            let deseralized = DisambiguationRuleContainer::deserialize(
                &mut serde_xml_rs::Deserializer::new(EventReader::new(xml.as_bytes())),
            );

            let category: Option<Category> = None;

            out.extend(match deseralized {
                Ok(rule_container) => match rule_container {
                    DisambiguationRuleContainer::Rule(rule) => {
                        vec![Ok((rule, None, category))]
                    }
                    DisambiguationRuleContainer::RuleGroup(rule_group) => {
                        flatten_group!(rule_group, category)
                            .into_iter()
                            .map(Ok)
                            .collect()
                    }
                    DisambiguationRuleContainer::Unification(unification) => {
                        unifications.push(unification);

                        vec![]
                    }
                },
                Err(err) => vec![Err(err)],
            });
            out
        })
        .flatten()
        .collect();

    rules
        .into_iter()
        .map(|result| match result {
            Ok(mut x) => {
                x.0.unifications = Some(unifications.clone());

                Ok(x)
            }
            Err(x) => Err(x),
        })
        .collect()
}

#[cfg(test)]
mod regex_deser_tests {
    use super::*;

    #[test]
    fn regexp_element_with_text_child() {
        let xml = r#"<regexp mark="1" type="exact"><text text="abc"/></regexp>"#;
        let mut de = serde_xml_rs::Deserializer::new_from_reader(xml.as_bytes());
        let r = Regex::deserialize(&mut de);
        assert!(r.is_ok(), "failed: {:?}", r.err());
        assert_eq!(r.unwrap().text.to_string(), "abc");
    }

    #[test]
    fn rule_with_regexp_only() {
        let xml = r#"<rule id="DOUBLES_ESPACES" name="Deux espaces">
            <regexp mark="1" type="exact"><text text="abc"/></regexp>
            <message><text text="msg"/></message>
            <example type="incorrect"><marker><text text="a  b"/></marker></example>
        </rule>"#;
        let mut de = serde_xml_rs::Deserializer::new_from_reader(xml.as_bytes());
        let r = Rule::deserialize(&mut de);
        assert!(r.is_ok(), "failed: {:?}", r.err().map(|e| e.to_string()));
    }
}

#[cfg(test)]
mod read_rules_tests {
    use super::read_rules;

    #[test]
    fn nom_agreement_group_subrules() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../data2/fr/grammar.xml");
        if !path.exists() {
            return; // data dir not present in some environments
        }
        let rules = read_rules(path);
        let n = rules
            .iter()
            .filter_map(|r| r.as_ref().ok())
            .filter(|(_, group, _)| {
                group.as_ref().map_or(false, |g| g.id == "NOM_AGREEMENT")
            })
            .count();
        eprintln!("NOM_AGREEMENT subrules parsed: {}", n);
        assert!(n >= 5, "expected 5 subrules, got {}", n);
    }
}

/// Extract rule-level `<regexp>` rules (LT regex-on-text rules, e.g. de
/// `GENANT_SPELLING_RULE`, gl `UNITS_OF_MEASURE_SPACING`) from the RAW
/// grammar XML. Text nodes in the sanitized/re-indented chunks are polluted
/// with indentation whitespace, so this reads the original file. The
/// pattern pipeline is left untouched: its chunks still contain the
/// `<regexp>` children, which fail structure deserialization there exactly
/// as before (they are skipped with a counted warning).
pub fn extract_regex_rule_defs(raw_xml: &str) -> Vec<crate::rule::regex_rule::RegexRuleDef> {
    use crate::rule::regex_rule::{RegexRuleDef, RegexSugPart, ReplacePart};
    use crate::utils::regex::Regex;

    let doc = match roxmltree::Document::parse(raw_xml) {
        Ok(doc) => doc,
        Err(_) => return Vec::new(),
    };

    // split a literal with \N regex-group backrefs into parts
    fn lit_with_backrefs(s: &str) -> Vec<RegexSugPart> {
        let mut parts = Vec::new();
        let mut lit = String::new();
        let mut chars = s.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\\' {
                if let Some(&d) = chars.peek() {
                    if d.is_ascii_digit() {
                        chars.next();
                        if !lit.is_empty() {
                            parts.push(RegexSugPart::Lit(std::mem::take(&mut lit)));
                        }
                        parts.push(RegexSugPart::Group(d.to_digit(10).unwrap() as usize));
                        continue;
                    }
                }
            }
            lit.push(c);
        }
        if !lit.is_empty() {
            parts.push(RegexSugPart::Lit(lit));
        }
        parts
    }

    // `$k` replacement string -> parts
    fn replace_parts(s: &str) -> Vec<ReplacePart> {
        let mut parts = Vec::new();
        let mut lit = String::new();
        let mut chars = s.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '$' {
                if let Some(&d) = chars.peek() {
                    if d.is_ascii_digit() {
                        chars.next();
                        let mut n = d.to_digit(10).unwrap() as usize;
                        while n < 10 {
                            match chars.peek() {
                                Some(&d2) if d2.is_ascii_digit() => {
                                    n = n * 10 + d2.to_digit(10).unwrap() as usize;
                                    chars.next();
                                }
                                _ => break,
                            }
                        }
                        if !lit.is_empty() {
                            parts.push(ReplacePart::Lit(std::mem::take(&mut lit)));
                        }
                        parts.push(ReplacePart::Ref(n));
                        continue;
                    }
                }
            }
            lit.push(c);
        }
        if !lit.is_empty() {
            parts.push(ReplacePart::Lit(lit));
        }
        parts
    }

    fn default_on(node: roxmltree::Node) -> bool {
        match node.attribute("default") {
            Some("on") | None => true,
            Some("off") | Some("temp_off") => false,
            // unknown values: treat as on, never panic the build
            Some(_) => true,
        }
    }

    fn has_picky(node: roxmltree::Node) -> bool {
        node.attribute("tags")
            .map(|x| x.split_whitespace().any(|t| t == "picky"))
            .unwrap_or(false)
    }

    // build one def from a <rule> element that has a direct <regexp> child
    fn def_from_rule(
        rule: roxmltree::Node,
        regexp: roxmltree::Node,
        source: String,
        enabled: bool,
    ) -> Option<RegexRuleDef> {
        let pattern = regexp.text().unwrap_or_default().trim().to_string();
        if pattern.is_empty() {
            return None;
        }
        let case_sensitive = regexp
            .attribute("case_sensitive")
            .map(|x| x == "yes" || x == "true")
            .unwrap_or(false);

        let converted = super::utils::from_java_regex(&pattern, case_sensitive, false).ok()?;
        let regex = Regex::new(converted);
        // a rule whose regex cannot compile would panic at runtime: skip it
        if regex.try_compile().is_err() {
            log::warn!("regex rule {} cannot compile, skipped", source);
            return None;
        }

        let mut suggestions = Vec::new();
        let mut message = String::new();
        if let Some(m) = rule
            .children()
            .find(|c| c.is_element() && c.tag_name().name() == "message")
        {
            for node in m.children() {
                if node.is_text() {
                    let t = node.text().unwrap_or_default();
                    // raw file: only meaningful text (skip pure indentation)
                    if !t.trim().is_empty() {
                        message.push_str(t.trim());
                    }
                } else if node.is_element() && node.tag_name().name() == "suggestion" {
                    let mut parts = Vec::new();
                    for child in node.children() {
                        if child.is_text() {
                            parts.extend(lit_with_backrefs(child.text().unwrap_or_default()));
                        } else if child.is_element() && child.tag_name().name() == "match" {
                            let no: usize = child
                                .attribute("no")
                                .and_then(|x| x.parse().ok())
                                .unwrap_or(1);
                            match (
                                child.attribute("regexp_match"),
                                child.attribute("regexp_replace"),
                            ) {
                                (Some(rm), Some(rr)) => {
                                    let inner = Regex::new(rm.to_string());
                                    if inner.try_compile().is_err() {
                                        log::warn!(
                                            "regex rule {}: match regexp_match cannot compile",
                                            source
                                        );
                                        continue;
                                    }
                                    parts.push(RegexSugPart::GroupMatch {
                                        group: no,
                                        regex: inner,
                                        replace: replace_parts(rr),
                                    });
                                }
                                _ => parts.push(RegexSugPart::Group(no)),
                            }
                        }
                    }
                    suggestions.push(parts);
                }
            }
        }
        if suggestions.is_empty() {
            // no suggestion: LT still flags with no replacement
            suggestions.push(Vec::new());
        }

        Some(RegexRuleDef {
            source,
            regex,
            suggestions,
            message,
            enabled,
        })
    }

    let mut defs = Vec::new();

    for node in doc
        .root_element()
        .children()
        .filter(|c| c.is_element() && c.tag_name().name() == "category")
    {
        let category_id = node.attribute("id").unwrap_or("MISC").to_string();

        for group_or_rule in node
            .children()
            .filter(|c| c.is_element() && (c.tag_name().name() == "rulegroup" || c.tag_name().name() == "rule"))
        {
            if group_or_rule.tag_name().name() == "rule" {
                let regexp = match group_or_rule
                    .children()
                    .find(|c| c.is_element() && c.tag_name().name() == "regexp")
                {
                    Some(r) => r,
                    None => continue,
                };
                let rule_id = match group_or_rule.attribute("id") {
                    Some(id) => id.to_string(),
                    None => continue,
                };
                let enabled = default_on(group_or_rule) && !has_picky(group_or_rule);
                let source = format!("{}/{}/{}", category_id, rule_id, 0);
                if let Some(def) = def_from_rule(group_or_rule, regexp, source, enabled) {
                    defs.push(def);
                }
            } else {
                // rulegroup
                let group_id = group_or_rule.attribute("id").unwrap_or_default().to_string();
                let group_on = default_on(group_or_rule);
                let group_picky = has_picky(group_or_rule);
                let mut n = 0usize;
                for child in group_or_rule
                    .children()
                    .filter(|c| c.is_element() && c.tag_name().name() == "rule")
                {
                    let idx = n;
                    n += 1;
                    let regexp = match child
                        .children()
                        .find(|c| c.is_element() && c.tag_name().name() == "regexp")
                    {
                        Some(r) => r,
                        None => continue,
                    };
                    let sub_id = child.attribute("id").map(|x| x.to_string());
                    let enabled =
                        default_on(child) && group_on && !group_picky && !has_picky(child);
                    let source = match &sub_id {
                        Some(sub) => format!("{}/{}/{}", category_id, sub, 0),
                        None => format!("{}/{}/{}", category_id, group_id, idx),
                    };
                    if let Some(def) = def_from_rule(child, regexp, source, enabled) {
                        defs.push(def);
                    }
                }
            }
        }
    }

    defs
}
