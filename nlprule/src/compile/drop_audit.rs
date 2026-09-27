//! Fast XML drop audit (dev tool): runs the structure pipeline over every
//! data2 grammar.xml and prints which rules fail to deserialize or fail
//! regex extraction — without the expensive tagger/synthesizer work.
//!
//! Run with:
//!   RUST_LOG=warn cargo test --release -p nlprule drop_audit -- --nocapture

use super::structure;

#[test]
fn drop_audit() {
    let root = match std::env::var("DROP_AUDIT_ROOT") {
        Ok(x) => std::env::temp_dir().join(x).display().to_string(), // unused branch guard
        Err(_) => "/home/alex/Documentos/Github/nlprule/data2".to_string(),
    };
    let root = std::env::var("DROP_AUDIT_ROOT")
        .unwrap_or_else(|_| "/home/alex/Documentos/Github/nlprule/data2".to_string());

    struct StderrLog;
    impl log::Log for StderrLog {
        fn enabled(&self, md: &log::Metadata) -> bool {
            md.level() <= log::Level::Warn
        }
        fn log(&self, record: &log::Record) {
            if self.enabled(record.metadata()) {
                eprintln!("[{}] {}", record.level(), record.args());
            }
        }
        fn flush(&self) {}
    }
    let _ = log::set_boxed_logger(Box::new(StderrLog));
    log::set_max_level(log::LevelFilter::Warn);

    let mut langs: Vec<String> = std::fs::read_dir(&root)
        .expect("read data2 root")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir() && p.join("grammar.xml").exists())
        .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
        .collect();
    langs.sort();

    let mut total_err = 0usize;
    let mut total_regex = 0usize;

    for lang in &langs {
        let path = std::path::Path::new(&root).join(lang).join("grammar.xml");
        let (rules, regex_defs) = structure::read_rules(&path);
        let n_err = rules.iter().filter(|r| r.is_err()).count();
        total_err += n_err;
        total_regex += regex_defs.len();

        if let Ok(find) = std::env::var("DROP_AUDIT_FIND") {
            for r in rules.iter().filter_map(|r| r.as_ref().ok()) {
                let has_group = r
                    .1
                    .as_ref()
                    .map_or(false, |g| g.id.contains(&find));
                if r.0.id.as_deref().map_or(false, |i| i.contains(find.as_str()))
                    || has_group
                {
                    eprintln!(
                        "   FIND {lang}: rule id={:?} group={:?} name={:?}",
                        r.0.id,
                        r.1.as_ref().map(|g| g.id.clone()),
                        r.0.name
                    );
                }
            }
        }

        eprintln!(
            "== {lang}: {} ok, {n_err} ERR, {} regex defs",
            rules.len() - n_err,
            regex_defs.len()
        );
        for e in rules.iter().filter(|r| r.is_err()) {
            if let Err(x) = e {
                eprintln!("   ERR {x}");
            }
        }
    }

    eprintln!("TOTAL deserialize errors: {total_err}, regex defs: {total_regex}");
}

#[test]

#[test]
fn regex_probe2() {
    for (name, pat) in [
        ("alt-plain", r"(?<!a|bc)"),
        ("alt-group", r"(?<!([a-vyz]|[a-vyz]\d))"),
        ("alt-group-quant", r"(?<!([a-vyz]|[a-vyz]\d{2}))"),
        ("full-lb", r"(?<!([a-vyz]|[a-vyz]\d|[a-vyz]\d{2}|[a-vyz]\d{3}|[a-vyz]\d{4}|[a-vyz]\d{5}))x"),
        ("la-neg", r"((?!abc)(\d+)x)"),
    ] {
        let r = crate::utils::regex::Regex::new(pat.to_string());
        match r.try_compile() {
            Ok(_) => println!("{}: OK", name),
            Err(e) => println!("{}: ERR {:?}", name, e),
        }
    }
}

