fn main() {
    // LNK4104 asks for the COM entry points (DllGetClassObject, ...) to be exported as PRIVATE,
    // which rustc's generated export list can't express. It is harmless for a COM server.
    println!("cargo:rustc-cdylib-link-arg=/IGNORE:4104");
}
