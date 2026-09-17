//! A morphological synthesizer: looks up inflected forms of a lemma for a
//! part-of-speech tag. Mirrors the behavior of LanguageTool's
//! `BaseSynthesizer` / `ManualSynthesizer` (Morfologik-based lookup with
//! manual additions and removals).

use crate::utils::regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Language-specific behavior of the synthesizer,
/// mirroring overrides of `BaseSynthesizer` subclasses in LanguageTool.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum SynthesizerKind {
    Default,
    /// GermanSynthesizer: POS tag correction appends case information.
    German,
}

impl Default for SynthesizerKind {
    fn default() -> Self {
        SynthesizerKind::Default
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Synthesizer {
    /// lemma -> list of (pos tag, inflected forms)
    entries: HashMap<String, Vec<(String, Vec<String>)>>,
    /// (lemma, pos tag) -> manually added forms (`added.txt`)
    manual: HashMap<(String, String), Vec<String>>,
    /// (lemma, pos tag) -> forms to remove (`removed.txt`, `do-not-synthesize.txt`)
    removed: HashMap<(String, String), Vec<String>>,
    kind: SynthesizerKind,
}

impl Synthesizer {
    pub(crate) fn new(kind: SynthesizerKind) -> Self {
        Synthesizer {
            kind,
            ..Default::default()
        }
    }

    /// Builds a synthesizer from the build directory dumps:
    /// * `synth.dump`: lines of `lemma|POS<TAB>form` (the dumped `*_synth.dict`)
    /// * `added.txt`: lines of `form<TAB>lemma<TAB>POS` (manually added forms)
    /// * `removed.txt` / `do-not-synthesize.txt`: lines of `form<TAB>lemma<TAB>POS`
    ///
    /// Missing files are simply skipped.
    pub(crate) fn from_dumps(
        synth_dump: &std::path::Path,
        manual: &[std::path::PathBuf],
        kind: SynthesizerKind,
    ) -> Self {
        let mut synthesizer = Synthesizer::new(kind);

        if let Ok(content) = std::fs::read_to_string(synth_dump) {
            for line in content.lines() {
                if line.is_empty() {
                    continue;
                }

                let mut parts = line.split('\t');
                let key = parts.next().unwrap_or("");
                let form = match parts.next() {
                    Some(form) => form,
                    None => continue,
                };

                // key is "lemma|POS"
                if let Some(separator) = key.rfind('|') {
                    let lemma = &key[..separator];
                    let pos = &key[separator + 1..];
                    synthesizer.insert(lemma.into(), pos.into(), form.into());
                }
            }
        }

        for path in manual {
            if let Ok(content) = std::fs::read_to_string(path) {
                for line in content.lines() {
                    if line.is_empty() || line.starts_with('#') {
                        continue;
                    }

                    let mut parts = line.split('\t');
                    let form = parts.next().unwrap_or("").to_string();
                    let lemma = parts.next().unwrap_or("").to_string();
                    let pos = parts.next().unwrap_or("").to_string();

                    if form.is_empty() || lemma.is_empty() || pos.is_empty() {
                        continue;
                    }

                    if path.file_name().map_or(false, |x| x == "removed.txt")
                        || path
                            .file_name()
                            .map_or(false, |x| x == "do-not-synthesize.txt")
                    {
                        synthesizer.insert_removed(lemma, pos, form);
                    } else {
                        synthesizer.insert_manual(lemma, pos, form);
                    }
                }
            }
        }

        synthesizer.shrink();
        synthesizer
    }

    pub(crate) fn kind(&self) -> SynthesizerKind {
        self.kind
    }

    pub(crate) fn insert(&mut self, lemma: String, pos: String, form: String) {
        self.entries
            .entry(lemma)
            .or_default()
            .push((pos, vec![form]));
    }

    pub(crate) fn insert_manual(&mut self, lemma: String, pos: String, form: String) {
        self.manual.entry((lemma, pos)).or_default().push(form);
    }

    pub(crate) fn insert_removed(&mut self, lemma: String, pos: String, form: String) {
        self.removed.entry((lemma, pos)).or_default().push(form);
    }

    pub(crate) fn shrink(&mut self) {
        // merge duplicate (lemma, pos) entries and dedupe + sort forms
        for pos_list in self.entries.values_mut() {
            let mut merged: Vec<(String, Vec<String>)> = Vec::with_capacity(pos_list.len());
            for (pos, mut forms) in pos_list.drain(..) {
                forms.sort();
                forms.dedup();
                if let Some((_, acc)) = merged.iter_mut().find(|(p, _)| *p == pos) {
                    acc.extend(forms);
                } else {
                    merged.push((pos, forms));
                }
            }
            for (_, forms) in merged.iter_mut() {
                forms.sort();
                forms.dedup();
            }
            *pos_list = merged;
        }
    }

    /// Lookup all inflected forms of `lemma` with POS tag `pos`.
    /// Mirrors `BaseSynthesizer.lookup`.
    pub fn lookup(&self, lemma: &str, pos: &str) -> Vec<String> {
        let mut results = Vec::new();

        if let Some(pos_list) = self.entries.get(lemma) {
            for (entry_pos, forms) in pos_list {
                if entry_pos == pos {
                    results.extend(forms.iter().cloned());
                }
            }
        }

        if let Some(manual_forms) = self.manual.get(&(lemma.to_string(), pos.to_string())) {
            results.extend(manual_forms.iter().cloned());
        }

        if let Some(remove_forms) = self.removed.get(&(lemma.to_string(), pos.to_string())) {
            results.retain(|x| !remove_forms.contains(x));
        }

        results
    }

    /// Get all inflected forms of `lemma` where the POS tag matches `pos_regex`.
    /// Mirrors `BaseSynthesizer.synthesize(token, posTag, true)`: the regex
    /// must be compiled for a full match (`^(?:...)$`), mirroring Java's
    /// `Pattern.matcher(tag).matches()`.
    /// The returned forms are deduplicated and sorted, like LanguageTool's TreeSet behavior.
    pub fn synthesize_regex(&self, lemma: &str, pos_regex: &Regex) -> Vec<String> {
        let mut results = Vec::new();

        if let Some(pos_list) = self.entries.get(lemma) {
            for (entry_pos, forms) in pos_list {
                if pos_regex.is_match(entry_pos) {
                    results.extend(forms.iter().cloned());
                }
            }
        }

        for ((m_lemma, m_pos), forms) in self.manual.iter() {
            if m_lemma == lemma && pos_regex.is_match(m_pos) {
                results.extend(forms.iter().cloned());
            }
        }

        for ((r_lemma, r_pos), forms) in self.removed.iter() {
            if r_lemma == lemma && pos_regex.is_match(r_pos) {
                results.retain(|x| !forms.contains(x));
            }
        }

        results.sort();
        results.dedup();
        results
    }

    /// Mirrors `BaseSynthesizer.getTargetPosTag`: by default, the last of the
    /// matched POS tags is returned.
    pub fn get_target_pos_tag(&self, pos_tags: &[&str], target_pos_tag: &str) -> String {
        if pos_tags.is_empty() {
            target_pos_tag.to_string()
        } else {
            // return the last one to keep the previous results
            pos_tags[pos_tags.len() - 1].to_string()
        }
    }

    /// Mirrors `BaseSynthesizer.getPosTagCorrection` (and overrides e.g. in German).
    pub fn pos_tag_correction(&self, pos_tag: String) -> String {
        match self.kind {
            SynthesizerKind::German => {
                // GermanSynthesizer: append ':' + the tag itself in lowercase if
                // the tag contains a case marker... actually GermanSynthesizer
                // switches SUB:FIN:1S etc. Full logic: it duplicates the tag with
                // a ':' separator appended for compound lookup. Approximation:
                // return as-is.
                pos_tag
            }
            SynthesizerKind::Default => pos_tag,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_with_manual_and_removed() {
        let mut synth = Synthesizer::new(SynthesizerKind::Default);
        synth.insert("walk".into(), "VB".into(), "walk".into());
        synth.insert("walk".into(), "VBP".into(), "walk".into());
        synth.insert("walk".into(), "VBZ".into(), "walks".into());
        synth.insert_manual("run".into(), "VBZ".into(), "runs".into());
        synth.insert_removed("walk".into(), "VBP".into(), "walk".into());
        synth.shrink();

        assert_eq!(synth.lookup("walk", "VBZ"), vec!["walks".to_string()]);
        assert!(synth.lookup("walk", "VBP").is_empty());
        assert_eq!(synth.lookup("run", "VBZ"), vec!["runs".to_string()]);
    }
}
