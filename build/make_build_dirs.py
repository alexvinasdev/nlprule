"""Generate nlprule build directories for all LanguageTool languages.

Works from a LanguageTool desktop distribution (unzipped) which contains:
- loose resources under org/languagetool/{rules,resource}/
- POS dictionary jars under libs/

Requires Java (11+) on PATH or via --java to run the DictionaryExporter.

Usage:
    python make_build_dirs.py --lt_dir /path/to/LanguageTool-6.5 --out_root ../data
"""

from argparse import ArgumentParser
from pathlib import Path
from shutil import copyfile
from subprocess import run
from tempfile import TemporaryDirectory
from urllib.request import urlopen
from zipfile import ZipFile
import json
import os
import re
import logging

import lxml.etree as ET
import wordfreq

LT_MASTER = "https://raw.githubusercontent.com/languagetool-org/languagetool/master"

# analyzer + synth dict locations: ('loose', filename_stem) or ('jar', jar, entry)
DICT_SOURCES = {
    "ar": ("loose", "arabic"),
    "ast": ("jar", "asturian-pos-dict.jar", "asturian"),
    "br": ("loose", "breton"),
    "ca": ("jar", "catalan-pos-dict.jar", "ca-ES"),
    "da": ("loose", "danish"),
    "de": ("jar", "german-pos-dict.jar", "german"),
    "de-DE-x-simple-language": ("jar", "german-pos-dict.jar", "german"),
    "el": ("loose", "greek"),
    "en": ("jar", "english-pos-dict.jar", "english"),
    "es": ("jar", "spanish-pos-dict.jar", "es-ES"),
    "fr": ("jar", "french-pos-dict.jar", "french"),
    "ga": ("jar", "languagetool-ga-dicts.jar", "irish"),
    "gl": ("loose", "galician"),
    "it": ("loose", "italian"),
    "km": ("loose", "khmer"),
    "nl": ("jar", "dutch-pos-dict.jar", "dutch"),
    "pl": ("loose", "polish"),
    "pt": ("jar", "portuguese-pos-dict.jar", "portuguese"),
    "ro": ("loose", "romanian"),
    "ru": ("loose", "russian"),
    "sk": ("loose", "slovak"),
    "sv": ("loose", "swedish"),
    "ta": ("loose", "tamil"),
    "tl": ("loose", "tagalog"),
    "uk": ("jar", "morfologik-ukrainian-lt.jar", "ukrainian"),
}

# languages whose grammar/disambiguation XML must be fetched from LT master
GITHUB_ONLY = {"lt", "ml", "sr"}

# wordfreq language codes where they differ from the LT code
WORDFREQ_CODES = {
    "de-DE-x-simple-language": "de",
    "tl": "fil",
}

URL_JOIN_REGEX = (
    r"(https?:\\/\\/(?:www\\.|(?!www))[a-zA-Z0-9][a-zA-Z0-9-]+[a-zA-Z0-9]"
    r"\\.[^\\s]{2,}|www\\.[a-zA-Z0-9][a-zA-Z0-9-]+[a-zA-Z0-9]\\.[^\\s]{2,}|"
    r"https?:\\/\\/(?:www\\.|(?!www))[a-zA-Z0-9]+\\.[^\\s]{2,}|"
    r"www\\.[a-zA-Z0-9]+\\.[^\\s]{2,})"
)

def sh(cmd, **kwargs):
    result = run(cmd, capture_output=True, text=True, **kwargs)
    if result.returncode != 0:
        logging.error("Command failed: %s\n%s", cmd, result.stderr[:2000])
    return result


def fetch_github(path: str, dest: Path):
    try:
        content = urlopen(f"{LT_MASTER}/{path}", timeout=60).read()
        dest.write_bytes(content)
        return True
    except Exception as e:  # noqa: BLE001
        logging.warning("Could not fetch %s: %s", path, e)
        return False


def canonicalize(src: Path, dest: Path):
    """Parse `src` (resolving external DTD entities like &foo; / %entities;) and
    write the canonicalized XML to `dest`."""
    parser = ET.XMLParser(load_dtd=True, resolve_entities=True, no_network=True, huge_tree=True)
    try:
        et = ET.parse(str(src), parser)
    except ET.XMLSyntaxError:
        et = ET.parse(str(src))
    et.write_c14n(open(dest, "wb"))


def write_freqlist(f, lang_code, top_n=1000):
    wf_code = WORDFREQ_CODES.get(lang_code, lang_code)
    try:
        words = wordfreq.top_n_list(wf_code, top_n)
    except Exception:  # noqa: BLE001
        words = []
    if not words:
        logging.warning("no wordfreq data for %s; common.txt will be minimal", lang_code)
    for word in words:
        f.write(word + "\n")
        f.write(word.title() + "\n")


