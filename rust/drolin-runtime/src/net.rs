use std::io::{self, Read};
use std::os::fd::{AsRawFd, FromRawFd, RawFd};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;

use drolin_core::NetworkConfig;
use thiserror::Error;

const MAX_FRAME: usize = 65_536;
const PREFIX: usize = 4;

#[derive(Debug, Error)]
pub enum NetError {
    #[error("cannot create the network socketpair: {0}")]
    Socket(#[from] io::Error),
    #[error("cannot start passt {path}: {source}")]
    Passt {
        path: String,
        #[source]
        source: io::Error,
    },
}

/// The userspace network bridge.
///
/// UML's vector `fd` transport is datagram-oriented: one Ethernet frame per
/// datagram. passt speaks qemu's stream protocol: a 4-byte big-endian length
/// followed by the frame. This object owns the two socketpairs, the passt
/// child, and the pump that translates between the two.
pub struct Network {
    guest_fd: RawFd,
    passt: Child,
    stop: Arc<AtomicBool>,
    pump: Option<JoinHandle<()>>,
}

impl Network {
    pub fn start(config: &NetworkConfig) -> Result<Self, NetError> {
        let mut uml = [0; 2];
        if unsafe { libc::socketpair(libc::AF_UNIX, libc::SOCK_DGRAM, 0, uml.as_mut_ptr()) } != 0 {
            return Err(NetError::Socket(io::Error::last_os_error()));
        }
        let mut passt = [0; 2];
        if unsafe { libc::socketpair(libc::AF_UNIX, libc::SOCK_STREAM, 0, passt.as_mut_ptr()) } != 0
        {
            close(uml[0]);
            close(uml[1]);
            return Err(NetError::Socket(io::Error::last_os_error()));
        }

        let size: libc::c_int = 1 << 20;
        for fd in [uml[0], uml[1]] {
            unsafe {
                libc::setsockopt(
                    fd,
                    libc::SOL_SOCKET,
                    libc::SO_RCVBUF,
                    &size as *const _ as *const libc::c_void,
                    std::mem::size_of_val(&size) as libc::socklen_t,
                );
                libc::setsockopt(
                    fd,
                    libc::SOL_SOCKET,
                    libc::SO_SNDBUF,
                    &size as *const _ as *const libc::c_void,
                    std::mem::size_of_val(&size) as libc::socklen_t,
                );
            }
        }

        // Both children must inherit their end across exec.
        clear_cloexec(uml[0]);
        clear_cloexec(passt[0]);

        let fd_arg = passt[0].to_string();
        let mut args = vec![
            "-f".to_owned(),
            "-F".to_owned(),
            fd_arg,
            "-4".to_owned(),
            "-a".to_owned(),
            config.address.clone(),
            "-n".to_owned(),
            config.netmask.clone(),
            "-g".to_owned(),
            config.gateway.clone(),
        ];
        if let Some(dns) = &config.dns {
            args.push("-D".into());
            args.push(dns.clone());
        }
        if let Some(interface) = &config.interface {
            args.push("-i".into());
            args.push(interface.clone());
        }
        let tcp: Vec<_> = config
            .forwards
            .iter()
            .filter(|f| f.protocol == drolin_core::ForwardProtocol::Tcp)
            .collect();
        let udp: Vec<_> = config
            .forwards
            .iter()
            .filter(|f| f.protocol == drolin_core::ForwardProtocol::Udp)
            .collect();
        if tcp.is_empty() {
            args.push("-t".into());
            args.push("none".into());
        } else {
            for forward in tcp {
                args.push("-t".into());
                args.push(format!("{}:{}", forward.host_port, forward.guest_port));
            }
        }
        if udp.is_empty() {
            args.push("-u".into());
            args.push("none".into());
        } else {
            for forward in udp {
                args.push("-u".into());
                args.push(format!("{}:{}", forward.host_port, forward.guest_port));
            }
        }
        let child = Command::new(&config.passt)
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|source| NetError::Passt {
                path: config.passt.clone(),
                source,
            })?;

        close(passt[0]);

        let stop = Arc::new(AtomicBool::new(false));
        let pump = {
            let stop = stop.clone();
            let passt_fd = passt[1];
            let uml_fd = uml[1];
            std::thread::spawn(move || pump(uml_fd, passt_fd, stop))
        };

