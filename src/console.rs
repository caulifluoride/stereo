//! The Windows build is a GUI program, so it has no console of its own. When started from a
//! terminal, this attaches to it so Rune's headless output (`--snap`, `--tree`) and errors
//! print there.

use std::fs::OpenOptions;
use std::os::windows::io::IntoRawHandle;

const ATTACH_PARENT_PROCESS: u32 = u32::MAX;
const STD_OUTPUT_HANDLE: u32 = -11i32 as u32;
const STD_ERROR_HANDLE: u32 = -12i32 as u32;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn AttachConsole(process_id: u32) -> i32;
    fn GetStdHandle(std_handle: u32) -> isize;
    fn SetStdHandle(std_handle: u32, handle: isize) -> i32;
}

pub fn attach() {
    // Fails when there's no parent console (launched from the desktop), which is fine.
    if unsafe { AttachConsole(ATTACH_PARENT_PROCESS) } == 0 {
        return;
    }
    // Handles inherited from a pipe or a file are already set; only fill in the missing ones.
    for which in [STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
        if unsafe { GetStdHandle(which) } == 0 {
            if let Ok(out) = OpenOptions::new().write(true).open("CONOUT$") {
                unsafe { SetStdHandle(which, out.into_raw_handle() as isize) };
            }
        }
    }
}
