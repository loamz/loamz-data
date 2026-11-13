// The worker finds libduckdb.so next to itself, wherever it is put (the sandbox mounts both in one folder).
fn main() {
    println!("cargo:rustc-link-arg=-Wl,-rpath,$ORIGIN");
}
