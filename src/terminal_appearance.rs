//! Binary-private terminal appearance detection.

#[cfg(any(unix, windows))]
use std::env;
#[cfg(any(unix, windows))]
use std::fs::File;
#[cfg(unix)]
use std::fs::OpenOptions;
#[cfg(any(unix, windows))]
use std::io::Write;
#[cfg(unix)]
use std::os::fd::{AsRawFd, RawFd};
#[cfg(any(unix, windows))]
use std::path::PathBuf;
#[cfg(any(target_os = "macos", windows))]
use std::process::Command;
#[cfg(any(unix, windows))]
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TerminalAppearance {
    Light,
    Dark,
}

#[cfg(any(unix, windows))]
const SVG_THEME_AUTO_DEBUG_ENV: &str = "MMDFLUX_DEBUG_SVG_THEME_AUTO";
#[cfg(any(unix, windows))]
const OSC_11_QUERY: &[u8] = b"\x1b]11;?\x1b\\";
#[cfg(any(unix, windows))]
const OSC_11_READ_DEADLINE: Duration = Duration::from_millis(500);
#[cfg(any(unix, windows))]
const TRACE_POST_LOGICAL_RESTORE_WINDOW: Duration = Duration::from_millis(250);

pub(crate) fn detect_terminal_appearance() -> Option<TerminalAppearance> {
    detect_osc_terminal_appearance()
}

pub(crate) fn detect_os_appearance() -> Option<TerminalAppearance> {
    detect_os_appearance_impl()
}

#[cfg(unix)]
fn detect_osc_terminal_appearance() -> Option<TerminalAppearance> {
    let mut trace = ProbeTrace::from_env();
    log_trace(&mut trace, "probe_mode", "rust-select-default");
    detect_osc_terminal_appearance_rust(&mut trace)
}

#[cfg(not(any(unix, windows)))]
fn detect_osc_terminal_appearance() -> Option<TerminalAppearance> {
    None
}

#[cfg(unix)]
fn detect_osc_terminal_appearance_rust(
    trace: &mut Option<ProbeTrace>,
) -> Option<TerminalAppearance> {
    let tty = OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .ok()?;
    let fd = tty.as_raw_fd();
    log_trace(trace, "open_tty", format!("fd={fd} path=/dev/tty"));

    let mut termios = TermiosGuard::activate(fd, trace).ok()?;
    let result = run_rust_probe(&tty, trace);
    if let Err(error) = &result {
        log_trace(trace, "probe_error", error.to_string());
    }
    let appearance = result
        .as_ref()
        .ok()
        .and_then(|response| response.appearance);
    log_trace(
        trace,
        "probe_result",
        match appearance {
            Some(TerminalAppearance::Dark) => "appearance=dark".to_string(),
            Some(TerminalAppearance::Light) => "appearance=light".to_string(),
            None => "appearance=unknown".to_string(),
        },
    );

    if let Err(error) = termios.restore(trace) {
        log_trace(trace, "restore_error", error.to_string());
    }
    drop(tty);

    appearance
}

