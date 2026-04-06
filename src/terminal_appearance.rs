//! Binary-private terminal appearance detection.

#[cfg(unix)]
use std::env;
#[cfg(unix)]
use std::fs::{File, OpenOptions};
#[cfg(unix)]
use std::io::Write;
#[cfg(unix)]
use std::os::fd::{AsRawFd, RawFd};
#[cfg(unix)]
use std::path::PathBuf;
#[cfg(target_os = "macos")]
use std::process::Command;
#[cfg(unix)]
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TerminalAppearance {
    Light,
    Dark,
}

#[cfg(unix)]
const SVG_THEME_AUTO_DEBUG_ENV: &str = "MMDFLUX_DEBUG_SVG_THEME_AUTO";
#[cfg(unix)]
const OSC_11_QUERY: &[u8] = b"\x1b]11;?\x1b\\";
#[cfg(unix)]
const OSC_11_READ_DEADLINE: Duration = Duration::from_millis(500);
#[cfg(unix)]
const TRACE_POST_LOGICAL_RESTORE_WINDOW: Duration = Duration::from_millis(250);

pub(crate) fn detect_terminal_appearance() -> Option<TerminalAppearance> {
    detect_osc_terminal_appearance()
}

pub(crate) fn detect_macos_terminal_appearance() -> Option<TerminalAppearance> {
    detect_macos_appearance()
}

#[cfg(unix)]
fn detect_osc_terminal_appearance() -> Option<TerminalAppearance> {
    let mut trace = ProbeTrace::from_env();
    log_trace(&mut trace, "probe_mode", "rust-select-default");
    detect_osc_terminal_appearance_rust(&mut trace)
}

#[cfg(not(unix))]
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

    loop {
        let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
            break;
        };

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

#[cfg(unix)]
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

#[cfg(unix)]
fn parse_rgb_triplet(response: &[u8]) -> Option<(u8, u8, u8)> {
    let response = String::from_utf8_lossy(response);
    let rgb = response.split_once("rgb:")?.1;
    let mut components = rgb.split('/');
    let red = normalize_component(leading_hex_component(components.next()?))?;
    let green = normalize_component(leading_hex_component(components.next()?))?;
    let blue = normalize_component(leading_hex_component(components.next()?))?;
    Some((red, green, blue))
}

#[cfg(unix)]
fn leading_hex_component(component: &str) -> &str {
    let len = component
        .bytes()
        .take_while(|byte| byte.is_ascii_hexdigit())
        .count();
    &component[..len]
}

#[cfg(unix)]
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

#[cfg(unix)]
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

#[cfg(unix)]
fn escape_bytes(bytes: &[u8]) -> String {
    let mut escaped = String::with_capacity(bytes.len());
    for byte in bytes {
        escaped.extend(std::ascii::escape_default(*byte).map(char::from));
    }
    escaped
}

#[cfg(unix)]
fn log_trace(trace: &mut Option<ProbeTrace>, label: &str, detail: impl Into<String>) {
    if let Some(trace) = trace.as_mut() {
        trace.log(label, detail.into());
    }
}

#[cfg(unix)]
struct ProbeResponse {
    appearance: Option<TerminalAppearance>,
    #[allow(dead_code)]
    raw_response: Vec<u8>,
}

#[cfg(unix)]
struct ProbeTrace {
    started: Instant,
    file: File,
}

#[cfg(unix)]
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
fn detect_macos_appearance() -> Option<TerminalAppearance> {
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

#[cfg(not(target_os = "macos"))]
fn detect_macos_appearance() -> Option<TerminalAppearance> {
    None
}
