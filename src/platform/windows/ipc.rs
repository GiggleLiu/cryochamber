use std::cell::Cell;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};

use crate::socket::{Request, Response};
use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FlushFileBuffers, ReadFile, WriteFile, OPEN_EXISTING,
};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, SetNamedPipeHandleState,
    PIPE_READMODE_BYTE, PIPE_TYPE_BYTE, PIPE_WAIT,
};

// PIPE_ACCESS_DUPLEX = 0x00000003 (not exported by windows_sys 0.59 from Pipes module)
const PIPE_ACCESS_DUPLEX: u32 = 0x00000003;
// PIPE_NOWAIT = 0x00000001
const PIPE_NOWAIT: u32 = 0x00000001;

fn pipe_name(dir: &Path) -> String {
    let hash = crate::service::dir_hash(dir);
    format!(r"\\.\pipe\cryo-{hash}")
}

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Server side of IPC via named pipes. Daemon creates this on startup.
pub struct IpcServer {
    pipe_name: String,
    handle: Cell<HANDLE>,
}

/// Handle to send a response back to the client.
pub struct IpcResponder {
    handle: HANDLE,
}

/// Wrapper to implement Read for a raw HANDLE.
struct PipeReader {
    handle: HANDLE,
}

impl Read for PipeReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let mut bytes_read: u32 = 0;
        let ok = unsafe {
            ReadFile(
                self.handle,
                buf.as_mut_ptr().cast(),
                buf.len() as u32,
                &mut bytes_read,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(bytes_read as usize)
        }
    }
}

