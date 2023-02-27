
fn main() {
    println!("cargo:rerun-if-changed=native/protocol.c");
    cc::Build::new().file("native/protocol.c").compile("m100protocol");
}
