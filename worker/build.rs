use std::env;

fn main() {
    let pyenv_root = env::var("PYENV_ROOT").unwrap();

    println!("cargo:rustc-link-lib=dylib=python3.12");
    println!(
        "cargo:rustc-link-search=native={}/versions/3.12.3/lib",
        pyenv_root
    );
    println!(
        "cargo:include={}/versions/3.12.3/include/python3.12",
        pyenv_root
    );
}
