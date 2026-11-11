use std::io::{Read, Write};
use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use hex_api::{Proc, Refusal};
use hex_client::guest::{Duplex, Link};
use rustix::event::{PollFd, PollFlags, Timespec};
use serde::Deserialize;
use socket2::{Domain, SockAddr, Socket, Type};

use super::{exhausted, invalid};

pub(super) struct Endpoint {
    pub path: PathBuf,
    pub timeout: Duration,
    /// Present only while creating a VM. Normal control/stdio connections have no lifetime cap.
    pub create_deadline: Option<Instant>,
}

impl Endpoint {
    fn remaining(&self) -> Duration {
        self.create_deadline.map_or(self.timeout, |until| {
            self.timeout
                .min(until.saturating_duration_since(Instant::now()))
        })
    }
}

fn left(deadline: Instant) -> Result<Duration, Refusal> {
    let duration = deadline.saturating_duration_since(Instant::now());
    if duration.is_zero() {
        Err(Refusal::os(
            "waiting for Firecracker",
            "operation deadline elapsed",
        ))
    } else {
        Ok(duration)
    }
}

fn deadline(timeout: Duration) -> Result<Instant, Refusal> {
    if timeout.is_zero() {
        return Err(invalid("operation deadline has elapsed"));
    }
    Instant::now()
        .checked_add(timeout)
        .ok_or_else(|| invalid("operation deadline is unrepresentable"))
}

fn wait(fd: impl AsFd, flags: PollFlags, until: Instant) -> Result<(), Refusal> {
    loop {
        let duration = left(until)?;
        let timespec = Timespec {
            tv_sec: i64::try_from(duration.as_secs())
                .map_err(|_| invalid("operation deadline is unrepresentable"))?,
            tv_nsec: i64::from(duration.subsec_nanos()),
        };
        let mut descriptors = [PollFd::new(&fd, flags)];
        match rustix::event::poll(&mut descriptors, Some(&timespec)) {
            Ok(0) => {
                return Err(Refusal::os(
                    "waiting for Firecracker",
                    "operation deadline elapsed",
                ));
            }
            Ok(_) => return Ok(()),
            Err(rustix::io::Errno::INTR) => continue,
            Err(error) => return Err(Refusal::os("waiting for Firecracker", error)),
        }
    }
}

fn connect(path: &Path, until: Instant) -> Result<UnixStream, Refusal> {
    let socket = Socket::new(Domain::UNIX, Type::STREAM, None)
        .map_err(|e| Refusal::os("opening Firecracker connection", e))?;
    socket
        .set_nonblocking(true)
        .map_err(|e| Refusal::os("making Firecracker connection nonblocking", e))?;
    let address =
        SockAddr::unix(path).map_err(|e| Refusal::os("naming Firecracker connection", e))?;
    match socket.connect(&address) {
        Ok(()) => {}
        Err(error)
            if error.kind() == std::io::ErrorKind::WouldBlock
                || error.raw_os_error() == Some(libc::EINPROGRESS) =>
        {
            wait(&socket, PollFlags::OUT, until)?;
            if let Some(error) = socket
                .take_error()
                .map_err(|e| Refusal::os("checking Firecracker connection", e))?
            {
                return Err(Refusal::os("connecting to Firecracker", error));
            }
        }
        Err(error) => return Err(Refusal::os("connecting to Firecracker", error)),
    }
    socket
        .set_nonblocking(false)
        .map_err(|e| Refusal::os("configuring Firecracker connection", e))?;
    let descriptor: OwnedFd = socket.into();
    Ok(UnixStream::from(descriptor))
}

fn write(stream: &mut UnixStream, mut bytes: &[u8], until: Instant) -> Result<(), Refusal> {
    while !bytes.is_empty() {
        stream
            .set_write_timeout(Some(left(until)?))
            .map_err(|e| Refusal::os("bounding Firecracker write", e))?;
        let count = stream
            .write(bytes)
            .map_err(|e| Refusal::os("writing Firecracker request", e))?;
        if count == 0 {
            return Err(Refusal::os(
                "writing Firecracker request",
                "connection ended",
            ));
        }
        bytes = bytes
            .get(count..)
            .ok_or_else(|| invalid("write count exceeds request"))?;
    }
    Ok(())
}