def dump_dictionary(java, classpath, dict_path, info_path, out_path):
    result = sh(
        [
            java,
            "-cp",
            classpath,
            "org.languagetool.tools.DictionaryExporter",
            "-i",
            str(dict_path),
            "-info",
            str(info_path),
            "-o",
            str(out_path),
        ]
    )
    if result.returncode != 0:
        logging.error("DictionaryExporter failed for %s", dict_path)
        return False

    # dumped dictionaries are sometimes not in utf-8; re-encode defensively
    try:
        content = out_path.read_bytes().decode("utf-8")
    except UnicodeDecodeError:
        import chardet

        detection = chardet.detect(out_path.read_bytes()[:2_000_000])
        encoding = detection.get("encoding") or "latin-1"
        content = out_path.read_bytes().decode(encoding, errors="replace")
    out_path.write_text(content)
    return True


def extract_dict(jar_path, stem, resource_dir, workdir):
    """Extract {stem}.dict/.info for a language from its jar (or copy if loose)."""
    extracted = {}
    for suffix in (".dict", ".info"):
        name = f"{stem}{suffix}"
        loose = resource_dir / name
        target = workdir / name
        if loose.exists():
            copyfile(loose, target)
        else:
            with ZipFile(jar_path) as zf:
                entry = f"org/languagetool/resource/{resource_dir.name}/{name}"
                try:
                    target.write_bytes(zf.read(entry))
                except KeyError:
                    logging.warning("%s not found in %s", entry, jar_path)
                    continue
        extracted[suffix] = target
    return extracted


def literal_postags(xml_path):
    """All literal POS tags referenced via postag= attributes (ignoring regexes)."""
    if not xml_path.exists():
        return set()
    content = xml_path.read_text(encoding="utf-8", errors="replace")
    tags = set()
    for match in re.finditer(r'postag="([^"]*)"', content):
        tag = match.group(1)
        if tag and not re.search(r"[|?*+()\[\]{}.\\^$]", tag):
            tags.add(tag)
    return tags


def dump_tags(dump_path):
    """All POS tags present in an analyzer dump (3rd column)."""
    tags = set()
    if not dump_path.exists():
        return tags
    with open(dump_path, encoding="utf-8", errors="replace") as f:
        for line in f:
            parts = line.rstrip("\n").split("\t")
            if len(parts) >= 3 and parts[2]:
                tags.add(parts[2])
    return tags


