//! Makes `sqlx::migrate!` visible to cargo's fingerprint.
//!
//! The macro reads `migrations/` at compile time and embeds every file into the
//! binary, but cargo has no way of knowing that it did: adding a migration and
//! rebuilding recompiles nothing, and the binary that comes out is missing the
//! migration entirely. It then starts, reports itself healthy, and fails the first
//! request that needs the new schema — which is exactly how an `ALTER TABLE` for a
//! new import source reached production as a 500.
fn main() {
    println!("cargo:rerun-if-changed=migrations");
}