#[test]
fn regex_probe() {
    let pattern = "(?<!([a-vyz]|[a-vyz]\\d|[a-vyz]\\d{2}|[a-vyz]\\d{3}|[a-vyz]\\d{4}|[a-vyz]\\d{5}))((?!(?:[,;\\(\\.][x\\*]([\\d\\.,\u{207b}\u{00b9}\u{00b2}\u{00b3}\u{2074}\u{2075}\u{2076}\u{2077}\u{2078}\u{2079}\u{2070}]+?)|(?:[\\d\\.,\u{207b}\u{00b9}\u{00b2}\u{00b3}\u{2074}\u{2075}\u{2076}\u{2077}\u{2078}\u{2079}\u{2070}]+?)[x\\*][,;\\.\\)\\!\\?]))([\\d\\.,\u{207b}\u{00b9}\u{00b2}\u{00b3}\u{2074}\u{2075}\u{2076}\u{2077}\u{2078}\u{2079}\u{2070}]+?)[x\\*]([\\d\\.,\u{207b}\u{00b9}\u{00b2}\u{00b3}\u{2074}\u{2075}\u{2076}\u{2077}\u{2078}\u{2079}\u{2070}]+))";
    let conv = crate::compile::utils::from_java_regex(pattern, false, false);
    match conv {
        Ok(s) => {
            println!("CONVERTED: {}", s);
            let r = crate::utils::regex::Regex::new(s);
            match r.try_compile() {
                Ok(_) => println!("COMPILES OK"),
                Err(e) => println!("COMPILE ERR: {:?}", e),
            }
        }
        Err(e) => println!("CONV ERR: {:?}", e),
    }
}

#[test]
fn regex_probe3() {
    for (name, pat) in [
        ("islatin", r"\p{IsLatin}{2,30}"),
        ("brace-possessive", r"\p{Lu}{2}+[i]*\p{Lu}+"),
        ("brace-possessive2", r"[\p{L}&&[^\p{Lu}]]{1,4}+"),
        ("octal-class", r#"[?.!,:;\"\02]"#),
    ] {
        match crate::compile::utils::from_java_regex(pat, false, false) {
            Ok(s) => {
                let r = crate::utils::regex::Regex::new(s);
                match r.try_compile() {
                    Ok(_) => println!("{}: OK", name),
                    Err(e) => println!("{}: COMPILE ERR {:?}", name, e),
                }
            }
            Err(e) => println!("{}: CONV ERR {:?}", name, e),
        }
    }
}

#[test]
fn regex_probe4() {
    for (name, pat, cs, fm) in [
        ("postag-prp", r"PRP$?", true, true),
        ("inline-i", r"(?i)id", false, false),
        ("inline-id", r"(?id)egos?", false, false),
        ("surrogate", r#"[\ud83c\udc00-\ud83c\udfff]+"#, false, false),
    ] {
        match crate::compile::utils::from_java_regex(pat, cs, fm) {
            Ok(s) => {
                let r = crate::utils::regex::Regex::new(s.clone());
                match r.try_compile() {
                    Ok(_) => println!("{}: OK → {}", name, s),
                    Err(e) => println!("{}: COMPILE ERR {:?} → {}", name, e, s),
                }
            }
            Err(e) => println!("{}: CONV ERR {:?}", name, e),
        }
    }
}

#[test]
fn regex_probe5() {
    let pat = std::fs::read_to_string("/tmp/lp_regex.txt").unwrap();
    let pat = pat.trim();
    match crate::compile::utils::from_java_regex(pat, true, true) {
        Ok(s) => {
            let r = crate::utils::regex::Regex::new(s.clone());
            match r.try_compile() {
                Ok(_) => println!("lp: OK (len {})", s.len()),
                Err(e) => {
                    println!("lp: COMPILE ERR {:?}", e);
                    // find suspect escapes in output
                    for (i, c) in s.char_indices() {
                        if c == '\\' {
                            let seg: String = s[i..].chars().take(8).collect();
                            if seg.contains('x') || seg.contains('u') { println!("  esc at {}: {:?}", i, seg); }
                        }
                    }
                }
            }
        }
        Err(e) => println!("lp: CONV ERR {:?}", e),
    }
}
