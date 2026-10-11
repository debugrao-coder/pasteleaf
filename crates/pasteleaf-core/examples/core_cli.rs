//! Isolated core diagnostic: no clipboard access, no background sync.
use std::io::{self, Read};
fn main() {
    let mut args = std::env::args().skip(1);
    let Some(root) = args.next() else {
        eprintln!("usage: core_cli ABSOLUTE_DATA_ROOT [--json JSON]");
        std::process::exit(2)
    };
    let mut input = String::new();
    if args.next().as_deref() == Some("--json") {
        input = args.next().unwrap_or_default();
    } else {
        io::stdin().read_to_string(&mut input).expect("read stdin");
    }
    match pasteleaf_core::Engine::open(root) {
        Ok(mut engine) => println!("{}", engine.request_json(&input)),
        Err(e) => {
            println!("{}", pasteleaf_core::envelope(Err(e)));
            std::process::exit(1)
        }
    }
}
