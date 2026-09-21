//! Demo: prints the worst-case gap between runs for a handful of schedules.

use cronslop::try_max_period_seconds;

const DEMO_SCHEDULES: [(&str, &str); 9] = [
    ("0 * * * *", "every hour"),
    ("0 6-20 * * *", "hours 6-20"),
    ("0 0,12 * * *", "twice daily"),
    ("0 */6 * * *", "every 6 hours"),
    ("0 0 1 * *", "1st of month"),
    ("@monthly", "alias monthly"),
    ("0 9 * * MON-FRI", "named weekdays"),
    ("0 0 1 JAN *", "named month"),
    ("@every 1h30m", "fixed interval"),
];

fn main() {
    for (expr, label) in DEMO_SCHEDULES {
        match try_max_period_seconds(expr) {
            Ok(seconds) => println!("{expr:<18} {label:<16} -> {seconds:>10}s"),
            Err(e) => println!("{expr:<18} {label:<16} -> ERROR: {e}"),
        }
    }
}
