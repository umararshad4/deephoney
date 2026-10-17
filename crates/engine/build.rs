use std::{env, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=native/document-reader.swift");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    let target = match env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
        Ok("aarch64") => "arm64-apple-macosx15.0",
        Ok("x86_64") => "x86_64-apple-macosx15.0",
        other => panic!("unsupported document reader target: {other:?}"),
    };
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR")).join("deephoney-document-reader");
    let status = Command::new("xcrun")
        .args(["swiftc", "-O", "-target", target, "native/document-reader.swift", "-o"])
        .arg(&output)
        .status()
        .expect("install Xcode Command Line Tools to build the native document reader");
    assert!(status.success(), "native document reader compilation failed");
    println!("cargo:rustc-env=DEEPHONEY_DOCUMENT_READER={}", output.display());
}
