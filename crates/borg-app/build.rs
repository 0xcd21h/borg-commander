use std::env;

const TARGET_ENV: &str = "TARGET";
/// Set to 1 to build Borg without its eBPF programs, on a Linux machine without the tools they
/// need. The watcher then hears the kernel's process events instead.
const NO_EBPF: &str = "BORG_NO_EBPF";

fn main() {
    if let Ok(target) = env::var(TARGET_ENV) {
        println!("cargo:rustc-env=BORG_TARGET={target}");
    }
    println!("cargo::rustc-check-cfg=cfg(borg_ebpf)");
    println!("cargo::rerun-if-env-changed={NO_EBPF}");
    let linux = env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux");
    if linux && env::var_os(NO_EBPF).is_none_or(|value| value != "1") && ebpf::build() {
        println!("cargo::rustc-cfg=borg_ebpf");
    }
}

/// The watcher's eBPF programs, built on a Linux machine only.
#[cfg(target_os = "linux")]
mod ebpf {
    use std::{env, path::Path};

    /// The nightly compiler the programs are built with: they need the core library built for
    /// the BPF target, which only nightly does. CI and the release workflow install the same one.
    const TOOLCHAIN: &str = "nightly-2026-10-27";
    const PACKAGE: &str = "borg-watch-ebpf";

    /// Builds the programs into `OUT_DIR`, and says whether they are there.
    pub fn build() -> bool {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../borg-watch-ebpf");
        let package = aya_build::Package { name: PACKAGE, root_dir: root, ..Default::default() };
        if let Err(error) = aya_build::build_ebpf([package], aya_build::Toolchain::Custom(TOOLCHAIN)) {
            panic!(
                "building the watcher's eBPF programs failed: {error:#}\n\n\
                 Install what they need:\n  \
                 rustup toolchain install {TOOLCHAIN} --profile minimal --component rust-src\n  \
                 cargo install bpf-linker --version 0.12.0 --locked\n\
                 or build Borg without them: BORG_NO_EBPF=1 cargo build"
            );
        }
        env::var_os("OUT_DIR").is_some_and(|out| Path::new(&out).join(PACKAGE).is_file())
    }
}

#[cfg(not(target_os = "linux"))]
mod ebpf {
    pub fn build() -> bool {
        false
    }
}
