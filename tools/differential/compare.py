#!/usr/bin/env python3
"""Compares cronslop against an oracle, line by line.

Usage:  compare.py <cronslop-output> <oracle-output> [--allow-over]

Each file holds `<expr>\tOK <seconds>` or `<expr>\tERR` lines. Exits
non-zero if cronslop disagrees. `--allow-over` permits cronslop to report
a *longer* gap than the oracle, which is expected only against the Go
oracle, whose `Next` gives up after five years.
"""
import sys

def load(path):
    rows = {}
    with open(path) as handle:
        for line in handle:
            line = line.rstrip("\n")
            if line:
                expr, result = line.split("\t", 1)
                rows[expr] = result
    return rows

def main():
    if len(sys.argv) < 3:
        sys.exit(__doc__)
    mine, oracle = load(sys.argv[1]), load(sys.argv[2])
    allow_over = "--allow-over" in sys.argv

    exact = both_err = 0
    problems, allowed = [], []
    for expr, theirs in oracle.items():
        ours = mine.get(expr)
        if ours is None:
            problems.append(f"  MISSING   {expr!r} absent from cronslop output")
        elif (ours == "ERR") != (theirs == "ERR"):
            problems.append(f"  PARITY    {expr!r}  cronslop={ours}  oracle={theirs}")
        elif ours == "ERR":
            both_err += 1
        else:
            ours_v, theirs_v = int(ours.split()[1]), int(theirs.split()[1])
            if ours_v == theirs_v:
                exact += 1
            elif ours_v < theirs_v:
                problems.append(
                    f"  UNDER     {expr!r}  cronslop={ours_v}s  oracle={theirs_v}s"
                    f"  (short by {(theirs_v - ours_v) // 86400}d)"
                )
            elif allow_over:
                allowed.append(f"  over      {expr!r}  cronslop={ours_v}s  oracle={theirs_v}s")
            else:
                problems.append(f"  OVER      {expr!r}  cronslop={ours_v}s  oracle={theirs_v}s")

    print(f"{exact} exact, {both_err} both-rejected, {len(allowed)} allowed-over, "
          f"{len(problems)} problems, of {len(oracle)}")
    for line in allowed:
        print(line)
    for line in problems:
        print(line)
    return 1 if problems else 0

sys.exit(main())
