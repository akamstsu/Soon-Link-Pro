//! Private loopback IPC between the stdio process and the GUI control thread.
//! Never used by an audio callback. The token file grants access only to this user.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    io::{self, BufRead, BufReader, Write},
    net::{Ipv4Addr, SocketAddrV4, TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

pub const MAX_FRAME: usize = 1024 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(3);

pub fn endpoint_path() -> io::Result<PathBuf> {
    std::env::var_os("LOCALAPPDATA")
        .map(|root| {
            PathBuf::from(root)
                .join("SoonLinkPro")
                .join("mcp-endpoint.json")
        })
        .ok_or_else(|| io::Error::other("LOCALAPPDATA is not set"))
}

/// Bounded newline framing. An unterminated final frame is rejected, never executed.
pub fn read_frame(
    reader: &mut impl BufRead,
    deadline: Option<Instant>,
) -> io::Result<Option<Vec<u8>>> {
    let mut frame = Vec::new();
    loop {
        if deadline.is_some_and(|d| Instant::now() >= d) {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "Frame timed out"));
        }
        let chunk = reader.fill_buf()?;
        if chunk.is_empty() {
            return if frame.is_empty() {
                Ok(None)
            } else {
                Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "Unterminated frame",
                ))
            };
        }
        let newline = chunk.iter().position(|b| *b == b'\n');
        let count = newline.map_or(chunk.len(), |i| i + 1);
        if frame.len() + count > MAX_FRAME {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Message exceeds 1 MiB",
            ));
        }
        frame.extend_from_slice(&chunk[..count]);
        reader.consume(count);
        if newline.is_some() {
            return Ok(Some(frame));
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Endpoint {
    port: u16,
    token: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    token: String,
    tool: String,
    arguments: Value,
}

type Reply = Result<Value, String>;
enum Progress {
    Waiting,
    Completed(Reply),
    Cancelled,
}
struct Pending {
    request: Request,
    deadline: Instant,
    progress: Arc<Mutex<Progress>>,
}

pub struct LocalServer {
    receiver: mpsc::Receiver<Pending>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    path: PathBuf,
}

fn send(stream: &mut impl Write, reply: &impl Serialize) -> io::Result<()> {
    let mut bytes = serde_json::to_vec(reply)?;
    bytes.push(b'\n');
    if bytes.len() > MAX_FRAME {
        return Err(io::Error::other("Response exceeds 1 MiB"));
    }
    stream.write_all(&bytes)?;
    stream.flush()
}

// SECURITY_ATTRIBUTES and its descriptor only live for CreateFileW; the kernel
// copies the descriptor. Ownership of a successful file HANDLE transfers to File.
// The protected user-only DACL prevents other Windows accounts reading the token.
// Use TokenUser explicitly: an elevated token's default file owner may be the
// Administrators group, which must not receive access in place of the actual user.
fn user_sddl() -> io::Result<String> {
    use windows_sys::Win32::{
        Foundation::*,
        Security::{Authorization::ConvertSidToStringSidW, *},
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };
    unsafe {
        let mut token = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut bytes = 0;
        GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut bytes);
        if bytes == 0 || bytes > 65536 {
            let error = io::Error::last_os_error();
            CloseHandle(token);
            return Err(error);
        }
        // usize storage provides TOKEN_USER's pointer alignment, followed by the
        // SID data. The SID remains alive until ConvertSidToStringSidW copies it.
        let mut storage = vec![0usize; (bytes as usize).div_ceil(std::mem::size_of::<usize>())];
        let success = GetTokenInformation(
            token,
            TokenUser,
            storage.as_mut_ptr().cast(),
            bytes,
            &mut bytes,
        );
        let error = io::Error::last_os_error();
        CloseHandle(token);
        if success == 0 {
            return Err(error);
        }
        let user = &*storage.as_ptr().cast::<TOKEN_USER>();
        let mut sid = std::ptr::null_mut();
        if ConvertSidToStringSidW(user.User.Sid, &mut sid) == 0 {
            return Err(io::Error::last_os_error());
        }
        // The API allocates a NUL-terminated UTF-16 string with LocalAlloc.
        let text = windows::core::PWSTR(sid).to_string();
        LocalFree(sid.cast());
        let text = text.map_err(io::Error::other)?;
        Ok(format!("D:P(A;;FA;;;{text})"))
    }
}

