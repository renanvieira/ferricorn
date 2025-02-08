fn main() {
    println!("cargo:rustc-link-lib=dylib=python3.12");
    println!("cargo:rustc-link-search=native=/Users/renanvieira/.pyenv/versions/3.12.3/lib");
    println!("cargo:include=/Users/renanvieira/.pyenv/versions/3.12.3/include/python3.12");
}
