pub(super) fn run() -> std::result::Result<(), String> {
    use std::env;
    use std::ffi::{c_void, OsStr};
    use std::mem::{size_of, transmute};
    use std::path::PathBuf;
    use std::ptr;

    use windows_sys::Win32::Foundation::{GetLastError, FARPROC, WAIT_FAILED, WAIT_OBJECT_0};
    use windows_sys::Win32::System::Diagnostics::Debug::WriteProcessMemory;
    use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
    use windows_sys::Win32::System::Memory::{
        VirtualAllocEx, VirtualFreeEx, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE,
    };
    use windows_sys::Win32::System::Threading::{
        CreateRemoteThread, GetExitCodeThread, OpenProcess, WaitForSingleObject, INFINITE,
        LPTHREAD_START_ROUTINE, PROCESS_CREATE_THREAD, PROCESS_QUERY_INFORMATION,
        PROCESS_VM_OPERATION, PROCESS_VM_READ, PROCESS_VM_WRITE,
    };

    // Proton launch example from Linux:
    // STEAM_COMPAT_DATA_PATH="$HOME/.local/share/Steam/steamapps/compatdata/29720" \
    // STEAM_COMPAT_CLIENT_INSTALL_PATH="$HOME/.local/share/Steam" \
    // "$HOME/.local/share/Steam/compatibilitytools.d/cachyos-11.0-20260602-slr/proton" run \
    //   target/i686-pc-windows-msvc/release/tyria_injector.exe Gw.exe \
    //   target/i686-pc-windows-msvc/release/tyria_sniffer.dll

    const USAGE: &str = "usage: tyria_injector.exe <pid|process-name> <dll-path>";
    let mut args = env::args_os().skip(1);
    let (Some(target), Some(dll_path), None) = (args.next(), args.next(), args.next()) else {
        return Err(USAGE.to_string());
    };

    let target = target.to_string_lossy().into_owned();
    let dll_path = PathBuf::from(dll_path);
    let dll_path = dll_path
        .canonicalize()
        .map_err(|err| format!("canonicalize dll path failed: {err}"))?;
    if !dll_path.is_file() {
        return Err(format!("DLL path is not a file: {}", dll_path.display()));
    }
    let dll_wide = wide_nul(dll_path.as_os_str());
    let byte_len = dll_wide.len() * size_of::<u16>();

    let pid = match target.parse::<u32>() {
        Ok(pid) => pid,
        Err(_) => {
            find_process_id(&target)?.ok_or_else(|| format!("process not found: {target}"))?
        }
    };

    let desired_access = PROCESS_CREATE_THREAD
        | PROCESS_QUERY_INFORMATION
        | PROCESS_VM_OPERATION
        | PROCESS_VM_WRITE
        | PROCESS_VM_READ;

    let debug_privilege = enable_debug_privilege();

    unsafe {
        let process = OpenProcess(desired_access, 0, pid);
        if process.is_null() {
            let mut message = format!("OpenProcess({pid}) failed: {}", GetLastError());
            if let Err(err) = &debug_privilege {
                message.push_str(&format!("; SeDebugPrivilege unavailable: {err}"));
            }
            message.push_str(
                "; run this injector elevated, or launch Guild Wars without admin elevation",
            );
            return Err(message);
        }
        let _process_guard = HandleGuard(process);

        let remote = VirtualAllocEx(
            process,
            ptr::null(),
            byte_len,
            MEM_COMMIT | MEM_RESERVE,
            PAGE_READWRITE,
        );
        if remote.is_null() {
            return Err(format!("VirtualAllocEx failed: {}", GetLastError()));
        }

        let mut written = 0usize;
        let ok = WriteProcessMemory(
            process,
            remote,
            dll_wide.as_ptr().cast::<c_void>(),
            byte_len,
            &mut written,
        );
        if ok == 0 || written != byte_len {
            let last_error = GetLastError();
            let _ = VirtualFreeEx(process, remote, 0, MEM_RELEASE);
            return Err(format!(
                "WriteProcessMemory failed: error={last_error}, written={written}/{byte_len}"
            ));
        }

        let kernel32 = GetModuleHandleW(wide_nul(OsStr::new("kernel32.dll")).as_ptr());
        if kernel32.is_null() {
            let _ = VirtualFreeEx(process, remote, 0, MEM_RELEASE);
            return Err(format!(
                "GetModuleHandleW(kernel32.dll) failed: {}",
                GetLastError()
            ));
        }
        let load_library = GetProcAddress(kernel32, c"LoadLibraryW".as_ptr().cast());
        if load_library.is_none() {
            let _ = VirtualFreeEx(process, remote, 0, MEM_RELEASE);
            return Err(format!(
                "GetProcAddress(LoadLibraryW) failed: {}",
                GetLastError()
            ));
        }

        let mut thread_id = 0u32;
        // SAFETY: on 32-bit Windows, LoadLibraryW and LPTHREAD_START_ROUTINE use the
        // system ABI with pointer-sized parameters and return values.
        let start_routine = transmute::<FARPROC, LPTHREAD_START_ROUTINE>(load_library);
        let thread = CreateRemoteThread(
            process,
            ptr::null(),
            0,
            start_routine,
            remote,
            0,
            &mut thread_id,
        );
        if thread.is_null() {
            let last_error = GetLastError();
            let _ = VirtualFreeEx(process, remote, 0, MEM_RELEASE);
            return Err(format!("CreateRemoteThread failed: {last_error}"));
        }
        let thread_guard = HandleGuard(thread);
        let wait_result = WaitForSingleObject(thread, INFINITE);
        if wait_result != WAIT_OBJECT_0 {
            // The thread state is unknown, so freeing `remote` could race LoadLibraryW.
            return Err(if wait_result == WAIT_FAILED {
                format!("WaitForSingleObject failed: {}", GetLastError())
            } else {
                format!("WaitForSingleObject returned unexpected status: {wait_result}")
            });
        }

        let mut exit_code = 0u32;
        if GetExitCodeThread(thread, &mut exit_code) == 0 {
            let last_error = GetLastError();
            let _ = VirtualFreeEx(process, remote, 0, MEM_RELEASE);
            drop(thread_guard);
            return Err(format!("GetExitCodeThread failed: {last_error}"));
        }
        let _ = VirtualFreeEx(process, remote, 0, MEM_RELEASE);
        drop(thread_guard);

        if exit_code == 0 {
            return Err("LoadLibraryW returned null inside target process".to_string());
        }
    }

    println!("injected {dll_path:?} into pid {pid}");
    Ok(())
}

