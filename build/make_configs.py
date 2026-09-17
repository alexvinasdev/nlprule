"""Generate nlprule language configs for all languages with a build dir.

Derives the tagger `extra_tags` automatically: all literal POS tags referenced in
disambiguation.xml that are not present in the analyzer dump must be declared so
the tagger's tag store knows them.

Usage:
    python make_configs.py --data_root ../data2 --configs ../nlprule/configs
"""

from argparse import ArgumentParser
from pathlib import Path
import json
import re

from make_build_dirs import URL_JOIN_REGEX, dump_tags, literal_postags

CASED_LANGUAGES = {"de", "de-DE-x-simple-language"}


def main():
    parser = ArgumentParser()
    parser.add_argument("--data_root", type=Path, required=True)
    parser.add_argument("--configs", type=Path, required=True)
    args = parser.parse_args()

    for build_dir in sorted(args.data_root.iterdir()):
        if not (build_dir / "lang_code.txt").exists():
            continue
        lang = (build_dir / "lang_code.txt").read_text().strip()
        out = args.configs / lang
        if lang in {"en", "de", "es"}:
            # do not overwrite manually tuned configs
            continue
        out.mkdir(parents=True, exist_ok=True)

        dump = build_dir / "tags" / "output.dump"
        disambig = build_dir / "disambiguation.xml"
        grammar = build_dir / "grammar.xml"

        known = dump_tags(dump)

        if not known:
            # no analyzer dictionary: LT has no POS tagger for this language,
            # POS tags are never assigned, so none are needed in the store
            extra = []
        else:
            extra = sorted(
                (literal_postags(disambig) | literal_postags(grammar)) - known
            )

        (out / "tagger.json").write_text(
            json.dumps(
                {
                    "use_compound_split_heuristic": lang in CASED_LANGUAGES,
                    "always_add_lower_tags": lang not in CASED_LANGUAGES,
                    "extra_tags": extra,
                    "retain_last": True,
                },
                indent=4,
            )
            + "\n"
        )
        (out / "rules.json").write_text(
            json.dumps({"allow_errors": True, "ignore_ids": []}, indent=4) + "\n"
        )
        (out / "tokenizer.json").write_text(
            json.dumps(
                {
                    "allow_errors": True,
                    "ignore_ids": [],
                    "extra_join_regexes": [URL_JOIN_REGEX],
                },
                indent=4,
            )
            + "\n"
        )
        print(f"{lang}: extra_tags={len(extra)}")


if __name__ == "__main__":
    main()
