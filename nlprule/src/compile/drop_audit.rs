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
fn es_quote_probe() {
    let tok = crate::Tokenizer::new("/home/alex/Documentos/Github/nlprule/nlprule-fork/storage/es_tokenizer.bin").unwrap();
    for sentence in tok.pipe("El dijo \"hola.") {
        for t in sentence.iter() {
            eprintln!("ESQ {:?}", t.word().as_str());
        }
    }
}

#[test]
fn unpaired_es_probe() {
    let tok = crate::Tokenizer::new("/home/alex/Documentos/Github/nlprule/nlprule-fork/storage/es_tokenizer.bin").unwrap();
    for text in ["El dijo \"hola.", "El dijo (hola."] {
        for sentence in tok.pipe(text) {
            let sugg = crate::builtins::unpaired_brackets(&sentence, Some("es"));
            eprintln!("UP {:?} → {:?}", text, sugg.iter().map(|s| s.source().to_string()).collect::<Vec<_>>());
        }
    }
}

#[test]
fn units_regex_probe() {
    let pat = std::fs::read_to_string("/tmp/units_regex.txt").unwrap();
    let pat = pat.trim();
    match crate::compile::utils::from_java_regex(pat, false, false) {
        Ok(s) => {
            println!("UNITS converted: {}", s);
            if let Ok(s2) = crate::compile::utils::from_java_regex(r"(\-?[0-9]+(°C|°F))\b", false, false) {
                println!("MINI ci: {}", s2);
            }
            if let Ok(s3) = crate::compile::utils::from_java_regex(r"(\-?[0-9]+(°C|°F))\b", true, false) {
                println!("MINI cs: {}", s3);
            }
            let raw = crate::utils::regex::Regex::new(pat.to_string());
            if raw.try_compile().is_ok() {
                println!("UNITS RAW (sin convertir) is_match: {}", raw.is_match("25°C"));
            } else {
                println!("UNITS RAW no compila");
            }
            let r = crate::utils::regex::Regex::new(s.clone());
            match r.try_compile() {
                Ok(_) => {
                    // test actual matching with the onig API used at runtime
                    println!("UNITS compiled ok, len={}", s.len());
                    let hits: Vec<_> = r.captures_iter("A temperatura é de 25°C.")
                        .filter_map(|c| c.get(0).map(|m| m.as_str().to_string()))
                        .collect();
                    println!("UNITS captures_iter: {:?}", hits);
                    for (name, pat, txt) in [
                        ("cls-deg", r"[°]", "°"),
                        ("deg-cC", r"[°][cC]", "°C"),
                        ("lit-degC", r"°C", "°C"),
                        ("num-degC", r"[0-9]+°C", "25°C"),
                        ("num-degC-b", r"[0-9]+°C\b", "25°C."),
                        ("grp", r"([0-9]+([°][cC]|[°][fF]))\b", "25°C."),
                        ("wordb-C-dot", r"C\b", "C."),
                        ("wordb-C-end", r"C\b", "C"),
                        ("degC-b-nodot", r"[0-9]+°C\b", "25°C"),
                        ("num-C-b", r"[0-9]+C\b", "25C."),
                        ("deg-C-b", r"°C\b", "°C."),
                        ("num-deg-C-b", r"[0-9]+°C\b", "x25°C."),
                    ] {
                        let rr = crate::utils::regex::Regex::new(pat.to_string());
                        let ok = rr.try_compile().is_ok();
                        println!("BISECT {}: compile={} match={}", name, ok, if ok { rr.is_match(txt) } else { false });
                    }
                }
                Err(e) => println!("UNITS compile err {:?}", e),
            }
        }
        Err(e) => println!("UNITS conv err {:?}", e),
    }
}
