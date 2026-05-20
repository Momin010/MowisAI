use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::sync::mpsc;
use std::thread;

use nix::sys::termios::{self, SetArg, Termios};
use nix::sys::wait::{waitpid, WaitStatus};
use nix::unistd::{self, ForkResult, Pid};

/// Message from the PTY shell to the TUI
#[derive(Debug, Clone)]
pub enum ShellEvent {
    Output(String),
    Exited(i32),
}

/// Message from the TUI to the PTY shell
#[derive(Debug, Clone)]
pub enum ShellInput {
    Data(Vec<u8>),
    Resize(u16, u16),
    Kill,
}

/// A PTY-backed shell that runs in the background and communicates via channels.
pub struct PtyShell {
    pub input_tx: mpsc::Sender<ShellInput>,
    pub event_rx: mpsc::Receiver<ShellEvent>,
    pub pid: Pid,
}

impl PtyShell {
    pub fn spawn(cwd: &str) -> Result<Self, String> {
        // Create PTY pair using libc
        let mut master: RawFd = 0;
        let mut slave: RawFd = 0;
        let ret = unsafe { libc::openpty(&mut master, &mut slave, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut()) };
        if ret != 0 {
            return Err(format!("openpty failed: {}", std::io::Error::last_os_error()));
        }

        // Set master to raw mode
        if let Ok(mut raw) = termios::tcgetattr(master) {
            termios::cfmakeraw(&mut raw);
            let _ = termios::tcsetattr(master, SetArg::TCSANOW, &raw);
        }

        let pid = match unsafe { unistd::fork() } {
            Ok(ForkResult::Parent { child }) => child,
            Ok(ForkResult::Child) => {
                // Close master in child
                unsafe { libc::close(master); }

                // Create new session
                unsafe { libc::setsid(); }

                // Set slave as controlling terminal
                unsafe { libc::ioctl(slave, libc::TIOCSCTTY, 0); }

                // Dup slave to stdin/stdout/stderr
                unsafe {
                    libc::dup2(slave, 0);
                    libc::dup2(slave, 1);
                    libc::dup2(slave, 2);
                    if slave > 2 {
                        libc::close(slave);
                    }
                }

                // Change directory
                let c_cwd = std::ffi::CString::new(cwd).unwrap();
                unsafe { libc::chdir(c_cwd.as_ptr()); }

                // Exec shell
                let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
                let c_shell = std::ffi::CString::new(shell.clone()).unwrap();
                let c_arg = std::ffi::CString::new("-i").unwrap();
                unsafe {
                    libc::execvp(c_shell.as_ptr(), [c_shell.as_ptr(), c_arg.as_ptr(), std::ptr::null()].as_ptr());
                }

                // If exec fails
                unsafe { libc::_exit(1); }
                unreachable!()
            }
            Err(e) => {
                unsafe { libc::close(master); }
                unsafe { libc::close(slave); }
                return Err(format!("fork failed: {}", e));
            }
        };

        // Parent: close slave
        unsafe { libc::close(slave); }

        let (input_tx, input_rx) = mpsc::channel::<ShellInput>();
        let (event_tx, event_rx) = mpsc::channel::<ShellEvent>();

        // Reader thread
        let master_read = master;
        let pid_clone = pid;
        thread::spawn(move || {
            let mut buf = [0u8; 4096];
            let mut file = unsafe { std::fs::File::from_raw_fd(master_read) };
            loop {
                match file.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        let data = String::from_utf8_lossy(&buf[..n]).to_string();
                        if event_tx.send(ShellEvent::Output(data)).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
            if let Ok(WaitStatus::Exited(_, code)) = waitpid(pid_clone, None) {
                let _ = event_tx.send(ShellEvent::Exited(code));
            } else {
                let _ = event_tx.send(ShellEvent::Exited(-1));
            }
        });

        // Writer thread
        let master_write = master;
        thread::spawn(move || {
            // We need a separate fd for writing — dup the master
            let write_fd = unsafe { libc::dup(master_write) };
            if write_fd < 0 {
                return;
            }
            let mut file = unsafe { std::fs::File::from_raw_fd(write_fd) };
            while let Ok(msg) = input_rx.recv() {
                match msg {
                    ShellInput::Data(bytes) => {
                        if file.write_all(&bytes).is_err() { break; }
                        let _ = file.flush();
                    }
                    ShellInput::Resize(cols, rows) => {
                        let ws = libc::winsize {
                            ws_row: rows,
                            ws_col: cols,
                            ws_xpixel: 0,
                            ws_ypixel: 0,
                        };
                        unsafe { libc::ioctl(master_write, libc::TIOCSWINSZ, &ws); }
                    }
                    ShellInput::Kill => {
                        unsafe { libc::kill(pid.as_raw(), libc::SIGTERM); }
                        break;
                    }
                }
            }
        });

        Ok(PtyShell { input_tx, event_rx, pid })
    }

    pub fn send(&self, data: &[u8]) {
        let _ = self.input_tx.send(ShellInput::Data(data.to_vec()));
    }

    pub fn resize(&self, cols: u16, rows: u16) {
        let _ = self.input_tx.send(ShellInput::Resize(cols, rows));
    }

    pub fn kill(&self) {
        let _ = self.input_tx.send(ShellInput::Kill);
    }
}

impl Drop for PtyShell {
    fn drop(&mut self) {
        self.kill();
    }
}