#[cfg(unix)]
fn run_rust_probe(tty: &File, trace: &mut Option<ProbeTrace>) -> std::io::Result<ProbeResponse> {
    let fd = tty.as_raw_fd();
    let mut response = Vec::with_capacity(256);
    let started = Instant::now();
    let deadline = started + OSC_11_READ_DEADLINE;

    log_trace(
        trace,
        "write_start",
        format!(
            "bytes={} escaped={}",
            OSC_11_QUERY.len(),
            escape_bytes(OSC_11_QUERY)
        ),
    );
    write_all(fd, OSC_11_QUERY)?;
    log_trace(
        trace,
        "write_end",
        format!("deadline_ms={}", OSC_11_READ_DEADLINE.as_millis()),
    );

    let mut saw_readable = false;
    while response.len() < 1024 {
        let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
            log_trace(trace, "select_timeout", "deadline expired".to_string());
            break;
        };

        let ready = select_readable(fd, remaining)?;
        if !ready {
            log_trace(
                trace,
                "select_timeout",
                format!("remaining_ms={}", remaining.as_millis()),
            );
            break;
        }

        if !saw_readable {
            log_trace(
                trace,
                "first_readable",
                format!("elapsed_us={}", started.elapsed().as_micros()),
            );
            saw_readable = true;
        }

        let chunk = read_chunk(fd)?;
        if chunk.is_empty() {
            log_trace(trace, "read_eof", "read returned 0 bytes".to_string());
            break;
        }

        log_trace(
            trace,
            "read_chunk",
            format!("len={} escaped={}", chunk.len(), escape_bytes(&chunk)),
        );
        response.extend_from_slice(&chunk);

        if osc_11_response_terminated(&response) {
            log_trace(
                trace,
                "terminator_detected",
                format!("elapsed_us={}", started.elapsed().as_micros()),
            );
            break;
        }
    }

    // Keep raw/no-echo active through a short quiet window after the reply so
    // the terminal has time to finish delivering any in-flight OSC bytes
    // before the original settings are restored.
    log_trace(
        trace,
        "logical_restore_point",
        format!(
            "elapsed_us={} response_len={}",
            started.elapsed().as_micros(),
            response.len()
        ),
    );
    let after_logical_restore = read_additional_chunks(
        fd,
        TRACE_POST_LOGICAL_RESTORE_WINDOW,
        trace,
        &mut response,
        "post_logical_restore_chunk",
    )?;
    log_trace(
        trace,
        "post_logical_restore_summary",
        format!(
            "window_ms={} extra_bytes={}",
            TRACE_POST_LOGICAL_RESTORE_WINDOW.as_millis(),
            after_logical_restore
        ),
    );

    let appearance = parse_terminal_appearance(&response);
    log_trace(
        trace,
        "response_summary",
        format!(
            "total_len={} escaped={}",
            response.len(),
            escape_bytes(&response)
        ),
    );

    Ok(ProbeResponse {
        appearance,
        raw_response: response,
    })
}

#[cfg(unix)]
fn read_additional_chunks(
    fd: RawFd,
    window: Duration,
    trace: &mut Option<ProbeTrace>,
    response: &mut Vec<u8>,
    label: &str,
) -> std::io::Result<usize> {
    let deadline = Instant::now() + window;
    let initial_len = response.len();

    while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
        let ready = select_readable(fd, remaining)?;
        if !ready {
            break;
        }

        let chunk = read_chunk(fd)?;
        if chunk.is_empty() {
            break;
        }

        log_trace(
            trace,
            label,
            format!("len={} escaped={}", chunk.len(), escape_bytes(&chunk)),
        );
        response.extend_from_slice(&chunk);
    }

    Ok(response.len().saturating_sub(initial_len))
}

#[cfg(any(unix, windows))]
fn parse_terminal_appearance(response: &[u8]) -> Option<TerminalAppearance> {
    let (red, green, blue) = parse_rgb_triplet(response)?;
    let luminance =
        ((u32::from(red) * 299) + (u32::from(green) * 587) + (u32::from(blue) * 114)) / 1000;
    if luminance < 128 {
        Some(TerminalAppearance::Dark)
    } else {
        Some(TerminalAppearance::Light)
    }
}

#[cfg(any(unix, windows))]
fn parse_rgb_triplet(response: &[u8]) -> Option<(u8, u8, u8)> {
    let response = String::from_utf8_lossy(response);
    let rgb = response.split_once("rgb:")?.1;
    let mut components = rgb.split('/');
    let red = normalize_component(leading_hex_component(components.next()?))?;
    let green = normalize_component(leading_hex_component(components.next()?))?;
    let blue = normalize_component(leading_hex_component(components.next()?))?;
    Some((red, green, blue))
}

#[cfg(any(unix, windows))]
fn leading_hex_component(component: &str) -> &str {
    let len = component
        .bytes()
        .take_while(|byte| byte.is_ascii_hexdigit())
        .count();
    &component[..len]
}

#[cfg(any(unix, windows))]
fn normalize_component(component: &str) -> Option<u8> {
    if component.is_empty() || !component.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }

    let value = u32::from_str_radix(component, 16).ok()?;
    let max = (16_u32)
        .checked_pow(component.len() as u32)?
        .checked_sub(1)?;
    let normalized = ((value * 255) + (max / 2)) / max;
    u8::try_from(normalized).ok()
}

