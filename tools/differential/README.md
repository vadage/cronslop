# Differential testing

cronslop computes the worst-case gap between cron runs with closed-form
calendar arithmetic. That arithmetic is only worth trusting if it agrees
with something that actually walks a calendar, so this directory holds
two independent oracles and a comparator.

## Why two oracles

`main.go` is the authority on **what is a valid schedule**: it calls
`cron.ParseStandard` from `robfig/cron v3`, the exact function Kubernetes
validates CronJobs with (`pkg/apis/batch/validation/validation.go` ->
`pkg/util/parsers/parsers.go`). If it rejects an expression and cronslop
accepts it, cronslop would report a period for a CronJob the API server
would never admit.

It is *not* the authority on gap length. `robfig/cron`'s `Next` returns
the zero time when it cannot find a firing within five years (`spec.go`,
`yearLimit := t.Year() + 5`), so it cannot see across a skipped century
leap year — it reports 1461 days for `0 0 29 2 *` where the true answer
is 2921.

`calendar_oracle.py` covers that gap: a plain day-by-day walk over a full
400-year Gregorian cycle, with no search limit. It only understands
day-level fields, so feed it schedules that fire once a day.

## Running it

Requires Go and Python 3. From the repository root:

```sh
# 1. cronslop's answers
cargo build --release
rustc --edition 2024 -O \
  --extern cronslop=target/release/libcronslop.rlib -L target/release/deps \
  -o /tmp/cronslop_out tools/differential/cronslop_out.rs
grep -v '^#' tests/fixtures/expected_periods.txt | cut -f1 > /tmp/corpus.txt
/tmp/cronslop_out < /tmp/corpus.txt > /tmp/mine.txt

# 2. the robfig/cron oracle — parse parity plus gaps it can reach
(cd tools/differential && go build -o /tmp/godiff .)
/tmp/godiff < /tmp/corpus.txt > /tmp/go.txt
python3 tools/differential/compare.py /tmp/mine.txt /tmp/go.txt --allow-over

# 3. the calendar oracle — exact gaps for daily schedules
grep -E '^0 0 ' /tmp/corpus.txt > /tmp/daily.txt
python3 tools/differential/calendar_oracle.py < /tmp/daily.txt > /tmp/cal.txt
python3 tools/differential/compare.py /tmp/mine.txt /tmp/cal.txt
```

`--allow-over` is correct only for the Go oracle, and only because of the
five-year limit above. Against the calendar oracle, any difference at all
is a bug.

## What it has caught

Two real defects, both silent under-reporting — the dangerous direction
for a staleness alert:

- The day-of-week bound ignored the month field, so "the 13th, or any
  Friday, in March" was answered with 7 days against a true 343. This
  was documented as a "conservative approximation"; it was a lower bound,
  not an upper one.
- The one-year leap model invented a 29 February that three years in four
  do not have, so `0 0 29 * *` was answered with 31 days against a true
  59.

Both are fixed, and the gap arithmetic no longer approximates anything.