fn private_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::os::windows::{ffi::OsStrExt, io::FromRawHandle};
    use windows_sys::Win32::{
        Foundation::*,
        Security::{Authorization::*, SECURITY_ATTRIBUTES},
        Storage::FileSystem::*,
    };
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let sddl: Vec<u16> = user_sddl()?.encode_utf16().chain(Some(0)).collect();
    unsafe {
        let mut descriptor = std::ptr::null_mut();
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            std::ptr::null_mut(),
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
        let attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor,
            bInheritHandle: 0,
        };
        let handle = CreateFileW(
            wide.as_ptr(),
            GENERIC_WRITE,
            FILE_SHARE_READ,
            &attributes,
            CREATE_NEW,
            FILE_ATTRIBUTE_NORMAL,
            std::ptr::null_mut(),
        );
        let error = io::Error::last_os_error();
        LocalFree(descriptor);
        if handle == INVALID_HANDLE_VALUE {
            return Err(error);
        }
        let mut file = std::fs::File::from_raw_handle(handle);
        file.write_all(bytes)?;
        file.sync_all()
    }
}

impl LocalServer {
    pub fn start(wake: impl Fn() + Send + 'static) -> io::Result<Self> {
        Self::start_at(endpoint_path()?, wake)
    }
    fn start_at(path: PathBuf, wake: impl Fn() + Send + 'static) -> io::Result<Self> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        listener.set_nonblocking(true)?;
        let mut random = [0u8; 32];
        getrandom::fill(&mut random).map_err(io::Error::other)?;
        let endpoint = Endpoint {
            port: listener.local_addr()?.port(),
            token: random.iter().map(|b| format!("{b:02x}")).collect(),
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        match std::fs::remove_file(&path) {
            Ok(()) => (),
            Err(e) if e.kind() == io::ErrorKind::NotFound => (),
            Err(e) => return Err(e),
        }
        if let Err(error) = private_file(&path, &serde_json::to_vec(&endpoint)?) {
            let _ = std::fs::remove_file(&path);
            return Err(error);
        }
        let (sender, receiver) = mpsc::sync_channel(8);
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let worker = thread::Builder::new()
            .name("soon-link-mcp-ipc".into())
            .spawn(move || {
                while !worker_stop.load(Ordering::Acquire) {
                    match listener.accept() {
                        Ok((mut stream, peer)) if peer.ip().is_loopback() => {
                            let reply =
                                serve(&mut stream, &endpoint.token, &sender, &worker_stop, &wake);
                            // IO/authentication failures cannot mutate the graph.
                            let reply = reply
                                .unwrap_or_else(|_| Err("Local control request failed".into()));
                            let _ = send(&mut stream, &reply);
                        }
                        Ok(_) => (),
                        Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                            thread::park_timeout(Duration::from_millis(20))
                        }
                        Err(_) => break,
                    }
                }
            });
        match worker {
            Ok(worker) => Ok(Self {
                receiver,
                stop,
                worker: Some(worker),
                path,
            }),
            Err(error) => {
                let _ = std::fs::remove_file(path);
                Err(error)
            }
        }
    }

    /// Called on the GUI thread before its tray/minimized early return. The short
    /// mutex serializes apply vs timeout; a timed-out queued edit cannot run later.
    pub fn poll(&mut self, mut execute: impl FnMut(&str, Value) -> Reply) {
        for pending in self.receiver.try_iter().take(8) {
            let Ok(mut progress) = pending.progress.lock() else {
                continue;
            };
            if !matches!(*progress, Progress::Waiting) {
                continue;
            }
            if Instant::now() >= pending.deadline {
                *progress = Progress::Cancelled;
                continue;
            }
            *progress =
                Progress::Completed(execute(&pending.request.tool, pending.request.arguments));
        }
    }
}

fn serve(
    stream: &mut TcpStream,
    token: &str,
    sender: &mpsc::SyncSender<Pending>,
    stop: &AtomicBool,
    wake: &impl Fn(),
) -> io::Result<Reply> {
    stream.set_read_timeout(Some(Duration::from_millis(500)))?;
    stream.set_write_timeout(Some(Duration::from_millis(500)))?;
    let line = read_frame(
        &mut BufReader::new(&mut *stream),
        Some(Instant::now() + REQUEST_TIMEOUT),
    )?
    .ok_or_else(|| io::Error::other("Empty request"))?;
    let request: Request = serde_json::from_slice(&line)?;
    if request.token != token {
        return Ok(Err("Agent control authentication failed".into()));
    }
    let deadline = Instant::now() + REQUEST_TIMEOUT;
    let progress = Arc::new(Mutex::new(Progress::Waiting));
    if sender
        .try_send(Pending {
            request,
            deadline,
            progress: progress.clone(),
        })
        .is_err()
    {
        return Ok(Err("Workbench control queue is unavailable".into()));
    }
    wake();
    loop {
        {
            let mut state = progress
                .lock()
                .map_err(|_| io::Error::other("Control queue failed"))?;
            if matches!(*state, Progress::Completed(_)) {
                if let Progress::Completed(reply) =
                    std::mem::replace(&mut *state, Progress::Cancelled)
                {
                    return Ok(reply);
                }
            }
            if stop.load(Ordering::Acquire)
                || Instant::now() >= deadline
                || matches!(*state, Progress::Cancelled)
            {
                *state = Progress::Cancelled;
                return Ok(Err(
                    "Workbench did not apply the request before timeout; reread state".into(),
                ));
            }
        }
        thread::park_timeout(Duration::from_millis(10));
    }
}