#[cfg(any(unix, windows))]
fn osc_11_response_terminated(response: &[u8]) -> bool {
    response.contains(&0x07) || response.windows(2).any(|window| window == b"\x1b\\")
}

#[cfg(unix)]
fn write_all(fd: RawFd, mut bytes: &[u8]) -> std::io::Result<()> {
    while !bytes.is_empty() {
        let written = unsafe { libc::write(fd, bytes.as_ptr().cast(), bytes.len()) };
        if written < 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error);
        }
        if written == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::WriteZero,
                "tty write returned 0 bytes",
            ));
        }

        bytes = &bytes[written as usize..];
    }

    Ok(())
}

#[cfg(unix)]
fn read_chunk(fd: RawFd) -> std::io::Result<Vec<u8>> {
    let mut buffer = [0_u8; 256];

    loop {
        let read = unsafe { libc::read(fd, buffer.as_mut_ptr().cast(), buffer.len()) };
        if read < 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error);
        }

        return Ok(buffer[..read as usize].to_vec());
    }
}

#[cfg(unix)]
fn select_readable(fd: RawFd, timeout: Duration) -> std::io::Result<bool> {
    loop {
        let mut read_set = FdSet::new(fd)?;
        let mut timeout = libc::timeval {
            tv_sec: timeout.as_secs() as _,
            tv_usec: timeout.subsec_micros() as _,
        };
        let result = unsafe {
            libc::select(
                fd + 1,
                read_set.as_mut_ptr(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut timeout,
            )
        };

        if result < 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error);
        }

        return Ok(result > 0);
    }
}

#[cfg(any(unix, windows))]
fn escape_bytes(bytes: &[u8]) -> String {
    let mut escaped = String::with_capacity(bytes.len());
    for byte in bytes {
        escaped.extend(std::ascii::escape_default(*byte).map(char::from));
    }
    escaped
}

#[cfg(any(unix, windows))]
fn log_trace(trace: &mut Option<ProbeTrace>, label: &str, detail: impl Into<String>) {
    if let Some(trace) = trace.as_mut() {
        trace.log(label, detail.into());
    }
}

#[cfg(any(unix, windows))]
struct ProbeResponse {
    appearance: Option<TerminalAppearance>,
    #[allow(dead_code)]
    raw_response: Vec<u8>,
}

#[cfg(any(unix, windows))]
struct ProbeTrace {
    started: Instant,
    file: File,
}

#[cfg(any(unix, windows))]
impl ProbeTrace {
    fn from_env() -> Option<Self> {
        let path = env::var_os(SVG_THEME_AUTO_DEBUG_ENV)?;
        let path = PathBuf::from(path);
        let mut file = File::create(&path).ok()?;
        let _ = writeln!(
            file,
            "# MMDFLUX SVG auto-theme trace\n# path={}\n# query={}\n# logical_restore_window_ms={}",
            path.display(),
            escape_bytes(OSC_11_QUERY),
            TRACE_POST_LOGICAL_RESTORE_WINDOW.as_millis()
        );
        Some(Self {
            started: Instant::now(),
            file,
        })
    }

    fn log(&mut self, label: &str, detail: String) {
        let _ = writeln!(
            self.file,
            "+{:>8}us {:<28} {}",
            self.started.elapsed().as_micros(),
            label,
            detail
        );
        let _ = self.file.flush();
    }
}

#[cfg(unix)]
struct FdSet {
    set: libc::fd_set,
}

#[cfg(unix)]
impl FdSet {
    fn new(fd: RawFd) -> std::io::Result<Self> {
        if fd < 0 || fd as usize >= libc::FD_SETSIZE {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("fd {fd} is outside FD_SETSIZE"),
            ));
        }

        let mut set = std::mem::MaybeUninit::uninit();
        unsafe {
            libc::FD_ZERO(set.as_mut_ptr());
            let mut set = set.assume_init();
            libc::FD_SET(fd, &mut set);
            Ok(Self { set })
        }
    }

    fn as_mut_ptr(&mut self) -> *mut libc::fd_set {
        &mut self.set
    }
}