fn read_exact(
    stream: &mut UnixStream,
    mut bytes: &mut [u8],
    until: Instant,
) -> Result<(), Refusal> {
    while !bytes.is_empty() {
        stream
            .set_read_timeout(Some(left(until)?))
            .map_err(|e| Refusal::os("bounding Firecracker read", e))?;
        let count = stream
            .read(bytes)
            .map_err(|e| Refusal::os("reading Firecracker response", e))?;
        if count == 0 {
            return Err(Refusal::os(
                "reading Firecracker response",
                "connection ended",
            ));
        }
        bytes = bytes
            .get_mut(count..)
            .ok_or_else(|| invalid("read count exceeds response"))?;
    }
    Ok(())
}

impl Link for Endpoint {
    fn reply_budget(&self) -> Option<Duration> {
        Some(self.remaining())
    }

    fn dial(&self, port: u32) -> Result<Box<dyn Duplex>, Refusal> {
        self.dial_before(port, None)
    }

    fn dial_before(
        &self,
        port: u32,
        caller_deadline: Option<Instant>,
    ) -> Result<Box<dyn Duplex>, Refusal> {
        let own = deadline(self.remaining())?;
        let until = caller_deadline.map_or(own, |caller| own.min(caller));
        left(until)?;
        let mut stream = connect(&self.path, until)?;
        write(&mut stream, format!("CONNECT {port}\n").as_bytes(), until)?;
        // Firecracker vsock.md: `OK <host-port>\n`, the port an unsigned 32-bit integer.
        let bound = "OK "
            .len()
            .checked_add(u32::MAX.to_string().len())
            .and_then(|n| n.checked_add(1))
            .ok_or_else(|| invalid("vsock response bound overflow"))?;
        let mut line = Vec::new();
        line.try_reserve_exact(bound)
            .map_err(|_| exhausted("vsock response", bound))?;
        while line.len() < bound {
            let mut byte = [0u8; 1];
            read_exact(&mut stream, &mut byte, until)?;
            let byte = *byte.first().ok_or_else(|| invalid("empty vsock byte"))?;
            line.push(byte);
            if byte == b'\n' {
                break;
            }
        }
        let text = std::str::from_utf8(&line).map_err(|_| invalid("malformed vsock response"))?;
        if text
            .strip_prefix("OK ")
            .and_then(|n| n.strip_suffix('\n'))
            .and_then(|n| n.parse::<u32>().ok())
            .filter(|n| *n > 0)
            .is_none()
        {
            return Err(invalid(
                "Firecracker did not acknowledge selected guest port",
            ));
        }
        stream
            .set_read_timeout(None)
            .and_then(|()| stream.set_write_timeout(None))
            .map_err(|e| Refusal::os("handing off guest stream", e))?;
        if port == hex_core::stream::files::FILES_PORT {
            // A staging connection is exactly one operation. Its current client otherwise resets
            // per-read timeouts and may be uncalibrated; retain one explicit absolute stage budget.
            return Ok(Box::new(StagingStream { stream, until }));
        }
        Ok(Box::new(stream))
    }
}

struct StagingStream {
    stream: UnixStream,
    until: Instant,
}

impl StagingStream {
    fn limit(&self, requested: Option<Duration>) -> std::io::Result<Option<Duration>> {
        let remaining = self.until.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "Firecracker staging operation deadline elapsed",
            ));
        }
        Ok(Some(
            requested.map_or(remaining, |requested| requested.min(remaining)),
        ))
    }
}

impl Read for StagingStream {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        self.set_read_timeout(self.stream.read_timeout()?)?;
        self.stream.read(bytes)
    }
}

impl Write for StagingStream {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.set_write_timeout(self.stream.write_timeout()?)?;
        self.stream.write(bytes)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.stream.flush()
    }
}

