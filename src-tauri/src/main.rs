#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Single-instance lock: only allow one WoxMail process
    #[cfg(target_os = "windows")]
    {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS};
        use windows_sys::Win32::System::Threading::CreateMutexW;

        unsafe {
            let name: Vec<u16> = OsStr::new("Global\\WoxMail_SingleInstance")
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();
            let handle = CreateMutexW(std::ptr::null(), 1, name.as_ptr());
            if !handle.is_null() && windows_sys::Win32::Foundation::GetLastError() == ERROR_ALREADY_EXISTS {
                CloseHandle(handle);
                return;
            }
        }
    }

    woxmail_lib::run();
}
