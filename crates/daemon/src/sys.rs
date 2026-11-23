//! Chamadas de sistema pequenas que o evdev não cobre: poll, eventfd, inotify.

use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::path::Path;
use std::time::Duration;

fn cvt(ret: libc::c_int) -> io::Result<libc::c_int> {
    if ret < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(ret)
    }
}

pub fn set_nonblocking(fd: RawFd) -> io::Result<()> {
    // SAFETY: fcntl em um descritor válido, sem ponteiros.
    unsafe {
        let flags = cvt(libc::fcntl(fd, libc::F_GETFL))?;
        cvt(libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK))?;
    }
    Ok(())
}

/// Resultado de `poll` para um descritor.
#[derive(Debug, Clone, Copy, Default)]
pub struct Ready {
    pub readable: bool,
    /// POLLERR/POLLHUP/POLLNVAL: o dispositivo sumiu.
    pub gone: bool,
}

/// Espera algum descritor ficar legível. `None` espera para sempre.
pub fn poll(fds: &[RawFd], timeout: Option<Duration>) -> io::Result<Vec<Ready>> {
    let mut pfds: Vec<libc::pollfd> = fds
        .iter()
        .map(|&fd| libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        })
        .collect();
    let ms = match timeout {
        None => -1,
        Some(d) => d.as_millis().min(i32::MAX as u128) as libc::c_int,
    };
    // SAFETY: o vetor vive durante a chamada e o tamanho é o dele.
    let r = unsafe { libc::poll(pfds.as_mut_ptr(), pfds.len() as libc::nfds_t, ms) };
    if r < 0 {
        let e = io::Error::last_os_error();
        if e.kind() == io::ErrorKind::Interrupted {
            return Ok(vec![Ready::default(); fds.len()]);
        }
        return Err(e);
    }
    Ok(pfds
        .iter()
        .map(|p| Ready {
            readable: p.revents & libc::POLLIN != 0,
            gone: p.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0,
        })
        .collect())
}

/// Um eventfd para acordar o laço de um worker.
#[derive(Debug)]
pub struct EventFd(OwnedFd);

impl EventFd {
    pub fn new() -> io::Result<EventFd> {
        // SAFETY: eventfd devolve um descritor novo, que passa a ser nosso.
        let fd = cvt(unsafe { libc::eventfd(0, libc::EFD_NONBLOCK | libc::EFD_CLOEXEC) })?;
        Ok(EventFd(unsafe { OwnedFd::from_raw_fd(fd) }))
    }

    pub fn wake(&self) {
        let one: u64 = 1;
        // SAFETY: escreve 8 bytes de uma variável local.
        unsafe {
            libc::write(self.0.as_raw_fd(), (&one as *const u64).cast(), 8);
        }
    }

    pub fn clear(&self) {
        let mut v: u64 = 0;
        // SAFETY: lê 8 bytes para uma variável local; EAGAIN é ignorado.
        unsafe {
            libc::read(self.0.as_raw_fd(), (&mut v as *mut u64).cast(), 8);
        }
    }
}

impl AsRawFd for EventFd {
    fn as_raw_fd(&self) -> RawFd {
        self.0.as_raw_fd()
    }
}

/// Observa um diretório (`/dev/input`) para saber quando dispositivos entram e saem.
#[derive(Debug)]
pub struct DirWatch(OwnedFd);

impl DirWatch {
    pub fn new(dir: &Path) -> io::Result<DirWatch> {
        use std::os::unix::ffi::OsStrExt;
        let c = std::ffi::CString::new(dir.as_os_str().as_bytes())
            .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
        // SAFETY: descritor novo; o caminho é uma CString válida.
        let fd = cvt(unsafe { libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC) })?;
        let fd = unsafe { OwnedFd::from_raw_fd(fd) };
        let mask = libc::IN_CREATE | libc::IN_DELETE | libc::IN_ATTRIB | libc::IN_MOVED_TO;
        cvt(unsafe { libc::inotify_add_watch(fd.as_raw_fd(), c.as_ptr(), mask) })?;
        Ok(DirWatch(fd))
    }

    /// Descarta os eventos pendentes; devolve se havia algum.
    pub fn drain(&self) -> bool {
        let mut buf = [0u8; 4096];
        let mut any = false;
        loop {
            // SAFETY: lê para um buffer local do tamanho informado.
            let n = unsafe { libc::read(self.0.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len()) };
            if n <= 0 {
                return any;
            }
            any = true;
        }
    }
}

impl AsRawFd for DirWatch {
    fn as_raw_fd(&self) -> RawFd {
        self.0.as_raw_fd()
    }
}

/// Escreve um arquivo de forma atômica (temporário + rename), criando o diretório.
pub fn write_atomic(path: &Path, contents: &str) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, contents)?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eventfd_acorda_poll() {
        let e = EventFd::new().unwrap();
        let r = poll(&[e.as_raw_fd()], Some(Duration::ZERO)).unwrap();
        assert!(!r[0].readable);
        e.wake();
        let r = poll(&[e.as_raw_fd()], Some(Duration::from_secs(1))).unwrap();
        assert!(r[0].readable);
        e.clear();
        let r = poll(&[e.as_raw_fd()], Some(Duration::ZERO)).unwrap();
        assert!(!r[0].readable);
    }

    #[test]
    fn inotify_ve_arquivo_novo() {
        let d = tempfile::tempdir().unwrap();
        let w = DirWatch::new(d.path()).unwrap();
        assert!(!w.drain());
        std::fs::write(d.path().join("event3"), "").unwrap();
        poll(&[w.as_raw_fd()], Some(Duration::from_secs(1))).unwrap();
        assert!(w.drain());
    }

    #[test]
    fn escrita_atomica() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("a/b/c.json");
        write_atomic(&p, "{}").unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "{}");
        assert!(!p.with_extension("tmp").exists());
    }
}
