//! What is expected to change code: compilers, interpreters, kernel threads, and the folders and settings that change what a program loads.

use super::Facts;
use super::findings::file_name;
use borg_core::MemoryRegion;

/// Pages the kernel maps into every process, which hold code and an image header by design.
pub(super) const KERNEL_PAGES: &[&str] =
    &["[vdso]", "[vvar]", "[vvar_vclock]", "[vsyscall]", "[vectors]", "[sigpage]", "[uprobes]"];

/// Programs, and what they map, that compile code as they run, so code no file backs is
/// expected of them.
pub(super) const COMPILING_PROGRAMS: &[&str] = &[
    "node", "deno", "bun", "java", "dotnet", "mono", "ruby", "php", "luajit", "julia", "pypy", "beam", "dart",
    "chrome", "chromium", "firefox", "electron", "qemu", "wine", "valgrind", "memcheck",
];

pub(super) const COMPILERS: &[&str] = &[
    "libv8",
    "libnode",
    "v8_context_snapshot",
    "snapshot_blob",
    "libjvm",
    "libcoreclr",
    "libclrjit",
    "libmono",
    "JavaScriptCore",
    "javascriptcore",
    "libxul",
    "libmozjs",
    "libluajit",
    "libLLVM",
    "libjulia",
    "libpcre",
    "libpypy",
    "Qml",
    "QtWebEngine",
    "Electron Framework",
    "Chromium Embedded Framework",
    "Chromium Framework",
    "Chrome Framework",
    "Edge Framework",
    "Browser Framework",
    "libcef",
];

/// Names of memory files that libraries make for code they build, such as closures and JIT
/// code mapped twice.
pub(super) const CODE_FILES: &[&str] = &["libffi", "jit", "doublemapper"];

/// Programs that take the name of the script they run.
pub(super) const INTERPRETERS: &[&str] = &[
    "python",
    "perl",
    "ruby",
    "node",
    "bash",
    "sh",
    "dash",
    "zsh",
    "fish",
    "ksh",
    "tcsh",
    "csh",
    "java",
    "php",
    "php-fpm",
    "lua",
    "tclsh",
    "wish",
    "osascript",
    "bun",
    "deno",
    "pwsh",
    "dotnet",
    "mono",
    "busybox",
    "env",
    "electron",
    "wine",
    "wine-preloader",
    "wine64-preloader",
    "qemu",
    "rscript",
    "beam.smp",
    "erl",
    "dart",
    "julia",
];

/// How kernel threads are named, which a program taking such a name means to look like.
pub(super) const KERNEL_THREADS: &[&str] = &[
    "kworker",
    "ksoftirqd",
    "kthreadd",
    "kswapd",
    "kauditd",
    "khugepaged",
    "kdevtmpfs",
    "migration/",
    "rcu_",
    "watchdog/",
    "jbd2/",
    "irq/",
];

pub(super) const TEMPORARY: &[&str] = &["/tmp/", "/var/tmp/", "/dev/shm/", "/private/tmp/", "/private/var/tmp/"];

/// Where macOS keeps each user's temporary folder, `<folders>/<xx>/<id>/T/`.
pub(super) const USER_TEMPORARY: &[&str] = &["/var/folders/", "/private/var/folders/"];

pub(super) const PRELOADS: &[&str] = &["LD_PRELOAD", "LD_AUDIT", "DYLD_INSERT_LIBRARIES"];

pub(super) const LIBRARY_PATHS: &[&str] = &["LD_LIBRARY_PATH", "DYLD_LIBRARY_PATH", "DYLD_FRAMEWORK_PATH"];

pub(super) fn temporary(path: &str) -> bool {
    TEMPORARY.iter().any(|folder| path.starts_with(folder))
        || USER_TEMPORARY
            .iter()
            .any(|folders| path.strip_prefix(folders).is_some_and(|rest| rest.split('/').nth(2) == Some("T")))
}

/// Whether the process compiles code as it runs, as browsers, Java and JavaScript engines do.
pub(super) fn compiles_code(facts: &Facts, regions: &[MemoryRegion]) -> bool {
    let program = facts.program.as_ref().map_or(facts.name.as_str(), |(path, _)| file_name(path)).to_lowercase();
    COMPILING_PROGRAMS.iter().any(|name| program.starts_with(name))
        || regions.iter().any(|region| {
            let name = region.name.to_ascii_lowercase();
            name.contains("jit")
                || name.contains("rosetta")
                || region.path.as_deref().is_some_and(|path| COMPILERS.iter().any(|c| file_name(path).contains(c)))
        })
}
