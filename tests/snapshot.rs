//! A locked-in record of what every expression in the corpus produces.
//!
//! The corpus is the one the current implementation was verified against
//! when it replaced its predecessor: 163 expressions covering ordinary
//! schedules, every descriptor, `@every` durations with fractional and
//! overflowing amounts, and every [`CronError`] variant. Recording the
//! error as `Debug` pins the variant and its payload, not just the fact
//! that something failed.
//!
//! This is the net under the parts of the library that fail quietly by
//! design — saturating arithmetic, `ValueSet` ignoring out-of-range
//! values, `unwrap_or` fallbacks. None of those would raise a warning or
//! a panic if a bound were wrong; they would just return a different
//! number, and this test is what notices.
#![expect(clippy::unwrap_used, clippy::panic, reason = "a test that cannot panic cannot fail")]

use cronslop::try_max_period_seconds;

const FIXTURE: &str = include_str!("fixtures/expected_periods.txt");
const FIXTURE_PATH: &str = "tests/fixtures/expected_periods.txt";

/// The recorded form of what an expression produces.
fn outcome(expr: &str) -> String {
    match try_max_period_seconds(expr) {
        Ok(seconds) => format!("OK {seconds}"),
        Err(error) => format!("ERR {error:?}"),
    }
}

/// The corpus, as (expression, recorded outcome) pairs.
fn recorded() -> impl Iterator<Item = (&'static str, &'static str)> {
    FIXTURE.lines().filter(|line| !line.starts_with('#') && !line.is_empty()).map(|line| {
        line.split_once('\t').unwrap_or_else(|| panic!("malformed fixture line: {line:?}"))
    })
}

#[test]
fn every_expression_matches_its_recorded_outcome() {
    let mut checked = 0;
    let mut mismatches = Vec::new();
    for (expr, expected) in recorded() {
        checked += 1;
        let actual = outcome(expr);
        if actual != expected {
            mismatches
                .push(format!("  {expr:?}\n    recorded: {expected}\n    actual:   {actual}"));
        }
    }

    assert!(checked > 150, "fixture looks truncated: only {checked} cases");
    assert!(
        mismatches.is_empty(),
        "{} of {checked} expressions changed behavior.\n{}\n\n\
         If the change is intended, regenerate with:\n    \
         cargo test --test snapshot -- --ignored regenerate\n\
         and review the diff before committing it.",
        mismatches.len(),
        mismatches.join("\n")
    );
}

/// Rewrites the fixture from current behavior, keeping the same
/// expressions. Deliberately ignored: run it only when a behavior change
/// is intended, and read the resulting diff.
#[test]
#[ignore = "rewrites the fixture; run only when a change in behavior is intended"]
fn regenerate() {
    let mut out = String::new();
    for line in FIXTURE.lines().take_while(|line| line.starts_with('#')) {
        out.push_str(line);
        out.push('\n');
    }
    for (expr, _) in recorded() {
        out.push_str(expr);
        out.push('\t');
        out.push_str(&outcome(expr));
        out.push('\n');
    }
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(FIXTURE_PATH);
    std::fs::write(&path, out).unwrap();
    println!("rewrote {}", path.display());
}