impl Drop for LocalServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        let _ = std::fs::remove_file(&self.path);
        if let Some(worker) = self.worker.take() {
            worker.thread().unpark();
            let _ = worker.join();
        }
    }
}

pub fn call(tool: &str, arguments: Value) -> Reply {
    call_at(
        &endpoint_path().map_err(|e| e.to_string())?,
        tool,
        arguments,
    )
}
fn call_at(path: &Path, tool: &str, arguments: Value) -> Reply {
    let bytes = std::fs::read(path)
        .map_err(|_| "Open Soon Link Pro and enable Agent control in settings first".to_owned())?;
    let endpoint: Endpoint = serde_json::from_slice(&bytes).map_err(|_| {
        "Invalid local control record; disable and enable Agent control again".to_owned()
    })?;
    if endpoint.token.len() != 64
        || !endpoint.token.bytes().all(|b| b.is_ascii_hexdigit())
        || endpoint.port == 0
    {
        return Err("Invalid local control record".into());
    }
    let address = SocketAddrV4::new(Ipv4Addr::LOCALHOST, endpoint.port);
    let mut stream = TcpStream::connect_timeout(&address.into(), Duration::from_secs(1))
        .map_err(|_| "Workbench is not reachable; reopen it and enable Agent control".to_owned())?;
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_secs(1)))
        .map_err(|e| e.to_string())?;
    send(
        &mut stream,
        &Request {
            token: endpoint.token,
            tool: tool.into(),
            arguments,
        },
    )
    .map_err(|e| e.to_string())?;
    let line = read_frame(
        &mut BufReader::new(stream),
        Some(Instant::now() + Duration::from_secs(5)),
    )
    .map_err(|e| format!("Local control connection failed: {e}; reread state before retrying"))?
    .ok_or("Workbench closed the control connection; reread state before retrying")?;
    serde_json::from_slice(&line)
        .map_err(|_| "Invalid workbench response; reread state before retrying".to_owned())?
}

