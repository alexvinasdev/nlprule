//! Runs the rule set over sentences read from stdin (one per line) and
//! prints one JSON object per line: the rule IDs and spans that fired.
//! Used by `build/compare_server.py` to compare against the official
//! LanguageTool HTTP server.

use nlprule::{Rules, Tokenizer};
use std::io::{self, BufRead, Write};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let tokenizer = Tokenizer::new(&args[1]).unwrap();
    let rules = Rules::new(&args[2]).unwrap();

    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut out = io::BufWriter::new(stdout.lock());

    for (i, line) in stdin.lock().lines().enumerate() {
        let text = line.unwrap();
        let mut hits: Vec<(String, usize, usize, String)> = Vec::new();
        for sentence in tokenizer.pipe(&text) {
            for suggestion in rules.apply(&sentence) {
                // nlprule id format: CATEGORY/RULE_OR_GROUP/N -> LT id: RULE_OR_GROUP
                let id = suggestion
                    .source()
                    .split('/')
                    .nth(1)
                    .unwrap_or(suggestion.source())
                    .to_string();
                hits.push((
                    id,
                    suggestion.span().byte().start,
                    suggestion.span().byte().end,
                    suggestion.replacements().first().cloned().unwrap_or_default(),
                ));
            }
        }
        serde_json::ser::to_writer(
            &mut out,
            &serde_json::json!({ "i": i, "text": text, "hits": hits }),
        )
        .unwrap();
        writeln!(out).unwrap();
    }
}
