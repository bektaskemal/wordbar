#!/usr/bin/env python3
"""Convert a Kindle Vocabulary Builder database into Wordbar CSV.

The Kindle stores looked-up words in `system/vocabulary/vocab.db` on the
device (`system` is hidden, so enable hidden files after plugging it in).
Copy that file to your computer and run:

    ./scripts/kindle-vocab.py vocab.db > kindle.csv
    wordbarctl import kindle.csv

Definitions are left empty on purpose: Wordbar looks each word up itself, and
its own dictionary data is better than what a bare list would carry. The
sentence you met the word in is kept as the example, which is the part the
dictionary cannot give you.
"""

import argparse
import csv
import sqlite3
import sys

# Kindle marks a word 100 once you have flagged it as mastered.
MASTERED = 100


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("database", help="path to vocab.db copied from the Kindle")
    parser.add_argument("--lang", default="en",
                        help="only export this language (default: en; use 'all' for every one)")
    parser.add_argument("--include-mastered", action="store_true",
                        help="also export words you marked as mastered on the device")
    parser.add_argument("--no-examples", action="store_true",
                        help="omit the sentence each word was met in")
    parser.add_argument("--inflected", action="store_true",
                        help="export the form as it appeared rather than its base form")
    args = parser.parse_args()

    try:
        connection = sqlite3.connect(f"file:{args.database}?mode=ro", uri=True)
    except sqlite3.Error as error:
        print(f"cannot open {args.database}: {error}", file=sys.stderr)
        return 1

    try:
        rows = fetch(connection, args)
    except sqlite3.Error as error:
        print(f"{args.database} does not look like a Kindle vocab.db: {error}", file=sys.stderr)
        return 1
    finally:
        connection.close()

    writer = csv.writer(sys.stdout)
    writer.writerow(["term", "definition", "example", "part_of_speech"])
    for term, example in rows:
        writer.writerow([term, "", example, ""])

    print(f"{len(rows)} word(s)", file=sys.stderr)
    return 0


def fetch(connection, args):
    """Words with the first sentence each was looked up in."""
    # `stem` is the dictionary form -- "ephemeral" rather than "ephemerally" --
    # which is what belongs in a vocabulary list.
    column = "w.word" if args.inflected else "COALESCE(NULLIF(w.stem, ''), w.word)"

    where = []
    parameters = []
    if args.lang != "all":
        where.append("w.lang = ?")
        parameters.append(args.lang)
    if not args.include_mastered:
        where.append("COALESCE(w.category, 0) <> ?")
        parameters.append(MASTERED)
    clause = f"WHERE {' AND '.join(where)}" if where else ""

    # One row per word: a word looked up in several books would otherwise
    # arrive several times, and Wordbar would reject the repeats as duplicates.
    # The earliest lookup is the sentence that made you reach for the
    # dictionary, so that is the one worth keeping.
    query = f"""
        SELECT {column} AS term,
               (SELECT l.usage
                  FROM LOOKUPS l
                 WHERE l.word_key = w.id
                 ORDER BY COALESCE(l.timestamp, 0), l.id
                 LIMIT 1) AS example
        FROM WORDS w
        {clause}
        GROUP BY term
        ORDER BY term COLLATE NOCASE
    """

    rows = []
    for term, example in connection.execute(query, parameters):
        term = (term or "").strip()
        if not term:
            continue
        example = "" if args.no_examples else clean(example)
        rows.append((term, example))
    return rows


def clean(sentence):
    """Kindle stores the surrounding text verbatim, newlines and all."""
    if not sentence:
        return ""
    collapsed = " ".join(str(sentence).split())
    # An overlong passage is context, not an example sentence.
    return collapsed if len(collapsed) <= 300 else ""


if __name__ == "__main__":
    sys.exit(main())
