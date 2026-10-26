//! UniFFI's generator, built from the same version as the library: it reads
//! the compiled library and writes the Swift that calls it.

fn main() {
    uniffi::uniffi_bindgen_main()
}