fn wide_nul(value: &std::ffi::OsStr) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    value.encode_wide().chain(std::iter::once(0)).collect()
}

fn find_process_id(name: &str) -> std::result::Result<Option<u32>, String> {
    use std::mem::size_of;

    use windows_sys::Win32::Foundation::{GetLastError, ERROR_NO_MORE_FILES};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };

    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == -1isize as _ {
            return Err(format!(
                "CreateToolhelp32Snapshot failed: {}",
                GetLastError()
            ));
        }
        let _guard = HandleGuard(snapshot);
        let mut entry = PROCESSENTRY32W {
            dwSize: size_of::<PROCESSENTRY32W>() as u32,
            cntUsage: 0,
            th32ProcessID: 0,
            th32DefaultHeapID: 0,
            th32ModuleID: 0,
            cntThreads: 0,
            th32ParentProcessID: 0,
            pcPriClassBase: 0,
            dwFlags: 0,
            szExeFile: [0; 260],
        };
        if Process32FirstW(snapshot, &mut entry) == 0 {
            return Err(format!("Process32FirstW failed: {}", GetLastError()));
        }
        loop {
            let exe = nul_terminated_string(&entry.szExeFile);
            if exe.eq_ignore_ascii_case(name) {
                return Ok(Some(entry.th32ProcessID));
            }
            if Process32NextW(snapshot, &mut entry) == 0 {
                let last_error = GetLastError();
                if last_error == ERROR_NO_MORE_FILES {
                    return Ok(None);
                }
                return Err(format!("Process32NextW failed: {last_error}"));
            }
        }
    }
}

fn nul_terminated_string(wide: &[u16]) -> String {
    let end = wide.iter().position(|ch| *ch == 0).unwrap_or(wide.len());
    String::from_utf16_lossy(&wide[..end])
}

fn enable_debug_privilege() -> std::result::Result<(), String> {
    use std::ptr;

    use windows_sys::Win32::Foundation::{
        GetLastError, SetLastError, ERROR_NOT_ALL_ASSIGNED, ERROR_SUCCESS, LUID,
    };
    use windows_sys::Win32::Security::{
        AdjustTokenPrivileges, LookupPrivilegeValueW, LUID_AND_ATTRIBUTES, SE_DEBUG_NAME,
        SE_PRIVILEGE_ENABLED, TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_QUERY,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    unsafe {
        let mut token = ptr::null_mut();
        if OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY,
            &mut token,
        ) == 0
        {
            return Err(format!("OpenProcessToken failed: {}", GetLastError()));
        }
        let _token_guard = HandleGuard(token);

        let mut luid = LUID {
            LowPart: 0,
            HighPart: 0,
        };
        if LookupPrivilegeValueW(ptr::null(), SE_DEBUG_NAME, &mut luid) == 0 {
            return Err(format!("LookupPrivilegeValueW failed: {}", GetLastError()));
        }

        let privileges = TOKEN_PRIVILEGES {
            PrivilegeCount: 1,
            Privileges: [LUID_AND_ATTRIBUTES {
                Luid: luid,
                Attributes: SE_PRIVILEGE_ENABLED,
            }],
        };

        SetLastError(ERROR_SUCCESS);
        if AdjustTokenPrivileges(token, 0, &privileges, 0, ptr::null_mut(), ptr::null_mut()) == 0 {
            return Err(format!("AdjustTokenPrivileges failed: {}", GetLastError()));
        }

        let last_error = GetLastError();
        if last_error == ERROR_NOT_ALL_ASSIGNED {
            return Err("SeDebugPrivilege is not assigned to this process token".to_string());
        }
        if last_error != ERROR_SUCCESS {
            return Err(format!("AdjustTokenPrivileges warning: {last_error}"));
        }

        Ok(())
    }
}

struct HandleGuard(windows_sys::Win32::Foundation::HANDLE);

impl Drop for HandleGuard {
    fn drop(&mut self) {
        unsafe {
            if !self.0.is_null() {
                let _ = windows_sys::Win32::Foundation::CloseHandle(self.0);
            }
        }
    }
}
