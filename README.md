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

| lang | runnable | passing | lang | runnable | passing |
|------|----------|---------|------|----------|---------|
| ar   | 961      | 822     | km   | 44       | 11      |
| ast  | 71       | 69      | lt   | 4        | 4       |
| be   | 516      | 506     | ml   | 18       | 18      |
| br   | 661      | 475     | nl   | 1516     | 1471    |
| ca   | 13846    | 10227   | pl   | 1778     | 1460    |
| crh  | 93       | 17      | pt   | 1832     | 895     |
| da   | 78       | 61      | ro   | 1167     | 1116    |
| de   | 5400     | 5162    | ru   | 1147     | 975     |
| de-DE-x-simple | 92 | 40   | sk   | 206      | 184     |
| el   | 98       | 95      | sl   | 85       | 82      |
| en   | 5572     | 4066    | sr   | 45       | 44      |
| eo   | 415      | 126     | sv   | 31       | 29      |
| es   | 1159     | 896     | ta   | 210      | 210     |
| fa   | 764      | 557     | tl   | 44       | 37      |
| fr   | 4533     | 3018    | uk   | 10902    | 10672   |
| ga   | 3566     | 3292    | zh   | 1863     | 1207    |
| gl   | 220      | 146     | it   | 129      | 124     |
| ja   | 735      | 702     |      |          |         |

Totals: ~51,000 runnable rules, ~42,200 passing their embedded LanguageTool
examples (~83%). Highlights: uk 97.8%, be 98.1%, nl 97.1%, sr 97.8%, de 95.6%,
ja 95.5% (lindera/ipadic), ro 95.6%, ta 100%, ga 92.3%, ru 85%, ar 85.5%.

Language-specific notes:
- `ja` uses lindera with the ipadic dictionary (the same dictionary data LT uses
  via Sen/Gosen); enable the `ja` cargo feature
- `zh` uses jieba segmentation (`zh` feature); LT uses HanLP whose core
  dictionary is not freely extractable, so vocabulary-dependent rules differ
- SimpleReplaceRule data tables (`replace*.txt`) are converted to XML rules
  automatically; tables larger than 10k lines are skipped (nl's 66k-line table
  does not fit the per-rule model)
- `sr` rules come from LT master (ekavian variant) plus its replace tables
- `da`/`km`/`crh`/`eo`/`fa` have incomplete tagger data in LT itself
- Filters implemented at runtime: UnderlineSpaces, ApostropheType,
  RegexAntiPattern, AdaptSuggestions, *SuppressMisspelledSuggestions

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