impl Duplex for StagingStream {
    fn try_clone(&self) -> std::io::Result<Box<dyn Duplex>> {
        Ok(Box::new(Self {
            stream: self.stream.try_clone()?,
            until: self.until,
        }))
    }
    fn shutdown(&self, how: std::net::Shutdown) -> std::io::Result<()> {
        self.stream.shutdown(how)
    }
    fn set_write_timeout(&self, timeout: Option<Duration>) -> std::io::Result<()> {
        self.stream.set_write_timeout(self.limit(timeout)?)
    }
    fn set_read_timeout(&self, timeout: Option<Duration>) -> std::io::Result<()> {
        self.stream.set_read_timeout(self.limit(timeout)?)
    }
    fn handle(&self) -> hex_client::guest::Handle<'_> {
        self.stream.as_fd()
    }
    fn set_nonblocking(&self, on: bool) -> std::io::Result<()> {
        self.stream.set_nonblocking(on)
    }
}

pub(super) fn ready(
    listener: &UnixListener,
    proc: &mut crate::linux::proc::PidfdProc,
    timeout: Duration,
) -> Result<(), Refusal> {
    let until = deadline(timeout)?;
    listener
        .set_nonblocking(true)
        .map_err(|e| Refusal::os("bounding guest readiness listener", e))?;
    loop {
        if proc.try_wait()?.is_some() {
            return Err(Refusal::Broken(
                "Firecracker ended before guest readiness".into(),
            ));
        }
        match listener.accept() {
            Ok((mut stream, _)) => {
                let mut frame = [0u8; hex_core::stream::GUEST_READY.len()];
                read_exact(&mut stream, &mut frame, until)?;
                if frame.as_slice() != hex_core::stream::GUEST_READY {
                    return Err(invalid("guest readiness frame disagrees"));
                }
                return Ok(());
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                let duration = left(until)?;
                let timespec = Timespec {
                    tv_sec: i64::try_from(duration.as_secs())
                        .map_err(|_| invalid("readiness deadline overflow"))?,
                    tv_nsec: i64::from(duration.subsec_nanos()),
                };
                let mut descriptors = [
                    PollFd::new(listener, PollFlags::IN),
                    PollFd::new(proc.pidfd(), PollFlags::IN),
                ];
                match rustix::event::poll(&mut descriptors, Some(&timespec)) {
                    Ok(0) => {
                        return Err(Refusal::os(
                            "waiting for guest readiness",
                            "boot deadline elapsed",
                        ));
                    }
                    Ok(_) | Err(rustix::io::Errno::INTR) => {}
                    Err(error) => return Err(Refusal::os("waiting for guest readiness", error)),
                }
            }
            Err(error) => return Err(Refusal::os("accepting guest readiness", error)),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Attachment {
    pub id: String,
    pub sbdf: String,
    pub guest_sbdf: String,
}

/// Readonly witness supplied by Hex's pinned Firecracker extension, never an ordinal guess.
pub(super) fn attachment(api: &Path, id: &str, timeout: Duration) -> Result<Attachment, Refusal> {
    if id
        .bytes()
        .any(|c| !c.is_ascii_alphanumeric() && !matches!(c, b'_' | b'-' | b':' | b'.'))
    {
        return Err(invalid("invalid attachment witness identifier"));
    }
    let until = deadline(timeout)?;
    let mut socket = connect(api, until)?;
    write(
        &mut socket,
        format!(
            "GET /device-passthrough/{id} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"
        )
        .as_bytes(),
        until,
    )?;
    let bound = hex_core::wire::MAX_MESSAGE;
    let room = bound
        .checked_add(1)
        .ok_or_else(|| exhausted("Firecracker API response", bound))?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(room)
        .map_err(|_| exhausted("Firecracker API response", room))?;
    bytes.resize(room, 0);
    let mut used = 0usize;
    let mut header_end = None;
    let mut response_end = None;
    loop {
        if response_end.is_some_and(|end| used == end) {
            break;
        }
        let end = response_end.unwrap_or(room);
        socket
            .set_read_timeout(Some(left(until)?))
            .map_err(|e| Refusal::os("bounding Firecracker API", e))?;
        let before = used;
        let tail = bytes
            .get_mut(used..end)
            .filter(|tail| !tail.is_empty())
            .ok_or_else(|| exhausted("Firecracker API response", bound))?;
        let count = socket
            .read(tail)
            .map_err(|e| Refusal::os("reading Firecracker API", e))?;
        if count == 0 {
            return Err(invalid(
                "attachment witness response ended before its Content-Length",
            ));
        }
        used = used
            .checked_add(count)
            .ok_or_else(|| invalid("API response count overflow"))?;
        if used > bound {
            return Err(exhausted("Firecracker API response", bound));
        }
        if header_end.is_none() {
            // Scan only new bytes plus the delimiter overlap; an adversarial one-byte sender
            // cannot make header recognition quadratic in the finite response byte budget.
            let overlap = before.saturating_sub(b"\r\n\r\n".len().saturating_sub(1));
            if let Some(index) = bytes.get(overlap..used).and_then(|part| {
                part.windows(b"\r\n\r\n".len())
                    .position(|part| part == b"\r\n\r\n")
            }) {
                let end = overlap
                    .checked_add(index)
                    .and_then(|index| index.checked_add(b"\r\n\r\n".len()))
                    .ok_or_else(|| invalid("HTTP header length overflow"))?;
                let header = bytes
                    .get(..end)
                    .ok_or_else(|| invalid("HTTP header is absent"))?;
                let length = content_length(header)?;
                let total = end
                    .checked_add(length)
                    .filter(|total| *total <= bound)
                    .ok_or_else(|| exhausted("Firecracker API response", bound))?;
                if used > total {
                    return Err(invalid(
                        "attachment witness has bytes beyond its Content-Length",
                    ));
                }
                header_end = Some(end);
                response_end = Some(total);
            }
        }
    }
    let end = header_end.ok_or_else(|| invalid("malformed Firecracker HTTP response"))?;
    let body = bytes
        .get(end..used)
        .ok_or_else(|| invalid("malformed Firecracker HTTP body"))?;
    serde_json::from_slice(body).map_err(|_| invalid("invalid Firecracker attachment witness"))
}

fn content_length(header: &[u8]) -> Result<usize, Refusal> {
    let text = std::str::from_utf8(header).map_err(|_| invalid("HTTP header is not UTF-8"))?;
    let mut lines = text.split("\r\n");
    if !lines
        .next()
        .is_some_and(|line| line.starts_with("HTTP/1.1 200 "))
    {
        return Err(invalid("Firecracker refused attachment witness"));
    }
    let mut length = None;
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| invalid("malformed HTTP header field"))?;
        if name.eq_ignore_ascii_case("transfer-encoding") {
            return Err(invalid("attachment witness cannot use Transfer-Encoding"));
        }
        if name.eq_ignore_ascii_case("content-length") {
            if length.is_some() {
                return Err(invalid("duplicate HTTP Content-Length"));
            }
            let value = value.trim_matches([' ', '\t']);
            if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(invalid("invalid HTTP Content-Length"));
            }
            length = Some(
                value
                    .parse::<usize>()
                    .map_err(|_| invalid("HTTP Content-Length overflow"))?,
            );
        }
    }
    length.ok_or_else(|| invalid("attachment witness needs an exact Content-Length"))
}

#[cfg(test)]
mod tests {
    use super::*;
    /// AC-T7.4.1: framing is finite on persistent HTTP connections and rejects ambiguous lengths.
    #[test]
    fn witness_response_requires_one_bounded_content_length() {
        assert_eq!(
            content_length(
                b"HTTP/1.1 200 OK\r\nContent-Length: 17\r\nConnection: keep-alive\r\n\r\n"
            )
            .unwrap(),
            17
        );
        for header in [
            b"HTTP/1.1 200 OK\r\n\r\n".as_slice(),
            b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n",
            b"HTTP/1.1 200 OK\r\nContent-Length: 1\r\ncontent-length: 1\r\n\r\n",
            b"HTTP/1.1 200 OK\r\nContent-Length: 1\r\nTransfer-Encoding: chunked\r\n\r\n",
            b"HTTP/1.1 200 OK\r\nContent-Length: +1\r\n\r\n",
            b"HTTP/1.1 200 OK\r\nContent-Length: 184467440737095516150\r\n\r\n",
        ] {
            assert!(content_length(header).is_err());
        }
    }
}