        Ok(Self {
            guest_fd: uml[0],
            passt: child,
            stop,
            pump: Some(pump),
        })
    }

    /// The argument appended to the UML command line.
    pub fn kernel_arg(&self, mac: &str) -> String {
        format!("vec0:transport=fd,fd={},mac={mac}", self.guest_fd)
    }

    /// Close the parent's copy after the kernel has inherited it.
    pub fn release_guest_fd(&mut self) {
        if self.guest_fd >= 0 {
            close(self.guest_fd);
            self.guest_fd = -1;
        }
    }

    pub fn finish(&mut self) {
        self.stop.store(true, Ordering::Release);
        let _ = self.passt.kill();
        let _ = self.passt.wait();
        self.release_guest_fd();
        if let Some(pump) = self.pump.take() {
            let _ = pump.join();
        }
    }
}

impl Drop for Network {
    fn drop(&mut self) {
        self.finish();
    }
}

fn clear_cloexec(fd: RawFd) {
    unsafe {
        libc::fcntl(fd, libc::F_SETFD, 0);
    }
}

fn close(fd: RawFd) {
    if fd >= 0 {
        unsafe {
            libc::close(fd);
        }
    }
}

fn pump(uml_fd: RawFd, passt_fd: RawFd, stop: Arc<AtomicBool>) {
    use std::os::unix::net::{UnixDatagram, UnixStream};

    let uml = unsafe { UnixDatagram::from_raw_fd(uml_fd) };
    let passt = unsafe { UnixStream::from_raw_fd(passt_fd) };
    let mut pollfds = [
        libc::pollfd {
            fd: uml.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        },
        libc::pollfd {
            fd: passt.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        },
    ];
    let mut stream_buffer = Vec::with_capacity(MAX_FRAME + PREFIX);
    let mut frame = [0u8; MAX_FRAME];

    while !stop.load(Ordering::Acquire) {
        for pfd in &mut pollfds {
            pfd.revents = 0;
        }
        let ready = unsafe { libc::poll(pollfds.as_mut_ptr(), pollfds.len() as _, 50) };
        if ready < 0 {
            if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                continue;
            }
            break;
        }
        if ready == 0 {
            continue;
        }

        if pollfds[0].revents & libc::POLLIN != 0 {
            match uml.recv(&mut frame) {
                Ok(n) if n > 0 => {
                    let mut out = Vec::with_capacity(PREFIX + n);
                    out.extend_from_slice(&(n as u32).to_be_bytes());
                    out.extend_from_slice(&frame[..n]);
                    if write_stream(&passt, &out).is_err() {
                        break;
                    }
                }
                Ok(_) => break,
                Err(error)
                    if error.kind() == io::ErrorKind::WouldBlock
                        || error.kind() == io::ErrorKind::Interrupted => {}
                Err(_) => break,
            }
        }

        if pollfds[1].revents & libc::POLLIN != 0 {
            let mut chunk = [0u8; 16 * 1024];
            let n = match (&passt).read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => n,
                Err(error)
                    if error.kind() == io::ErrorKind::WouldBlock
                        || error.kind() == io::ErrorKind::Interrupted =>
                {
                    continue;
                }
                Err(_) => break,
            };
            stream_buffer.extend_from_slice(&chunk[..n]);
            loop {
                if stream_buffer.len() < PREFIX {
                    break;
                }
                let len = u32::from_be_bytes([
                    stream_buffer[0],
                    stream_buffer[1],
                    stream_buffer[2],
                    stream_buffer[3],
                ]) as usize;
                if len == 0 || len > MAX_FRAME {
                    return;
                }
                if stream_buffer.len() < PREFIX + len {
                    break;
                }
                let frame = stream_buffer[PREFIX..PREFIX + len].to_vec();
                let _ = uml.send(&frame);
                stream_buffer.drain(..PREFIX + len);
            }
        }

        if pollfds
            .iter()
            .any(|p| p.revents & (libc::POLLERR | libc::POLLHUP) != 0)
        {
            break;
        }
    }
}

fn write_stream(stream: &std::os::unix::net::UnixStream, data: &[u8]) -> io::Result<()> {
    use std::io::Write;
    let mut stream = stream;
    stream.write_all(data)
}
