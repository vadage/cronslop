//! `try_max_period_seconds` must answer every input with a value or a
//! [`CronError`] — never a panic, and never a hang.
//!
//! Anything that panics fails the test by definition, so the assertions
//! here are deliberately weak: reaching the end of each case is the
//! property being tested.
#![expect(
    clippy::unwrap_used,
    clippy::missing_const_for_fn,
    reason = "a test that cannot panic cannot fail"
)]

use cronslop::try_max_period_seconds;

/// A deterministic xorshift, so a failure reproduces exactly.
struct Rng(u64);

#[expect(
    clippy::arithmetic_side_effects,
    clippy::as_conversions,
    clippy::indexing_slicing,
    reason = "test-local generator: shifts are constant, and an out-of-range \
              pick would fail the test it exists to run"
)]
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn pick<T: Copy>(&mut self, options: &[T]) -> T {
        let index = usize::try_from(self.next() % options.len() as u64).unwrap();
        options[index]
    }
}

/// Values chosen to sit on the boundaries the parser cares about.
const TOKENS: [&str; 34] = [
    "*",
    "?",
    "0",
    "1",
    "6",
    "7",
    "12",
    "13",
    "23",
    "24",
    "29",
    "31",
    "32",
    "59",
    "60",
    "-1",
    "1-5",
    "5-3",
    "0-0",
    "*/0",
    "*/1",
    "*/60",
    "5/15",
    "1-2-3",
    "1//2",
    "*/*",
    "JAN",
    "DEC",
    "MON-FRI",
    "sun",
    "MONDAY",
    "",
    " ",
    "4294967296",
];

#[test]
fn five_field_expressions_never_panic() {
    let mut rng = Rng(0x5DEE_CE66_D1CE_4001);
    for _ in 0..200_000 {
        let expr = format!(
            "{} {} {} {} {}",
            rng.pick(&TOKENS),
            rng.pick(&TOKENS),
            rng.pick(&TOKENS),
            rng.pick(&TOKENS),
            rng.pick(&TOKENS)
        );
        let _ = try_max_period_seconds(&expr);
    }
}

#[test]
fn arbitrary_text_never_panics() {
    let mut rng = Rng(0x0BAD_C0DE_DEAD_BEEF);
    // Bytes drawn from the ranges the parser gives meaning to, plus
    // multi-byte characters, to catch any slicing that assumes ASCII.
    let alphabet: Vec<char> =
        "0123456789*?,-/@ \tmonJANµsh.+e\u{00B5}\u{4F60}\u{1F600}".chars().collect();
    for _ in 0..200_000 {
        let len = usize::try_from(rng.next() % 24).unwrap();
        let expr: String = (0..len).map(|_| rng.pick(&alphabet)).collect();
        let _ = try_max_period_seconds(&expr);
    }
}

#[test]
fn every_durations_never_panic() {
    let mut rng = Rng(0xFEED_FACE_CAFE_BEEF);
    let units = ["ns", "us", "µs", "ms", "s", "m", "h", "", "x", "S", "H"];
    let numbers = [
        "0",
        "1",
        "9",
        ".5",
        "1.",
        "1.5",
        "1.2.3",
        ".",
        "18446744073709551615",
        "340282366920938463463374607431768211455",
        &"9".repeat(60),
        "",
    ];
    for _ in 0..100_000 {
        let terms = 1 + usize::try_from(rng.next() % 4).unwrap();
        let mut expr = String::from("@every ");
        for _ in 0..terms {
            expr.push_str(rng.pick(&numbers));
            expr.push_str(rng.pick(&units));
        }
        let _ = try_max_period_seconds(&expr);
    }
}

#[test]
fn pathological_inputs_never_panic() {
    let cases: Vec<String> = vec![
        String::new(),
        " ".repeat(1_000),
        "*".repeat(1_000),
        format!("{} * * * *", "1,".repeat(5_000)),
        format!("*/{} * * * *", u32::MAX),
        format!("{}-{} * * * *", u32::MAX, u32::MAX),
        // Regression: a step large enough to overflow the range walk.
        format!("59/{} * * * *", u32::MAX),
        format!("23/{} * * * *", u32::MAX - 5),
        format!("@every {}h{}h", "9".repeat(41), "9".repeat(41)),
        format!("@every {}", "1h".repeat(10_000)),
        "@every 18446744073709551616s".to_owned(),
        "0 0 * * \u{1F600}".to_owned(),
        "\u{00B5}s * * * *".to_owned(),
        "@every .\u{00B5}s".to_owned(),
    ];
    for expr in cases {
        let _ = try_max_period_seconds(&expr);
    }
}