fn write_to_handle(handle: HANDLE, data: &[u8]) -> std::io::Result<()> {
    let mut written: u32 = 0;
    let ok = unsafe {
        WriteFile(
            handle,
            data.as_ptr().cast(),
            data.len() as u32,
            &mut written,
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

impl IpcResponder {
    pub fn respond(self, response: &Response) -> anyhow::Result<()> {
        let mut payload = serde_json::to_string(response)?;
        payload.push('\n');
        write_to_handle(self.handle, payload.as_bytes())?;
        unsafe {
            FlushFileBuffers(self.handle);
            DisconnectNamedPipe(self.handle);
            CloseHandle(self.handle);
        }
        Ok(())
    }
}

impl IpcServer {
    /// Create a named pipe server for the given project directory.
    pub fn bind(dir: &Path) -> anyhow::Result<Self> {
        let name = pipe_name(dir);
        let handle = Self::create_pipe_instance(&name)?;
        Ok(Self {
            pipe_name: name,
            handle: Cell::new(handle),
        })
    }

    fn create_pipe_instance(name: &str) -> anyhow::Result<HANDLE> {
        let wide = to_wide(name);
        // Use 254 instead of PIPE_UNLIMITED_INSTANCES to avoid Windows limits
        const MAX_INSTANCES: u32 = 254;
        let handle = unsafe {
            CreateNamedPipeW(
                wide.as_ptr(),
                PIPE_ACCESS_DUPLEX,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
                MAX_INSTANCES,
                4096,
                4096,
                0,
                std::ptr::null(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            anyhow::bail!(
                "Failed to create named pipe {}: {}",
                name,
                std::io::Error::last_os_error()
            );
        }
        Ok(handle)
    }

    /// Accept one connection, parse the request, return it with a responder.
    pub fn accept_one(&self) -> anyhow::Result<Option<(Request, IpcResponder)>> {
        let current = self.handle.get();
        let ok = unsafe { ConnectNamedPipe(current, std::ptr::null_mut()) };
        if ok == 0 {
            let err = unsafe { GetLastError() };
            // ERROR_PIPE_CONNECTED (535) means client already connected before we called
            if err != 535 {
                anyhow::bail!(
                    "ConnectNamedPipe failed: {}",
                    std::io::Error::from_raw_os_error(err as i32)
                );
            }
        }

        let reader = PipeReader { handle: current };
        let mut buf_reader = BufReader::new(reader);
        let mut line = String::new();
        buf_reader.read_line(&mut line)?;

        if line.trim().is_empty() {
            unsafe {
                DisconnectNamedPipe(current);
            }
            return Ok(None);
        }

        let request: Request = serde_json::from_str(line.trim())?;

        // Create a new pipe instance for the next connection, give current to responder.
        // Use Cell for interior mutability — safe because IpcServer is single-threaded.
        let new_handle = Self::create_pipe_instance(&self.pipe_name)?;
        self.handle.set(new_handle);

        Ok(Some((request, IpcResponder { handle: current })))
    }

    /// Set non-blocking mode. Named pipes use PIPE_NOWAIT for this.
    pub fn set_nonblocking(&self, nonblocking: bool) -> anyhow::Result<()> {
        let mode: u32 = if nonblocking {
            PIPE_READMODE_BYTE | PIPE_NOWAIT
        } else {
            PIPE_READMODE_BYTE | PIPE_WAIT
        };
        let ok = unsafe {
            SetNamedPipeHandleState(self.handle.get(), &mode, std::ptr::null(), std::ptr::null())
        };
        if ok == 0 {
            anyhow::bail!(
                "SetNamedPipeHandleState failed: {}",
                std::io::Error::last_os_error()
            );
        }
        Ok(())
    }

    /// Remove the IPC endpoint. Named pipes are auto-cleaned, so this is a no-op.
    pub fn cleanup(_dir: &Path) {
        // Named pipes are cleaned up when the handle is closed.
    }
}

// Safety: Windows HANDLEs are kernel objects that can be used from any thread.
// Cell<HANDLE> is not Send by default due to the raw pointer, but the underlying
// Windows named pipe handle is safe to transfer between threads.
unsafe impl Send for IpcServer {}

impl Drop for IpcServer {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.handle.get());
        }
    }
}

/// Send a request to the daemon and return the response.
pub fn send_request(dir: &Path, request: &Request) -> anyhow::Result<Response> {
    let name = pipe_name(dir);
    let wide = to_wide(&name);

    // Retry up to 3 times if pipe is busy
    let mut last_error = None;
    for attempt in 0..3 {
        let handle = unsafe {
            CreateFileW(
                wide.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                0,
                std::ptr::null(),
                OPEN_EXISTING,
                0,
                std::ptr::null_mut(),
            )
        };

        if handle != INVALID_HANDLE_VALUE {
            // Success, send request
            let mut payload = serde_json::to_string(request)?;
            payload.push('\n');
            write_to_handle(handle, payload.as_bytes())?;
            unsafe {
                FlushFileBuffers(handle);
            }

            let reader = PipeReader { handle };
            let mut buf_reader = BufReader::new(reader);
            let mut line = String::new();
            buf_reader.read_line(&mut line)?;

            unsafe {
                CloseHandle(handle);
            }

            let response: Response = serde_json::from_str(line.trim())?;
            return Ok(response);
        }

        let err = std::io::Error::last_os_error();
        last_error = Some(err);

        // If pipe is busy (ERROR_PIPE_BUSY = 231), wait and retry
        if unsafe { GetLastError() } == 231 && attempt < 2 {
            std::thread::sleep(std::time::Duration::from_millis(50 * (attempt + 1) as u64));
            continue;
        }
        break;
    }

    anyhow::bail!(
        "Cannot connect to daemon pipe at {}: {}",
        name,
        last_error.unwrap()
    );
}

/// Return the IPC endpoint path for a project directory.
/// On Windows this returns a virtual path used for display/logging only.
pub fn ipc_endpoint_path(dir: &Path) -> PathBuf {
    PathBuf::from(pipe_name(dir))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pipe_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let server = IpcServer::bind(dir.path()).unwrap();

        let request = Request::Note {
            text: "test".into(),
        };
        let dir_path = dir.path().to_owned();
        let handle = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(100));
            send_request(&dir_path, &request).unwrap()
        });

        let (req, responder) = server.accept_one().unwrap().unwrap();
        assert!(matches!(req, Request::Note { .. }));
        responder
            .respond(&Response {
                ok: true,
                message: "ok".into(),
            })
            .unwrap();

        let response = handle.join().unwrap();
        assert!(response.ok);
    }

    #[test]
    fn test_pipe_name() {
        let dir = std::path::Path::new(r"C:\projects\test");
        let name = pipe_name(dir);
        assert!(name.starts_with(r"\\.\pipe\cryo-"));
    }
}
