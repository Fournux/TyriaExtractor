#[cfg(all(windows, target_arch = "x86"))]
mod windows;

#[cfg(not(all(windows, target_arch = "x86")))]
fn main() {
    eprintln!("tyria_injector must be built as 32-bit Windows, e.g. --target i686-pc-windows-msvc");
    std::process::exit(1);
}

#[cfg(all(windows, target_arch = "x86"))]
fn main() {
    if let Err(err) = windows::run() {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}
