//! Starting the resident app detached from the terminal that launched it.

/// Starts `exe` with no arguments, detached (see `start_detached`), and returns at once.
#[cfg(unix)]
pub fn spawn_detached(exe: &std::path::Path) -> std::io::Result<()> {
    use std::process::{Command, Stdio};
    let mut app = Command::new(exe);
    app.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    std::os::unix::process::CommandExt::process_group(&mut app, 0); // Ctrl-C / terminal hang-up skip it
    app.spawn().map(drop) // never waited for: it runs until Quit
}

/// Same on Windows, by `CreateProcessW` without handle inheritance: std's
/// `Command` passes on every inheritable handle (cmd.exe keeps spare copies of
/// its output pipe), so a script capturing this command's output would wait
/// until the app quits.
#[cfg(windows)]
pub fn spawn_detached(exe: &std::path::Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        CREATE_NEW_PROCESS_GROUP, CreateProcessW, DETACHED_PROCESS, PROCESS_INFORMATION,
        STARTUPINFOW,
    };
    use windows::core::{PCWSTR, PWSTR};

    let mut cmdline: Vec<u16> = std::iter::once(b'"' as u16)
        .chain(exe.as_os_str().encode_wide())
        .chain([b'"' as u16, 0])
        .collect();
    let startup = STARTUPINFOW {
        cb: size_of::<STARTUPINFOW>() as u32,
        ..STARTUPINFOW::default()
    };
    let mut info = PROCESS_INFORMATION::default();
    // SAFETY: `cmdline` is a writable NUL-terminated buffer; `startup` and `info`
    // live through the call; the two handles it returns are closed right away.
    unsafe {
        CreateProcessW(
            PCWSTR::null(),
            Some(PWSTR(cmdline.as_mut_ptr())),
            None,
            None,
            false,
            DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP,
            None,
            PCWSTR::null(),
            &startup,
            &mut info,
        )?;
        let _ = CloseHandle(info.hThread);
        let _ = CloseHandle(info.hProcess);
    }
    Ok(())
}
