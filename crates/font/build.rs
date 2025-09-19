fn main() {
  println!("cargo:rerun-if-changed=build.rs");
  let version;
  unsafe {
    let v = harfbuzz_sys::hb_version_string();
    let cstr = std::ffi::CStr::from_ptr(v);
    version = cstr.to_str().unwrap();
  }
  println!("cargo:rustc-cfg=harfbuzz_version=\"{}\"", version);
}
