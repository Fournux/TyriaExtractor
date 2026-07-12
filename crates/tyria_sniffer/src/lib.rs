#![allow(clippy::missing_safety_doc)]

#[cfg(any(test, all(windows, target_arch = "x86")))]
mod packet;

#[cfg(not(all(windows, target_arch = "x86")))]
#[no_mangle]
pub extern "C" fn tyria_sniffer_requires_i686_windows() {}

#[cfg(all(windows, target_arch = "x86"))]
mod win32_dll;