#[cfg(unix)]
struct TermiosGuard {
    fd: RawFd,
    old: libc::termios,
    restored: bool,
}

#[cfg(unix)]
impl TermiosGuard {
    fn activate(fd: RawFd, trace: &mut Option<ProbeTrace>) -> std::io::Result<Self> {
        let old = tcgetattr(fd)?;
        let mut raw = old;
        raw.c_lflag &= !(libc::ECHO | libc::ICANON);
        raw.c_cc[libc::VMIN] = 0;
        raw.c_cc[libc::VTIME] = 0;

        log_trace(trace, "set_raw_start", format!("fd={fd}"));
        tcsetattr(fd, libc::TCSANOW, &raw)?;
        log_trace(
            trace,
            "set_raw_end",
            format!(
                "lflag={} vmin={} vtime={}",
                raw.c_lflag,
                raw.c_cc[libc::VMIN],
                raw.c_cc[libc::VTIME]
            ),
        );

        Ok(Self {
            fd,
            old,
            restored: false,
        })
    }

    fn restore(&mut self, trace: &mut Option<ProbeTrace>) -> std::io::Result<()> {
        if self.restored {
            return Ok(());
        }

        log_trace(
            trace,
            "restore_start",
            format!("action=TCSAFLUSH fd={}", self.fd),
        );
        tcsetattr(self.fd, libc::TCSAFLUSH, &self.old)?;
        log_trace(trace, "restore_end", format!("fd={}", self.fd));
        self.restored = true;
        Ok(())
    }
}

#[cfg(unix)]
impl Drop for TermiosGuard {
    fn drop(&mut self) {
        if !self.restored {
            let _ = tcsetattr(self.fd, libc::TCSAFLUSH, &self.old);
        }
    }
}

#[cfg(unix)]
fn tcgetattr(fd: RawFd) -> std::io::Result<libc::termios> {
    loop {
        let mut termios = std::mem::MaybeUninit::uninit();
        let result = unsafe { libc::tcgetattr(fd, termios.as_mut_ptr()) };
        if result == 0 {
            return Ok(unsafe { termios.assume_init() });
        }

        let error = std::io::Error::last_os_error();
        if error.kind() == std::io::ErrorKind::Interrupted {
            continue;
        }
        return Err(error);
    }
}

#[cfg(unix)]
fn tcsetattr(fd: RawFd, action: libc::c_int, termios: &libc::termios) -> std::io::Result<()> {
    loop {
        let result = unsafe { libc::tcsetattr(fd, action, termios) };
        if result == 0 {
            return Ok(());
        }

        let error = std::io::Error::last_os_error();
        if error.kind() == std::io::ErrorKind::Interrupted {
            continue;
        }
        return Err(error);
    }
}

