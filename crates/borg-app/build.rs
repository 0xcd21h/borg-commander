use std::env;

const TARGET_ENV: &str = "TARGET";

fn main() {
    if let Ok(target) = env::var(TARGET_ENV) {
        println!("cargo:rustc-env=BORG_TARGET={target}");
    }
}
