#!/usr/bin/env python3
"""A second, independent oracle: a plain calendar walk.

`robfig/cron`'s `Next` gives up after five years (spec.go, `yearLimit`),
so it cannot see across a skipped century leap year and under-reports
schedules like `0 0 29 2 *`. This walker has no such limit, which is what
makes it the tie-breaker when the two disagree.

Reads `<minute> <hour> <dom> <month> <dow>` expressions on stdin, writes
`<expr>\tOK <seconds>` on stdout. Only day-level fields are interpreted;
give it schedules that fire once a day.
"""
import sys
from datetime import date, timedelta

START_YEAR, END_YEAR = 2000, 2400  # a full Gregorian cycle


MONTH_NAMES = {n: i for i, n in enumerate(
    "jan feb mar apr may jun jul aug sep oct nov dec".split(), start=1)}
DOW_NAMES = {n: i for i, n in enumerate("sun mon tue wed thu fri sat".split())}


def parse_value(text, names):
    """A number, or one of the field's three-letter names."""
    try:
        return int(text)
    except ValueError:
        return names[text.lower()]


def parse_set(spec, lo, hi, names=None):
    """Expands a cron field into its values, and whether it was restricted."""
    names = names or {}
    if spec in ("*", "?"):
        return set(range(lo, hi + 1)), False
    out = set()
    for part in spec.split(","):
        step = 1
        if "/" in part:
            part, step_text = part.split("/")
            step = int(step_text)
        if part in ("*", "?"):
            start, end = lo, hi
        elif "-" in part:
            a, b = part.split("-")
            start, end = parse_value(a, names), parse_value(b, names)
        else:
            start = parse_value(part, names)
            end = hi if step > 1 else start
        # Bounds are robfig/cron's job, not this oracle's — but an
        # out-of-range field must not quietly expand to nothing and look
        # like a real answer, so reject it and let the caller skip.
        if start < lo or end > hi or start > end or step < 1:
            raise ValueError(f"{spec!r} out of range for {lo}-{hi}")
        out |= set(range(start, end + 1, step))
    return out, True


def max_gap_seconds(expr):
    minute_spec, hour_spec, dom_spec, month_spec, dow_spec = expr.split()
    # This oracle counts days, so it is only meaningful for a schedule
    # that fires once a day. Refusing the rest stops a careless
    # invocation from producing a confident wrong answer.
    for spec, limit in ((minute_spec, 59), (hour_spec, 23)):
        if not spec.isdigit() or int(spec) > limit:
            raise ValueError(f"not a once-a-day schedule: {expr!r}")
    doms, dom_restricted = parse_set(dom_spec, 1, 31)
    months, _ = parse_set(month_spec, 1, 12, MONTH_NAMES)
    dows, dow_restricted = parse_set(dow_spec, 0, 6, DOW_NAMES)

    previous, worst = None, 0
    day = date(START_YEAR, 1, 1)
    end = date(END_YEAR, 1, 1)
    while day < end:
        if day.month in months:
            dom_ok = day.day in doms
            dow_ok = (day.weekday() + 1) % 7 in dows  # Monday=0 -> Sunday=0
            if dom_restricted and dow_restricted:
                fires = dom_ok or dow_ok
            elif dom_restricted:
                fires = dom_ok
            elif dow_restricted:
                fires = dow_ok
            else:
                fires = True
            if fires:
                ordinal = day.toordinal()
                if previous is not None:
                    worst = max(worst, ordinal - previous)
                previous = ordinal
        day += timedelta(days=1)
    return worst * 86400


for line in sys.stdin:
    expr = line.rstrip("\n")
    if not expr:
        continue
    try:
        print(f"{expr}\tOK {max_gap_seconds(expr)}")
    except (ValueError, KeyError) as problem:
        # Not a daily schedule this oracle understands. Skipping keeps it
        # honest: the comparator only checks expressions present in both.
        print(f"skipped {expr!r}: {problem}", file=sys.stderr)