#[cfg(target_os = "macos")]
fn detect_os_appearance_impl() -> Option<TerminalAppearance> {
    let output = Command::new("defaults")
        .args(["read", "-g", "AppleInterfaceStyle"])
        .output()
        .ok()?;

    if output.status.success() {
        Some(TerminalAppearance::Dark)
    } else {
        None
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
fn detect_os_appearance_impl() -> Option<TerminalAppearance> {
    None
}

// ---------------------------------------------------------------------------
// Windows: OS-level appearance via registry
// ---------------------------------------------------------------------------

#[cfg(windows)]
fn detect_os_appearance_impl() -> Option<TerminalAppearance> {
    let output = Command::new("reg")
        .args([
            "query",
            r"HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Themes\Personalize",
            "/v",
            "AppsUseLightTheme",
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    // Output format: "    AppsUseLightTheme    REG_DWORD    0x0"
    for line in stdout.lines() {
        let line = line.trim();
        if line.starts_with("AppsUseLightTheme") {
            if line.contains("0x0") {
                return Some(TerminalAppearance::Dark);
            } else {
                return Some(TerminalAppearance::Light);
            }
        }
    }

    None
}

// ---------------------------------------------------------------------------
// Windows: OSC 11 terminal background probe via Console API
// ---------------------------------------------------------------------------

#[cfg(windows)]
fn detect_osc_terminal_appearance() -> Option<TerminalAppearance> {
    let mut trace = ProbeTrace::from_env();
    log_trace(&mut trace, "probe_mode", "rust-windows-console");
    detect_osc_terminal_appearance_windows(&mut trace)
}

#[cfg(windows)]
fn detect_osc_terminal_appearance_windows(
    trace: &mut Option<ProbeTrace>,
) -> Option<TerminalAppearance> {
    let (h_in, h_out) = open_console_handles(trace).ok()?;

    let mut guard = match ConsoleModeGuard::activate(h_in, h_out, trace) {
        Ok(guard) => guard,
        Err(_) => {
            log_trace(trace, "console_mode_error", "failed to set console mode");
            unsafe {
                windows_sys::Win32::Foundation::CloseHandle(h_in);
                windows_sys::Win32::Foundation::CloseHandle(h_out);
            }
            return None;
        }
    };

    let result = run_windows_probe(h_in, h_out, trace);
    if let Err(error) = &result {
        log_trace(trace, "probe_error", error.to_string());
    }
    let appearance = result
        .as_ref()
        .ok()
        .and_then(|response| response.appearance);
    log_trace(
        trace,
        "probe_result",
        match appearance {
            Some(TerminalAppearance::Dark) => "appearance=dark".to_string(),
            Some(TerminalAppearance::Light) => "appearance=light".to_string(),
            None => "appearance=unknown".to_string(),
        },
    );

    if let Err(error) = guard.restore(trace) {
        log_trace(trace, "restore_error", error.to_string());
    }
    unsafe {
        windows_sys::Win32::Foundation::CloseHandle(h_in);
        windows_sys::Win32::Foundation::CloseHandle(h_out);
    }

    appearance
}

#[cfg(windows)]
fn open_console_handles(
    trace: &mut Option<ProbeTrace>,
) -> std::io::Result<(
    windows_sys::Win32::Foundation::HANDLE,
    windows_sys::Win32::Foundation::HANDLE,
)> {
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };

    let conin: Vec<u16> = "CONIN$\0".encode_utf16().collect();
    let conout: Vec<u16> = "CONOUT$\0".encode_utf16().collect();

    let h_in = unsafe {
        CreateFileW(
            conin.as_ptr(),
            0x80000000 | 0x40000000, // GENERIC_READ | GENERIC_WRITE
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null(),
            OPEN_EXISTING,
            0,
            0,
        )
    };
    if h_in == INVALID_HANDLE_VALUE {
        return Err(std::io::Error::last_os_error());
    }

    let h_out = unsafe {
        CreateFileW(
            conout.as_ptr(),
            0x80000000 | 0x40000000, // GENERIC_READ | GENERIC_WRITE
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null(),
            OPEN_EXISTING,
            0,
            0,
        )
    };
    if h_out == INVALID_HANDLE_VALUE {
        unsafe { windows_sys::Win32::Foundation::CloseHandle(h_in) };
        return Err(std::io::Error::last_os_error());
    }

    log_trace(trace, "open_console", "CONIN$+CONOUT$");

    Ok((h_in, h_out))
}

#[cfg(windows)]
struct ConsoleModeGuard {
    input_handle: windows_sys::Win32::Foundation::HANDLE,
    output_handle: windows_sys::Win32::Foundation::HANDLE,
    old_input_mode: u32,
    old_output_mode: u32,
    restored: bool,
}

#[cfg(windows)]
impl ConsoleModeGuard {
    fn activate(
        input_handle: windows_sys::Win32::Foundation::HANDLE,
        output_handle: windows_sys::Win32::Foundation::HANDLE,
        trace: &mut Option<ProbeTrace>,
    ) -> std::io::Result<Self> {
        use windows_sys::Win32::System::Console::{
            ENABLE_ECHO_INPUT, ENABLE_LINE_INPUT, ENABLE_VIRTUAL_TERMINAL_INPUT,
            ENABLE_VIRTUAL_TERMINAL_PROCESSING, GetConsoleMode, SetConsoleMode,
        };

        let mut old_input_mode: u32 = 0;
        let mut old_output_mode: u32 = 0;

        if unsafe { GetConsoleMode(input_handle, &mut old_input_mode) } == 0 {
            return Err(std::io::Error::last_os_error());
        }
        if unsafe { GetConsoleMode(output_handle, &mut old_output_mode) } == 0 {
            return Err(std::io::Error::last_os_error());
        }

        let new_input_mode = (old_input_mode & !(ENABLE_LINE_INPUT | ENABLE_ECHO_INPUT))
            | ENABLE_VIRTUAL_TERMINAL_INPUT;
        let new_output_mode = old_output_mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING;

        log_trace(
            trace,
            "set_console_mode_start",
            format!(
                "input=0x{:04x}->0x{:04x} output=0x{:04x}->0x{:04x}",
                old_input_mode, new_input_mode, old_output_mode, new_output_mode
            ),
        );

        if unsafe { SetConsoleMode(input_handle, new_input_mode) } == 0 {
            return Err(std::io::Error::last_os_error());
        }
        if unsafe { SetConsoleMode(output_handle, new_output_mode) } == 0 {
            // Restore input mode before returning error.
            unsafe { SetConsoleMode(input_handle, old_input_mode) };
            return Err(std::io::Error::last_os_error());
        }

        log_trace(trace, "set_console_mode_end", "ok");

        Ok(Self {
            input_handle,
            output_handle,
            old_input_mode,
            old_output_mode,
            restored: false,
        })
    }

    fn restore(&mut self, trace: &mut Option<ProbeTrace>) -> std::io::Result<()> {
        use windows_sys::Win32::System::Console::SetConsoleMode;

        if self.restored {
            return Ok(());
        }

        log_trace(trace, "restore_start", "restoring console modes");

        if unsafe { SetConsoleMode(self.input_handle, self.old_input_mode) } == 0 {
            return Err(std::io::Error::last_os_error());
        }
        if unsafe { SetConsoleMode(self.output_handle, self.old_output_mode) } == 0 {
            return Err(std::io::Error::last_os_error());
        }

        log_trace(trace, "restore_end", "ok");
        self.restored = true;
        Ok(())
    }
}

#[cfg(windows)]
impl Drop for ConsoleModeGuard {
    fn drop(&mut self) {
        if !self.restored {
            use windows_sys::Win32::System::Console::SetConsoleMode;
            unsafe {
                SetConsoleMode(self.input_handle, self.old_input_mode);
                SetConsoleMode(self.output_handle, self.old_output_mode);
            }
        }
    }
}

#[cfg(windows)]
fn run_windows_probe(
    h_in: windows_sys::Win32::Foundation::HANDLE,
    h_out: windows_sys::Win32::Foundation::HANDLE,
    trace: &mut Option<ProbeTrace>,
) -> std::io::Result<ProbeResponse> {
    let mut response = Vec::with_capacity(256);
    let started = Instant::now();
    let deadline = started + OSC_11_READ_DEADLINE;

    log_trace(
        trace,
        "write_start",
        format!(
            "bytes={} escaped={}",
            OSC_11_QUERY.len(),
            escape_bytes(OSC_11_QUERY)
        ),
    );
    win_write_all(h_out, OSC_11_QUERY)?;
    log_trace(
        trace,
        "write_end",
        format!("deadline_ms={}", OSC_11_READ_DEADLINE.as_millis()),
    );

    let mut saw_readable = false;
    while response.len() < 1024 {
        let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
            log_trace(trace, "wait_timeout", "deadline expired".to_string());
            break;
        };

        let ready = win_wait_readable(h_in, remaining)?;
        if !ready {
            log_trace(
                trace,
                "wait_timeout",
                format!("remaining_ms={}", remaining.as_millis()),
            );
            break;
        }

        if !saw_readable {
            log_trace(
                trace,
                "first_readable",
                format!("elapsed_us={}", started.elapsed().as_micros()),
            );
            saw_readable = true;
        }

        let chunk = win_read_chunk(h_in)?;
        if chunk.is_empty() {
            log_trace(trace, "read_eof", "read returned 0 bytes".to_string());
            break;
        }

        log_trace(
            trace,
            "read_chunk",
            format!("len={} escaped={}", chunk.len(), escape_bytes(&chunk)),
        );
        response.extend_from_slice(&chunk);

        if osc_11_response_terminated(&response) {
            log_trace(
                trace,
                "terminator_detected",
                format!("elapsed_us={}", started.elapsed().as_micros()),
            );
            break;
        }
    }

    log_trace(
        trace,
        "logical_restore_point",
        format!(
            "elapsed_us={} response_len={}",
            started.elapsed().as_micros(),
            response.len()
        ),
    );
    let after_logical_restore = win_read_additional_chunks(
        h_in,
        TRACE_POST_LOGICAL_RESTORE_WINDOW,
        trace,
        &mut response,
        "post_logical_restore_chunk",
    )?;
    log_trace(
        trace,
        "post_logical_restore_summary",
        format!(
            "window_ms={} extra_bytes={}",
            TRACE_POST_LOGICAL_RESTORE_WINDOW.as_millis(),
            after_logical_restore
        ),
    );

    let appearance = parse_terminal_appearance(&response);
    log_trace(
        trace,
        "response_summary",
        format!(
            "total_len={} escaped={}",
            response.len(),
            escape_bytes(&response)
        ),
    );

    Ok(ProbeResponse {
        appearance,
        raw_response: response,
    })
}

#[cfg(windows)]
fn win_write_all(
    handle: windows_sys::Win32::Foundation::HANDLE,
    mut bytes: &[u8],
) -> std::io::Result<()> {
    use windows_sys::Win32::Storage::FileSystem::WriteFile;

    while !bytes.is_empty() {
        let mut written: u32 = 0;
        let ok = unsafe {
            WriteFile(
                handle,
                bytes.as_ptr(),
                bytes.len() as u32,
                &mut written,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(std::io::Error::last_os_error());
        }
        if written == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::WriteZero,
                "console write returned 0 bytes",
            ));
        }
        bytes = &bytes[written as usize..];
    }

    Ok(())
}

