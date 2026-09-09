#![forbid(unsafe_code)]

fn main() {
    // SQLx tracks existing migration files, but stable Rust needs this directory
    // dependency to rebuild the embedded migrator when a new file is added.
    println!("cargo:rerun-if-changed=../../migrations");
}
