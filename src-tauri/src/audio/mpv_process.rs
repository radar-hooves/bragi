//! mpv process lifecycle management.
//!
//! Spawning, connecting, and configuring the mpv subprocess.

use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::OnceLock;
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;

use super::mpv_ipc::MpvIpc;

/// How long the startup reaper waits for an orphaned mpv to answer `quit`.
const REAP_QUIT_TIMEOUT: Duration = Duration::from_millis(300);

/// Maximum time to wait for mpv to start and create the socket.
const MPV_STARTUP_TIMEOUT: Duration = Duration::from_secs(5);

/// Locations to try when mpv is not on PATH. A Finder-launched app bundle
/// inherits the minimal launchd PATH (`/usr/bin:/bin:/usr/sbin:/sbin`), which
/// excludes nix profiles and Homebrew, so PATH lookup alone is not enough for
/// the installed app.
const MPV_FALLBACK_DIRS: &[&str] = &[
    "/run/current-system/sw/bin",
    "/usr/local/bin",
    "/opt/homebrew/bin",
    "/usr/bin",
];

/// Resolve the mpv binary, cached for the process lifetime.
///
/// Order: `BRAGI_MPV_PATH` env override, PATH lookup, then well-known install
/// directories (nix system/user profiles, Homebrew).
pub(crate) fn resolve_mpv_path() -> Option<&'static Path> {
    static MPV_PATH: OnceLock<Option<PathBuf>> = OnceLock::new();
    MPV_PATH
        .get_or_init(|| {
            if let Ok(override_path) = std::env::var("BRAGI_MPV_PATH") {
                let p = PathBuf::from(override_path);
                if p.is_file() {
                    return Some(p);
                }
                tracing::warn!(
                    "BRAGI_MPV_PATH is set but does not point at a file: {}",
                    p.display()
                );
            }

            let path_var = std::env::var_os("PATH").unwrap_or_default();
            for dir in std::env::split_paths(&path_var) {
                let candidate = dir.join("mpv");
                if candidate.is_file() {
                    return Some(candidate);
                }
            }

            let mut fallbacks: Vec<PathBuf> = MPV_FALLBACK_DIRS
                .iter()
                .map(|d| PathBuf::from(d).join("mpv"))
                .collect();
            if let Some(home) = dirs::home_dir() {
                fallbacks.insert(0, home.join(".nix-profile/bin/mpv"));
            }
            if let Ok(user) = std::env::var("USER") {
                fallbacks.insert(
                    0,
                    PathBuf::from(format!("/etc/profiles/per-user/{}/bin/mpv", user)),
                );
            }
            fallbacks.into_iter().find(|p| p.is_file())
        })
        .as_deref()
}

/// Generate a unique socket path for this instance.
pub(crate) fn get_socket_path() -> PathBuf {
    let pid = std::process::id();
    PathBuf::from(format!("/tmp/bragi-mpv-{}.sock", pid))
}