#[cfg(windows)]
fn win_read_chunk(handle: windows_sys::Win32::Foundation::HANDLE) -> std::io::Result<Vec<u8>> {
    use windows_sys::Win32::Storage::FileSystem::ReadFile;

    let mut buffer = [0_u8; 256];
    let mut read: u32 = 0;
    let ok = unsafe {
        ReadFile(
            handle,
            buffer.as_mut_ptr(),
            buffer.len() as u32,
            &mut read,
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        return Err(std::io::Error::last_os_error());
    }

    Ok(buffer[..read as usize].to_vec())
}

#[cfg(windows)]
fn win_wait_readable(
    handle: windows_sys::Win32::Foundation::HANDLE,
    timeout: Duration,
) -> std::io::Result<bool> {
    use windows_sys::Win32::System::Threading::WaitForSingleObject;

    let timeout_ms = timeout.as_millis().min(u32::MAX as u128) as u32;
    let result = unsafe { WaitForSingleObject(handle, timeout_ms) };

    match result {
        0 => Ok(true),           // WAIT_OBJECT_0
        0x00000102 => Ok(false), // WAIT_TIMEOUT
        _ => Err(std::io::Error::last_os_error()),
    }
}

#[cfg(windows)]
fn win_read_additional_chunks(
    handle: windows_sys::Win32::Foundation::HANDLE,
    window: Duration,
    trace: &mut Option<ProbeTrace>,
    response: &mut Vec<u8>,
    label: &str,
) -> std::io::Result<usize> {
    let deadline = Instant::now() + window;
    let initial_len = response.len();

    loop {
        let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
            break;
        };

        let ready = win_wait_readable(handle, remaining)?;
        if !ready {
            break;
        }

        let chunk = win_read_chunk(handle)?;
        if chunk.is_empty() {
            break;
        }

        log_trace(
            trace,
            label,
            format!("len={} escaped={}", chunk.len(), escape_bytes(&chunk)),
        );
        response.extend_from_slice(&chunk);
    }

    Ok(response.len().saturating_sub(initial_len))
}
