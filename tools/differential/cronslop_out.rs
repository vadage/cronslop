// Emits, for each expression on stdin: "<expr>\t<OK n | ERR>"
use cronslop::try_max_period_seconds;
fn main() {
    let mut input = String::new();
    std::io::Read::read_to_string(&mut std::io::stdin(), &mut input).unwrap();
    for expr in input.lines() {
        match try_max_period_seconds(expr) {
            Ok(v) => println!("{expr}\tOK {v}"),
            Err(_) => println!("{expr}\tERR"),
        }
    }
}