pub fn client_config() -> Value {
    let command = std::env::current_exe()
        .unwrap_or_default()
        .with_file_name("SoonLinkProMcp.exe");
    json!({"mcpServers":{"soon-link-pro":{"command":command.to_string_lossy(),"args":[]}}})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn framing_is_bounded_and_requires_newline() {
        assert!(read_frame(&mut &b"{}"[..], None).is_err());
        assert!(read_frame(&mut &vec![b'x'; MAX_FRAME + 1][..], None).is_err());
        let mut lines = &b"{}\n{}\r\n"[..];
        assert_eq!(read_frame(&mut lines, None).unwrap().unwrap(), b"{}\n");
        assert_eq!(read_frame(&mut lines, None).unwrap().unwrap(), b"{}\r\n");
        assert!(read_frame(&mut lines, None).unwrap().is_none());
    }
    fn temp_path() -> PathBuf {
        let mut random = [0u8; 8];
        getrandom::fill(&mut random).unwrap();
        std::env::temp_dir().join(format!(
            "soon-link-mcp-test-{:x}.json",
            u64::from_le_bytes(random)
        ))
    }
    #[test]
    fn loopback_authentication_control_and_shutdown() {
        let path = temp_path();
        let mut server = LocalServer::start_at(path.clone(), || {}).unwrap();
        let client_path = path.clone();
        let client = thread::spawn(move || call_at(&client_path, "get_routing_state", json!({})));
        let limit = Instant::now() + Duration::from_secs(2);
        let mut handled = false;
        while !handled && Instant::now() < limit {
            server.poll(|name, args| {
                assert_eq!(name, "get_routing_state");
                assert_eq!(args, json!({}));
                handled = true;
                Ok(json!({"test":true}))
            });
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(client.join().unwrap().unwrap(), json!({"test":true}));
        let endpoint: Endpoint = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, endpoint.port)).unwrap();
        send(
            &mut stream,
            &Request {
                token: "wrong".into(),
                tool: "patch_routing".into(),
                arguments: json!({}),
            },
        )
        .unwrap();
        let reply: Reply = serde_json::from_slice(
            &read_frame(&mut BufReader::new(stream), None)
                .unwrap()
                .unwrap(),
        )
        .unwrap();
        assert!(reply.unwrap_err().contains("authentication"));
        server.poll(|_, _| panic!("unauthenticated request must never enter the queue"));
        drop(server);
        assert!(!path.exists());
    }
    #[test]
    fn expired_or_cancelled_requests_never_apply() {
        let path = temp_path();
        let mut server = LocalServer::start_at(path, || {}).unwrap();
        // Inject expired work exactly as a stalled UI would see it.
        let (sender, receiver) = mpsc::channel();
        server.receiver = receiver;
        for state in [Progress::Waiting, Progress::Cancelled] {
            sender
                .send(Pending {
                    request: Request {
                        token: String::new(),
                        tool: "patch_routing".into(),
                        arguments: json!({}),
                    },
                    deadline: Instant::now() - Duration::from_secs(1),
                    progress: Arc::new(Mutex::new(state)),
                })
                .unwrap();
        }
        server.poll(|_, _| panic!("timed out edit applied"));
    }

    #[test]
    fn stalled_workbench_times_out_without_later_applying_the_edit() {
        let path = temp_path();
        let mut server = LocalServer::start_at(path.clone(), || {}).unwrap();
        let client = thread::spawn(move || call_at(&path, "patch_routing", json!({})));
        let error = client.join().unwrap().unwrap_err();
        assert!(error.contains("before timeout"), "{error}");
        server.poll(|_, _| panic!("a timed out client request was applied later"));
    }

    #[test]
    fn token_file_has_a_protected_dacl_for_the_actual_windows_user() {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::{
            Foundation::*,
            Security::{Authorization::*, *},
        };
        let path = temp_path();
        private_file(&path, b"test credential").unwrap();
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        unsafe {
            let mut descriptor = std::ptr::null_mut();
            assert_eq!(
                GetNamedSecurityInfoW(
                    wide.as_ptr(),
                    SE_FILE_OBJECT,
                    DACL_SECURITY_INFORMATION,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    &mut descriptor
                ),
                ERROR_SUCCESS
            );
            // SDDL output uses aliases such as LA on the hosted administrator
            // account. Compare the actual ACL and binary SID, not display text.
            let expected_sddl: Vec<u16> =
                user_sddl().unwrap().encode_utf16().chain(Some(0)).collect();
            let mut expected = std::ptr::null_mut();
            assert_ne!(
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    expected_sddl.as_ptr(),
                    SDDL_REVISION_1,
                    &mut expected,
                    std::ptr::null_mut()
                ),
                0
            );
            let mut control = 0;
            let mut revision = 0;
            assert_ne!(
                GetSecurityDescriptorControl(descriptor, &mut control, &mut revision),
                0
            );
            assert_ne!(control & SE_DACL_PROTECTED, 0);
            let mut aces = Vec::new();
            for sd in [descriptor, expected] {
                let mut acl = std::ptr::null_mut();
                let mut present = 0;
                let mut defaulted = 0;
                assert_ne!(
                    GetSecurityDescriptorDacl(sd, &mut present, &mut acl, &mut defaulted),
                    0
                );
                assert_ne!(present, 0);
                assert!(!acl.is_null());
                assert_eq!((*acl).AceCount, 1);
                let mut ace = std::ptr::null_mut();
                assert_ne!(GetAce(acl, 0, &mut ace), 0);
                aces.push(ace.cast::<ACCESS_ALLOWED_ACE>());
            }
            assert_eq!((*aces[0]).Header.AceType, (*aces[1]).Header.AceType);
            assert_eq!((*aces[0]).Header.AceFlags, (*aces[1]).Header.AceFlags);
            assert_eq!((*aces[0]).Mask, (*aces[1]).Mask);
            assert_ne!(
                EqualSid(
                    std::ptr::addr_of_mut!((*aces[0]).SidStart).cast(),
                    std::ptr::addr_of_mut!((*aces[1]).SidStart).cast()
                ),
                0
            );
            LocalFree(expected);
            LocalFree(descriptor);
        }
        std::fs::remove_file(path).unwrap();
    }
}