def make_build_dir(lang, lt_dir, out_root, java, classpath, keep_going):
    dist_rules = lt_dir / "org" / "languagetool" / "rules" / lang
    resource_dir = lt_dir / "org" / "languagetool" / "resource" / lang

    out_dir = out_root / lang
    if out_dir.exists() and (out_dir / "lang_code.txt").exists() and not keep_going:
        logging.info("%s: build dir exists, skipping", lang)
        return True
    out_dir.mkdir(parents=True, exist_ok=True)
    tag_dir = out_dir / "tags"
    tag_dir.mkdir(exist_ok=True)

    # grammar.xml
    grammar = out_dir / "grammar.xml"
    if dist_rules.exists() and (dist_rules / "grammar.xml").exists():
        # some languages (uk, sk) split rules over several grammar-*.xml files
        # which LT loads by convention; merge them into one file
        extra_grammars = sorted(
            g
            for g in dist_rules.glob("grammar-*.xml")
            if "-l2-" not in g.name  # L2 variants (e.g. English for German speakers) are separate languages
        )

        if extra_grammars:
            merged = [
                '<?xml version="1.0" encoding="UTF-8"?>',
                '<rules lang="%s">' % lang,
            ]
            dtd_parser = ET.XMLParser(
                load_dtd=True, resolve_entities=True, no_network=True, huge_tree=True
            )

            def children_of(path):
                try:
                    root = ET.parse(str(path), dtd_parser).getroot()
                except ET.XMLSyntaxError:
                    content = path.read_bytes()
                    try:
                        root = ET.fromstring(content)
                    except ET.XMLSyntaxError:
                        # fragment without a root element: wrap it
                        root = ET.fromstring(b"<rules>" + content + b"</rules>")
                for child in root:
                    if isinstance(child.tag, str):
                        yield ET.tostring(child, encoding="unicode")

            merged.extend(children_of(dist_rules / "grammar.xml"))
            for extra in extra_grammars:
                merged.extend(children_of(extra))
            merged.append("</rules>")
            grammar.write_text("\n".join(merged), encoding="utf-8")
            merged_grammar = True
        else:
            copyfile(dist_rules / "grammar.xml", grammar)
            merged_grammar = False
    elif lang in GITHUB_ONLY:
        if not fetch_github(
            f"languagetool-language-modules/{lang}/src/main/resources/"
            f"org/languagetool/rules/{lang}/grammar.xml",
            grammar,
        ):
            logging.warning("%s: no grammar.xml", lang)
    else:
        logging.warning("%s: no grammar.xml", lang)

    # disambiguation.xml
    disambig = out_dir / "disambiguation.xml"
    if (resource_dir / "disambiguation.xml").exists():
        copyfile(resource_dir / "disambiguation.xml", disambig)
    elif lang in GITHUB_ONLY:
        fetch_github(
            f"languagetool-language-modules/{lang}/src/main/resources/"
            f"org/languagetool/resource/{lang}/disambiguation.xml",
            disambig,
        )
    if not disambig.exists():
        disambig.write_text("<?xml version='1.0' encoding='UTF-8'?>\n<rules/>\n")

    for name, dest in [
        ("added.txt", "added.txt"),
        ("removed.txt", "removed.txt"),
        ("multiwords.txt", "multiwords.txt"),
        ("do-not-synthesize.txt", "do-not-synthesize.txt"),
    ]:
        if (resource_dir / name).exists():
            copyfile(resource_dir / name, tag_dir / dest)

    # segment.srx from the core jar
    srx = out_dir / "segment.srx"
    if not srx.exists():
        with ZipFile(lt_dir / "libs" / "languagetool-core.jar") as zf:
            srx.write_bytes(zf.read("org/languagetool/resource/segment.srx"))

    with open(out_dir / "common.txt", "w", encoding="utf-8") as f:
        write_freqlist(f, lang)

    # dictionaries
    output_dump = tag_dir / "output.dump"
    synth_dump = tag_dir / "synth.dump"
    source = DICT_SOURCES.get(lang)

    with TemporaryDirectory() as tmp:
        tmp = Path(tmp)
        if source is None:
            logging.warning("%s: no POS dictionary known; using empty dump", lang)
            output_dump.write_text("")
        else:
            if source[0] == "loose":
                extracted = extract_dict(None, source[1], resource_dir, tmp)
            else:
                jar_path = lt_dir / "libs" / source[1]
                extracted = extract_dict(jar_path, source[2], resource_dir, tmp)

            if ".dict" in extracted and ".info" in extracted:
                if not output_dump.exists():
                    ok = dump_dictionary(java, classpath, extracted[".dict"], extracted[".info"], output_dump)
                    if not ok and not output_dump.exists():
                        output_dump.write_text("")
                synth_dict, synth_info = extracted.get(".dict"), extracted.get(".info")
                # synth dict is a separate {stem}_synth.dict
                synth_pair = {}
                for suffix in (".dict", ".info"):
                    name = f"{source[1] if source[0] == 'loose' else source[2]}_synth{suffix}"
                    loose = resource_dir / name
                    target = tmp / name
                    if loose.exists():
                        copyfile(loose, target)
                        synth_pair[suffix] = target
                    elif source[0] == "jar":
                        with ZipFile(lt_dir / "libs" / source[1]) as zf:
                            entry = f"org/languagetool/resource/{resource_dir.name}/{name}"
                            try:
                                target.write_bytes(zf.read(entry))
                                synth_pair[suffix] = target
                            except KeyError:
                                pass
                if ".dict" in synth_pair and ".info" in synth_pair and not synth_dump.exists():
                    dump_dictionary(java, classpath, synth_pair[".dict"], synth_pair[".info"], synth_dump)
            else:
                output_dump.write_text("")

    if grammar.exists() and not merged_grammar:
        dist_grammar = dist_rules / "grammar.xml"
        canonicalize(dist_grammar if dist_grammar.exists() else grammar, grammar)

    dist_disambig = resource_dir / "disambiguation.xml"
    canonicalize(
        dist_disambig if dist_disambig.exists() else disambig,
        disambig,
    )

    (out_dir / "lang_code.txt").write_text(lang)
    logging.info("%s: done", lang)
    return True


def main():
    logging.basicConfig(level=logging.INFO, format="%(levelname)s %(message)s")
    parser = ArgumentParser()
    parser.add_argument("--lt_dir", type=Path, required=True)
    parser.add_argument("--out_root", type=Path, required=True)
    parser.add_argument("--java", default="java")
    parser.add_argument("--langs", nargs="*", default=None)
    parser.add_argument("--keep-going", action="store_true")
    args = parser.parse_args()

    lt_dir = args.lt_dir.resolve()
    out_root = args.out_root.resolve()
    out_root.mkdir(parents=True, exist_ok=True)

    classpath = os.pathsep.join([str(lt_dir / "libs" / "*")])

    all_langs = sorted(
        [p.name for p in (lt_dir / "org" / "languagetool" / "rules").iterdir() if p.is_dir()]
    )
    # languages that only exist in the LT source repo
    for extra in GITHUB_ONLY:
        if extra not in all_langs:
            all_langs.append(extra)
    all_langs.sort()

    langs = args.langs if args.langs else all_langs
    failures = []
    for lang in langs:
        try:
            make_build_dir(lang, lt_dir, out_root, args.java, classpath, args.keep_going)
        except Exception as e:  # noqa: BLE001
            logging.exception("%s failed: %s", lang, e)
            failures.append(lang)

    if failures:
        logging.error("failed languages: %s", failures)
    print(f"Done. {len(langs) - len(failures)}/{len(langs)} languages.")


if __name__ == "__main__":
    main()
