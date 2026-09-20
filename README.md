<h1 align='center'>
  nlprule
</h1>

<p align='center'>
    <a href="https://pypi.org/project/nlprule">
        <img src="https://img.shields.io/pypi/v/nlprule" alt="PyPI">
    </a>
    <a href="https://crates.io/crates/nlprule">
        <img src="https://img.shields.io/crates/v/nlprule" alt="Crates.io">
    </a>
    <a href="https://docs.rs/nlprule">
        <img src="https://docs.rs/nlprule/badge.svg" alt="Docs.rs">
    </a>
    <a href="https://pepy.tech/project/nlprule">
        <img src="https://pepy.tech/badge/nlprule/month" alt="PyPI Downloads">
    </a>
    <a href="">
        <img src="https://img.shields.io/crates/l/nlprule" alt="License">
    </a>
</p>

A fast, low-resource Natural Language Processing and Error Correction library written in Rust. nlprule implements a rule- and lookup-based approach to NLP using resources from [LanguageTool](https://github.com/languagetool-org/languagetool).

<details>
  <summary>Python Usage</summary>

Install: `pip install nlprule`

Use:
```python
from nlprule import Tokenizer, Rules

tokenizer = Tokenizer.load("en")
rules = Rules.load("en", tokenizer)
```
```python
rules.correct("He wants that you send him an email.")
# returns: 'He wants you to send him an email.'

rules.correct("I can due his homework.")
# returns: 'I can do his homework.'

for s in rules.suggest("She was not been here since Monday."):
    print(s.start, s.end, s.replacements, s.source, s.message)
# prints:
# 4 16 ['was not', 'has not been'] WAS_BEEN.1 Did you mean was not or has not been?
```
```python
for sentence in tokenizer.pipe("A brief example is shown."):
    for token in sentence:
        print(
            repr(token.text).ljust(10),
            repr(token.span).ljust(10),
            repr(token.tags).ljust(24),
            repr(token.lemmas).ljust(24),
            repr(token.chunks).ljust(24),
        )
# prints:
# 'A'        (0, 1)     ['DT']                   ['A', 'a']               ['B-NP-singular']       
# 'brief'    (2, 7)     ['JJ']                   ['brief']                ['I-NP-singular']       
# 'example'  (8, 15)    ['NN:UN']                ['example']              ['E-NP-singular']       
# 'is'       (16, 18)   ['VBZ']                  ['be', 'is']             ['B-VP']                
# 'shown'    (19, 24)   ['VBN']                  ['show', 'shown']        ['I-VP']                
# '.'        (24, 25)   ['.', 'PCT', 'SENT_END'] ['.']                    ['O']
```
</details>

<details>
  <summary>Rust Usage</summary>

Recommended setup:

`Cargo.toml`
```toml
[dependencies]
nlprule = "<version>"

[build-dependencies]
nlprule-build = "<version>" # must be the same as the nlprule version!
```

`build.rs`
```rust
fn main() -> Result<(), nlprule_build::Error> {
    println!("cargo:rerun-if-changed=build.rs");

    nlprule_build::BinaryBuilder::new(
        &["en"],
        std::env::var("OUT_DIR").expect("OUT_DIR is set when build.rs is running"),
    )
    .build()?
    .validate()
}
```

`src/main.rs`
```rust
use nlprule::{Rules, Tokenizer, tokenizer_filename, rules_filename};

fn main() {
    let mut tokenizer_bytes: &'static [u8] = include_bytes!(concat!(
        env!("OUT_DIR"),
        "/",
        tokenizer_filename!("en")
    ));
    let mut rules_bytes: &'static [u8] = include_bytes!(concat!(
        env!("OUT_DIR"),
        "/",
        rules_filename!("en")
    ));

    let tokenizer = Tokenizer::from_reader(&mut tokenizer_bytes).expect("tokenizer binary is valid");
    let rules = Rules::from_reader(&mut rules_bytes).expect("rules binary is valid");

    assert_eq!(
        rules.correct("She was not been here since Monday.", &tokenizer),
        String::from("She was not here since Monday.")
    );
}
```

`nlprule` and `nlprule-build` versions are kept in sync.

</details>

## Main features

- Rule-based Grammatical Error Correction through multiple thousand rules.
- A text processing pipeline doing sentence segmentation, part-of-speech tagging, lemmatization, chunking and disambiguation.
- Support for **35 languages** built from LanguageTool 6.5 resources (see table below).
- Morphological synthesis: `match` elements with `postag` / `postag_regexp` / `postag_replace`
  inflect lemmas in suggestions, mirroring LanguageTool's `MatchState`.
- Spellchecking. (*in progress*)

## Goals

- A single place to apply spellchecking and grammatical error correction for a downstream task.
- Fast, low-resource NLP suited for running:
    1. as a pre- / postprocessing step for more sophisticated (i. e. ML) approaches.
    2. in the background of another application with low overhead.
    3. client-side in the browser via WebAssembly.
- 100% Rust code and dependencies.

## Comparison to LanguageTool

All languages are built from a LanguageTool 6.5 desktop distribution with
`build/make_build_dirs.py` + `build/make_configs.py`; per-language binaries are compiled with
`scripts/compile_all.sh`. "XML rules" counts `<rule>`/`<rulegroup>` entries in `grammar.xml`
(expanded rules count higher); "passing" counts rules whose embedded LT examples
(`test --rules ...`) pass. Serbian rules are Java classes in LT (no XML to convert);
Japanese/Chinese need LT's specialized tokenizers; `da`/`km`/`crh` suffer from incomplete
tagger dictionaries in the LT distribution itself.

| lang           | runnable   | passing   | lang   | runnable   | passing   |
|----------------|------------|-----------|--------|------------|-----------|
| ar             | 965        | 822       | km     | 44         | 11        |
| ast            | 71         | 69        | lt     | 4          | 4         |
| be             | 516        | 506       | ml     | 18         | 18        |
| br | 665 | 476       | nl     | 1544       | 1492      |
| ca | 14162 | 10663     | pl | 1842 | 1491      |
| crh            | 93         | 17        | pt | 1969 | 953       |
| da             | 78         | 61        | ro     | 1167       | 1116      |
| de | 5477 | 5190      | ru | 1169 | 983       |
| de-DE-x-simple | 92         | 40        | sk     | 206        | 184       |
| el             | 98         | 95        | sl     | 85         | 82        |
| en | 5703 | 4858      | sr     | 45         | 44        |
| eo | 416 | 127       | sv     | 31         | 29        |
| es | 1797 | 1404      | ta     | 210        | 210       |
| fa             | 764        | 557       | tl     | 44         | 37        |
| fr | 4951 | 3137      | uk | 10915 | 10682     |
| ga             | 3566       | 3292      | zh     | 1863       | 1520      |
| gl | 298 | 204       | it | 131 | 124       |
| ja             | 735        | 702       |        |            |           |

Totals: ~53,000 runnable rules, ~44,200 passing their embedded LanguageTool
examples (~83%). Highlights: uk 97.9%, be 98.1%, nl 96.6%, sr 97.8%, de 94.9%,
ja 95.5% (lindera/ipadic), ro 95.6%, ta 100%, ga 92.3%, zh 81.6%, ru 84%,
ar 85.2%, es 78.1%, ca 75.3%.

### Live comparison against the LanguageTool HTTP server

`build/compare_server.py` sends each language's embedded example sentences
(unique, up to 1500 per language) to a local instance of the official
LanguageTool 6.5 HTTP server and to nlprule, and compares the rule IDs that
fire per sentence (standard level, ids normalized to LT's sub-rule ids):

| lang | sentences | only nlprule | only LT | ID precision vs LT | ID recall vs LT | Jaccard |
|------|-----------|--------------|---------|--------------------|----------------|---------|
| de   | 1500      | 55           | 161     | 0.92               | 0.80           | 0.89    |
| en   | 1500      | 50           | 397     | 0.92               | 0.59           | 0.74    |
| ru   | 1046      | 68           | 243     | 0.91               | 0.73           | 0.77    |
| ar   | 616       | 32           | 228     | 0.84               | 0.42           | 0.63    |
| es   | 1500      | 115          | 219     | 0.87               | 0.78           | 0.86    |
| pt   | 1500      | 78           | 488     | 0.89               | 0.57           | 0.68    |
| uk   | 1457      | 170          | 409     | 0.81               | 0.64           | 0.72    |
| fr   | 1500      | 185          | 448     | 0.72               | 0.51           | 0.70    |
| ca   | 1500      | 311          | 493     | 0.59               | 0.48           | 0.66    |

nlprule rarely fires rules that LT does not fire (precision 0.72-0.92). The
recall gap is dominated by rules that are pure Java classes in LT with no XML
representation (spellcheckers, `pl`'s stemmer-based rules, `en`'s chunker
rules) and by the deep linguistic filters listed below.

Language-specific notes:
- `ja` uses lindera with the ipadic dictionary (the same dictionary data LT uses
  via Sen/Gosen); enable the `ja` cargo feature
- `zh` uses jieba segmentation (`zh` feature) loaded with HanLP's own
  dictionaries (the mini CoreNatureDictionary + CustomDictionary, recovered
  from the double-array tries inside `hanlp.jar` — see
  `nlprule/configs/zh/hanlp_dict.txt`), so segmentation matches LT's HanLP
  setup closely (81.6% of examples pass; remaining gaps are HanLP's n-gram
  disambiguation and unknown-word heuristics)
- SimpleReplaceRule data tables (`replace*.txt`) are converted to XML rules
  automatically; tables larger than 10k lines are skipped (nl's 66k-line table
  does not fit the per-rule model)
- `sr` rules come from LT master (ekavian variant) plus its replace tables
- `da`/`km`/`crh`/`eo`/`fa` have incomplete tagger data in LT itself
- Java `RuleFilter` classes ported to runtime filters
  (`nlprule/src/rule/filter_java.rs`, data in `filter_data.rs`): the whole date
  family (DateCheck/FutureDate/NewYear/YMD/DMY/RecentYear/DateRange/... in 14
  languages), MultitokenSpeller (+ per-language word lists),
  *SuppressMisspelledSuggestions, FindSuggestions (en/fr/es/ca, with
  morfologik's frequency-ranked, diacritics-insensitive suggestion order),
  AdvancedSynthesizer, PartialPosTag (ru/ga/en), AddCommas, OrdinalSuffix,
  CompoundCheck, nl CompoundFilter, INN, DecadeSpelling, UppercaseNounReading,
  ConvertToSentenceCase, ConfusionCheck (es/pt), DiacriticsCheck (ca),
  RomanNumeral, RegularIrregularParticiple, ValidWord,
  RemoveUnknownCompounds, WhitespaceCheck, plus the English `+DT`/`+INDT`
  determiner synthesis of `EnglishSynthesizer`
- Also ported: ca's pronoun/verb-morphology filters (AdjustPronouns,
  AdjustVerbSuggestions, AnarA, DonarTemps, Oblidarse, PortarGerundi,
  PortarTemps, PronomsFeblesHelper with the full weak-pronoun transformation
  tables), the Catalan synthesizer on its valencia dictionary with the
  regional verb-variant fallback and PostagComparator,
  `Catalan.adaptSuggestion`, ar MasdarToVerb/VerbToMafoulMutlaq/
  AdjectiveToExclamation/ArabicDateCheck (with ArabicTagManager flag
  arithmetic), en AdverbFilter, the disambiguation `addchunk` action (GV /
  PTime chunks for ca+es), and `IsEnglishWordFilter` (approximated with a
  frequency list of the 20k most common English words)
- Still unported: `en`'s chunker (4.8k chunk references in `grammar.xml` need
  a port of LT's OpenNLP chunker models; the runtime chunk matcher exists),
  ar ArabicNumberPhraseFilter (needs ArabicNumbersWords), pt
  BrazilianToponymFilter (LT `<regexp>` rules are a separate rule type),
  WordWithDeterminer's `suggestionHasNoErrors` re-validation, and rules that
  are pure Java in LT with no XML (spellers, pl stemmer rules)

### Performance vs the LanguageTool server

`build/bench_server.py` (150 example sentences per language, same machine,
single thread; LT via its local HTTP server, nlprule in-process):

| lang | nlprule p50/sentence | nlprule p95 | nlprule throughput | LT server p50 | LT server p95 | LT throughput | nlprule RSS | LT JVM RSS |
|------|----------------------|-------------|--------------------|---------------|---------------|---------------|-------------|------------|
| en   | 1 ms                 | 3 ms        | 611 sents/s        | 11 ms         | 15 ms         | 63 sents/s    | 231 MB      | ~2.5 GB    |
| de   | 1 ms                 | 3 ms        | 758 sents/s        | 17 ms         | 32 ms         | 53 sents/s    | 353 MB      | ~2.5 GB    |
| fr   | 1 ms                 | 3 ms        | 625 sents/s        | 1949 ms       | 2272 ms       | 0.5 sents/s   | 293 MB      | ~2.5 GB    |
| es   | 1 ms                 | 2 ms        | 178 sents/s*       | 12 ms         | 42 ms         | 64 sents/s    | 1.2 GB      | ~2.5 GB    |
| ca   | 2 ms                 | 5 ms        | 375 sents/s        | 29 ms         | 35 ms         | 34 sents/s    | 414 MB      | ~2.5 GB    |

nlprule loads its binaries in 0.6-3.1 s per language and is 10-4000x faster per
sentence. (*`es` mean is 5.6 ms: sentences that hit the FindSuggestions
full-scan fallback over the 3.4M-form speller dominate; p50 is 1 ms.)
nlprule fires more matches than the server because it applies rules that LT
disables by default (`default="off"`).

With the original LT 5.2-based build directories, English passes 4192/4226 (99.2%) of its
example tests and German 3799/3903 (97.3%).

See the [benchmark issue](https://github.com/bminixhofer/nlprule/issues/6) for details.

## Projects using nlprule

- [prosemd](https://github.com/kitten/prosemd-lsp): a proofreading and linting language server for markdown files with VSCode integration.
- [cargo-spellcheck](https://github.com/drahnr/cargo-spellcheck): a tool to check all your Rust documentation for spelling and grammar mistakes.

Please submit a PR to add your project!

## Acknowledgements

All credit for the resources used in nlprule goes to [LanguageTool](https://github.com/languagetool-org/languagetool) who have made a Herculean effort to create high-quality resources for Grammatical Error Correction and broader NLP.

## License

nlprule is licensed under the MIT license or Apache-2.0 license, at your option.

The nlprule binaries (`*.bin`) are derived from LanguageTool v5.2 and licensed under the LGPLv2.1 license. nlprule statically and dynamically links to these binaries. Under LGPLv2.1 §6(a) this does not have any implications on the license of nlprule itself.
