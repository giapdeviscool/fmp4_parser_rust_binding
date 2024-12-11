fn main() {
    uniffi::generate_scaffolding("src/binding_ffi.udl").unwrap();
}
