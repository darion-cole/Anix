//! Whether a process id belongs to a running Anix. The crash markers (`session_marker.rs`,
//! the embedded player's web-playback marker) use it to tell a second launch, which hands over to
//! the running Anix, from a start after a crash. Linux reads `/proc/<pid>/comm`; macOS has no
//! `/proc` and asks the kernel with `proc_name`.

/// The executable name both systems report for Anix.
const NAME: &str = "anix";

/// Whether `pid` is a running process named `anix`.
pub fn is_anix(pid: u32) -> bool {
    process_name(pid).is_some_and(|name| name == NAME)
}

#[cfg(target_os = "macos")]
fn process_name(pid: u32) -> Option<String> {
    let pid = libc::c_int::try_from(pid).ok()?;
    // PROC_PIDPATHINFO_MAXSIZE is larger than any name; names are at most 2 * MAXCOMLEN.
    let mut buffer = [0u8; 256];
    // SAFETY: the buffer is valid for its whole length, which is passed along.
    let len = unsafe { libc::proc_name(pid, buffer.as_mut_ptr().cast(), buffer.len() as u32) };
    if len <= 0 {
        return None;
    }
    std::str::from_utf8(&buffer[..len as usize]).ok().map(String::from)
}

#[cfg(not(target_os = "macos"))]
fn process_name(pid: u32) -> Option<String> {
    std::fs::read_to_string(format!("/proc/{pid}/comm")).ok().map(|name| name.trim_end().to_string())
}
