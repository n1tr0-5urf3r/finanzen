//! Prints the OpenAPI document.
//!
//! `openapi.json` is committed and CI runs this binary, rewrites the file and fails
//! on `git diff --exit-code`, so a contract change is a reviewable diff rather than
//! a surprise for whoever wrote the client.
//!
//! ```text
//! cargo run --bin openapi-export > openapi.json
//! ```
fn main() {
    print!("{}", finanzen::openapi::document());
}
