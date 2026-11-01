//! Schreibt das JSON-Schema des Katalogs nach stdout.
//!
//!     cargo run -p bootforge-core --example gen_schema > catalog/catalog.schema.json
fn main() {
    print!("{}", bootforge_core::catalog::json_schema());
}
