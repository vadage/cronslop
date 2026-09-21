# cronslop

[![CI](https://github.com/vadage/cronslop/actions/workflows/ci.yml/badge.svg)](https://github.com/vadage/cronslop/actions/workflows/ci.yml)
[![Audit](https://github.com/vadage/cronslop/actions/workflows/audit.yml/badge.svg)](https://github.com/vadage/cronslop/actions/workflows/audit.yml)

Computes the **maximum possible gap, in seconds, between consecutive runs**
of a cron schedule — the worst-case "how long could this job go without
firing" — by calendar arithmetic, without simulating time forward.

```rust
use cronslop::try_max_period_seconds;

assert_eq!(try_max_period_seconds("0 * * * *").unwrap(), 3_600);      // hourly
assert_eq!(try_max_period_seconds("0 9 * * MON-FRI").unwrap(), 259_200); // Fri -> Mon
assert_eq!(try_max_period_seconds("@every 1h30m").unwrap(), 5_400);
```

## Scope

The accepted syntax, and the input rejected as malformed, match
`robfig/cron`'s `ParseStandard` — the parser Kubernetes' CronJob controller
validates schedules with. That means 5-field cron with `*`/`?`, lists,
ranges and steps; the `N/step` form; 3-letter month and weekday names; the
`@yearly`/`@monthly`/`@weekly`/`@daily`/`@hourly` descriptors; and
`@every <duration>`. Day-of-week is bounded at `0-6` strictly — `7` is
rejected, as Kubernetes rejects it.

Schedules are treated as pure UTC calendar arithmetic; timezones and DST
are deliberately out of scope. See the crate docs for why, and for the one
approximation made when day-of-month and day-of-week are both restricted.

## API

| Function                       | Behavior                                                                          |
|--------------------------------|-----------------------------------------------------------------------------------|
| `try_max_period_seconds(expr)` | Returns `Result<u64, CronError>`. Never panics, on any input.                     |
| `max_period_seconds(expr)`     | Panics on invalid input, by documented contract. For already-validated schedules. |

## Layout

| Path               | Contents                                                                        |
|--------------------|---------------------------------------------------------------------------------|
| `src/lib.rs`       | Public API and crate-level scope docs                                           |
| `src/schedule.rs`  | Cron syntax -> the values each field matches                                    |
| `src/gap.rs`       | Calendar and weekday gap arithmetic                                             |
| `src/value_set.rs` | Allocation-free bitmask set of a field's values                                 |
| `src/duration.rs`  | `@every` duration parsing (exact integer math)                                  |
| `src/error.rs`     | `CronError`                                                                     |
| `src/main.rs`      | Demo binary                                                                     |
| `tests/`           | Expected periods, `robfig/cron` compatibility, panic-freedom, behavior snapshot |

## Panics

`try_max_period_seconds` cannot panic on any input. The library contains no
indexing, unchecked arithmetic, casts, `unwrap` or `expect` on a runtime
path; the only `panic!` is `max_period_seconds`, which panics by documented
contract, and the only `assert!`s run at compile time while the field tables
are built. Overflow is reported as `CronError::InvalidDuration` rather than
wrapping, and `overflow-checks` is on in release builds too.

## Development

```sh
cargo test                      # unit, integration and panic-freedom tests
cargo test --release            # same, with optimizations and overflow checks
cargo clippy --all-targets      # pedantic + nursery + cargo + panic-focused restriction lints
cargo fmt --check
cargo doc --no-deps
cargo audit                     # no dependencies, so nothing to advise on
```

CI runs all of the above on Linux, macOS and Windows against current
stable Rust. The advisory audit also runs weekly on a schedule.

The lint configuration lives in `Cargo.toml` under `[lints]`. Library code
is expected to stay free of panicking constructs: indexing, unchecked
arithmetic, `unwrap`, `expect` and `panic!` are all warnings, and the few
justified exceptions carry an `#[expect(..., reason = "...")]` explaining
why. `tests/no_panics.rs` checks the property directly against ~500k
generated inputs.

Parts of the library fail quietly by design — saturating arithmetic,
`ValueSet` ignoring out-of-range values, `unwrap_or` fallbacks. None of
those would warn or panic if a bound were wrong; they would just return a
different number. `tests/snapshot.rs` is the net under them: it pins the
exact result of 163 expressions, errors included, so a silent change in
behavior fails the build.

## License

[GLWT (Good Luck With That) Public License](LICENSE). Use at your own risk;
the author has absolutely no clue what the code in this project does.
