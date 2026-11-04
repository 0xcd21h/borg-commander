//! The watcher's eBPF programs. Built for the `bpfel-unknown-none` target, they are the
//! `kernel` module; built for anything else, this is an empty program, so the workspace builds
//! and lints everywhere without a nightly compiler.

#![cfg_attr(target_arch = "bpf", no_std)]
#![cfg_attr(target_arch = "bpf", no_main)]

#[cfg(target_arch = "bpf")]
mod kernel;

#[cfg(not(target_arch = "bpf"))]
fn main() {}