/// Spawn mpv process with IPC socket.
pub(crate) fn spawn_mpv(socket_path: &PathBuf, device_name: Option<&str>) -> Result<Child, String> {
    // Remove stale socket if it exists
    let _ = std::fs::remove_file(socket_path);

    let mpv_path = resolve_mpv_path().ok_or_else(|| {
        "mpv binary not found on PATH or in known install locations. \
         Enter the nix devShell (`nix develop`) or install mpv where the app can find it, \
         or set BRAGI_MPV_PATH."
            .to_string()
    })?;

    let mut cmd = Command::new(mpv_path);
    cmd.args([
        "--idle=yes",              // Start in idle mode
        "--no-video",              // Audio only
        "--no-terminal",           // No terminal output
        "--gapless-audio=yes",     // Enable gapless playback
        "--prefetch-playlist=yes", // Prefetch next track
        "--audio-buffer=0.5",      // Small buffer for responsiveness
        // Cache settings for HTTP streams (especially OGG/Opus)
        "--cache=yes",                 // Enable caching for network streams
        "--demuxer-max-bytes=50M",     // Allow buffering up to 50MB
        "--demuxer-readahead-secs=60", // Read ahead 60 seconds
        // Never sit in paused-for-cache for long: resume after at most 2 s of
        // buffered audio. Without this, a seek into an unbuffered region of an
        // HTTP stream freezes playback for ~10 s while the full readahead
        // window refills.
        "--cache-pause-wait=2",
        "--cache-pause-initial=yes",
        // Disable mpv's ReplayGain - we handle normalization via Jellyfin's API metadata
        // (using adjusted_volume() with normalization_gain_db from Jellyfin)
        "--replaygain=no",
        &format!("--input-ipc-server={}", socket_path.display()),
    ]);

    // Set audio device if specified
    if let Some(device) = device_name {
        cmd.arg(format!("--audio-device={}", device));
    }

    // Extra mpv arguments for diagnostics and tuning (whitespace-separated).
    if let Ok(extra) = std::env::var("BRAGI_MPV_EXTRA_ARGS") {
        for arg in extra.split_whitespace() {
            cmd.arg(arg);
        }
    }

    // Audio output override for headless runs (e.g. BRAGI_MPV_AO=null lets
    // playback run silently with normal event timing on machines without a
    // usable audio device).
    if let Ok(ao) = std::env::var("BRAGI_MPV_AO") {
        if !ao.is_empty() {
            cmd.arg(format!("--ao={}", ao));
        }
    }

    // Die-with-parent (Linux): ask the kernel to SIGKILL mpv the instant this
    // bragi process dies by ANY means — crash, SIGKILL, or the dev hot-reload
    // supervisor hard-killing the binary. mpv is `--idle=yes` so it otherwise
    // lingers as an orphan (reparented to init); `std::process::Child` has no
    // kill-on-drop, and the graceful shutdown path is skipped on an abrupt
    // death. This is the root-cause fix for accumulated orphaned mpv processes.
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: pre_exec runs in the forked child before exec. We only call
        // async-signal-safe libc functions (prctl, getppid) and touch no shared
        // state, as required for post-fork hooks.
        unsafe {
            cmd.pre_exec(|| {
                if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL as libc::c_ulong) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                // Close the fork/exec race: if bragi already died between fork
                // and here, PDEATHSIG will never fire, so refuse to become an
                // unkillable orphan.
                if libc::getppid() == 1 {
                    return Err(std::io::Error::other("parent exited before mpv exec"));
                }
                Ok(())
            });
        }
    }

    cmd.spawn()
        .map_err(|e| format!("Failed to spawn mpv: {}", e))
}

/// Whether the bragi that owns a `bragi-mpv-<pid>.sock` is still alive, via
/// the portable `kill(pid, 0)` existence probe (sends no signal).
fn owner_bragi_is_alive(pid: i32) -> bool {
    // SAFETY: signal 0 is a pure existence/permission probe.
    if unsafe { libc::kill(pid, 0) } == 0 {
        return true;
    }
    std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
}

/// Quits mpv left running by a bragi that died abruptly, via its own JSON
/// IPC. Called once at startup; never touches a live bragi's mpv.
pub(crate) fn reap_orphaned_mpv() {
    reap_orphaned_mpv_in(Path::new("/tmp"));
}

/// Body of [`reap_orphaned_mpv`]; `dir` is a test seam for `/tmp`.
fn reap_orphaned_mpv_in(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let Some(owner_pid) = name
            .strip_prefix("bragi-mpv-")
            .and_then(|s| s.strip_suffix(".sock"))
            .and_then(|s| s.parse::<i32>().ok())
        else {
            continue;
        };

        if owner_bragi_is_alive(owner_pid) {
            continue;
        }

        let path = entry.path();
        tracing::info!("Reaping orphaned mpv (owning bragi pid {owner_pid} is gone)");
        // A wedged orphan (connected but silent) keeps its socket for a retry.
        let quit_sent_or_already_gone = match MpvIpc::connect_with_timeouts(
            &path,
            Duration::from_millis(50),
            REAP_QUIT_TIMEOUT,
            REAP_QUIT_TIMEOUT,
        ) {
            Ok(ipc) => ipc.command(vec![Value::String("quit".to_string())]).is_ok(),
            Err(_) => true,
        };
        if quit_sent_or_already_gone {
            let _ = std::fs::remove_file(&path);
        }
    }
}

