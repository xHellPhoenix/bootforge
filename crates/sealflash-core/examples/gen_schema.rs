//! Schreibt das JSON-Schema des Katalogs nach stdout.
//!
//!     cargo run -p sealflash-core --example gen_schema > catalog/catalog.schema.json
fn main() {
    print!("{}", sealflash_core::catalog::json_schema());
}
