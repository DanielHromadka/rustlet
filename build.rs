fn main() {
    cc::Build::new()
        .file("native/manager.m")
        .flag("-fobjc-arc")
        .flag("-mmacosx-version-min=12.0")
        .compile("rustlet_native");
    println!("cargo:rustc-link-lib=framework=AppKit");
    println!("cargo:rustc-link-lib=framework=ServiceManagement");
    println!("cargo:rustc-link-arg=-mmacosx-version-min=12.0");
    let clang = std::process::Command::new("xcrun")
        .args(["clang", "--print-resource-dir"])
        .output()
        .expect("Missing clang runtime");
    assert!(clang.status.success());
    let resource = String::from_utf8(clang.stdout).unwrap();
    println!(
        "cargo:rustc-link-arg={}/lib/darwin/libclang_rt.osx.a",
        resource.trim()
    );
    println!("cargo:rerun-if-changed=native/manager.m");
}