/// Wait for mpv to create the socket and connect.
pub(crate) fn connect_to_mpv(socket_path: &PathBuf) -> Result<MpvIpc, String> {
    let start = Instant::now();

    while start.elapsed() < MPV_STARTUP_TIMEOUT {
        if socket_path.exists() {
            match MpvIpc::connect(socket_path) {
                Ok(ipc) => return Ok(ipc),
                Err(_) => {
                    thread::sleep(Duration::from_millis(50));
                    continue;
                }
            }
        }
        thread::sleep(Duration::from_millis(50));
    }

    Err("Timeout waiting for mpv to start".to_string())
}

/// Set up property observers for playback state.
pub(crate) fn setup_property_observers(ipc: &MpvIpc) -> Result<(), String> {
    // Observe key properties for state updates
    ipc.observe_property(1, "time-pos")?;
    ipc.observe_property(2, "pause")?;
    ipc.observe_property(3, "playlist-pos")?;
    ipc.observe_property(4, "duration")?;
    ipc.observe_property(5, "idle-active")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixListener;
    use std::sync::mpsc;

    /// A pid guaranteed dead: fork, `_exit` the child, wait for it.
    fn dead_pid() -> i32 {
        // SAFETY: child calls only async-signal-safe `_exit`; parent waits
        // before using the pid.
        let pid = unsafe { libc::fork() };
        assert!(pid >= 0, "fork failed");
        if pid == 0 {
            unsafe { libc::_exit(0) };
        }
        let mut status = 0;
        unsafe { libc::waitpid(pid, &mut status, 0) };
        pid
    }

    fn scratch_dir(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("bragi-reap-test-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        dir
    }

    #[test]
    fn owner_bragi_is_alive_tracks_process_liveness() {
        assert!(owner_bragi_is_alive(std::process::id() as i32));
        assert!(!owner_bragi_is_alive(dead_pid()));
    }

    #[test]
    fn reaps_orphaned_socket_and_asks_mpv_to_quit() {
        let dir = scratch_dir("dead");
        let owner_pid = dead_pid();
        let sock_path = dir.join(format!("bragi-mpv-{owner_pid}.sock"));
        let listener = UnixListener::bind(&sock_path).expect("bind fake mpv socket");

        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let Ok((stream, _)) = listener.accept() else {
                return;
            };
            let mut writer = stream.try_clone().expect("clone stream");
            let mut reader = BufReader::new(stream);
            let mut line = String::new();
            let _ = reader.read_line(&mut line);
            let _ = writeln!(writer, r#"{{"request_id":1,"error":"success"}}"#);
            let _ = tx.send(line);
        });

        reap_orphaned_mpv_in(&dir);

        let received = rx
            .recv_timeout(Duration::from_secs(2))
            .expect("orphan reaper must connect and send a command");
        assert!(received.contains("\"quit\""), "got: {received}");
        assert!(!sock_path.exists(), "stale socket must be removed");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn leaves_a_live_owners_socket_untouched() {
        let dir = scratch_dir("alive");
        let sock_path = dir.join(format!("bragi-mpv-{}.sock", std::process::id()));
        let _listener = UnixListener::bind(&sock_path).expect("bind fake mpv socket");

        reap_orphaned_mpv_in(&dir);

        assert!(sock_path.exists(), "live owner's socket untouched");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ignores_files_that_are_not_bragi_mpv_sockets() {
        let dir = scratch_dir("unrelated");
        let unrelated = dir.join("mpvsocket");
        std::fs::write(&unrelated, b"").expect("write unrelated file");
        let non_numeric = dir.join("bragi-mpv-not-a-pid.sock");
        std::fs::write(&non_numeric, b"").expect("write non-numeric file");

        reap_orphaned_mpv_in(&dir);

        assert!(unrelated.exists());
        assert!(non_numeric.exists());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
