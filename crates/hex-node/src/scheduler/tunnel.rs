//! Authenticated peer shell (LB03): SSH supplies transport identity/encryption; Hex supplies its
//! existing hello, wire identity, bounded frames and typed calls. No submitted text becomes a command.

use std::io::{Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use hex_api::Refusal;
use hex_core::{schedule, stream, wire};

use super::Peer;

fn io(op: &'static str) -> impl Fn(std::io::Error) -> Refusal {
    move |e| Refusal::os(op, e)
}

pub fn read_frame(input: &mut impl Read) -> Result<Option<Vec<u8>>, Refusal> {
    let mut prefix = [0u8; size_of::<u32>()];
    // Clean EOF is allowed only between frames. A partial prefix/body is a broken connection.
    let (first, rest) = prefix
        .split_first_mut()
        .ok_or_else(|| Refusal::Broken("empty frame prefix".into()))?;
    loop {
        match input.read(std::slice::from_mut(first)) {
            Ok(0) => return Ok(None),
            Ok(_) => break,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(io("reading peer frame")(e)),
        }
    }
    input
        .read_exact(rest)
        .map_err(io("reading peer frame prefix"))?;
    let count =
        usize::try_from(u32::from_be_bytes(prefix)).map_err(|e| Refusal::Broken(e.to_string()))?;
    if count > wire::MAX_MESSAGE {
        return Err(Refusal::Exhausted {
            what: "peer message bytes",
            bound: wire::MAX_MESSAGE,
        });
    }
    let mut bytes = vec![0; count];
    input
        .read_exact(&mut bytes)
        .map_err(io("reading peer message"))?;
    Ok(Some(bytes))
}

pub fn write_frame(output: &mut impl Write, bytes: &[u8]) -> Result<(), Refusal> {
    output
        .write_all(&stream::frame(bytes, wire::MAX_MESSAGE).map_err(Refusal::from)?)
        .and_then(|()| output.flush())
        .map_err(io("writing peer message"))
}

fn answer(bytes: &[u8]) -> Result<(wire::Header, wire::Answer), Refusal> {
    wire::read_answer(
        &wire::Received {
            bytes,
            size: bytes.len(),
            descriptors: 0,
            descriptors_cut: false,
        },
        wire::MAX_MESSAGE,
    )
    .map_err(Into::into)
}

/// One bounded ordered command lane per configured peer. Runtime program messages and stdio keep
/// using hex-client's common reader; this tunnel forwards only scheduler control to that reader.
pub struct Connection {
    child: Child,
    input: ChildStdin,
    output: ChildStdout,
    next: u64,
}

impl Connection {
    pub fn open(peer: &Peer) -> Result<Self, Refusal> {
        if peer.host.is_empty()
            || peer.host.starts_with('-')
            || peer.host.contains(['\n', '\r', '\0'])
            || peer.command.is_empty()
        {
            return Err(Refusal::Invalid {
                what: "SSH peer",
                why: "needs a configured host and remote tunnel command".into(),
            });
        }
        let mut child = Command::new("ssh")
            .args([
                "-T",
                "-o",
                "BatchMode=yes",
                "-o",
                "StrictHostKeyChecking=yes",
                "--",
                &peer.host,
                &peer.command,
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(io("opening authenticated peer tunnel"))?;
        let Some(input) = child.stdin.take() else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(Refusal::Broken("SSH stdin was not piped".into()));
        };
        let Some(output) = child.stdout.take() else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(Refusal::Broken("SSH stdout was not piped".into()));
        };
        let mut conn = Self {
            child,
            input,
            output,
            next: 0,
        };
        let bytes = wire::request(
            0,
            &wire::Request::Hello {
                build: wire::build(),
            },
            wire::MAX_MESSAGE,
        )
        .map_err(Refusal::from)?;
        write_frame(&mut conn.input, &bytes)?;
        let bytes = read_frame(&mut conn.output)?
            .ok_or_else(|| Refusal::Broken("SSH peer ended before hello".into()))?;
        match answer(&bytes)? {
            (wire::Header { id: 0, .. }, wire::Answer::Hello(_)) => Ok(conn),
            (_, wire::Answer::Refused(why)) => Err(why),
            _ => Err(Refusal::Broken("peer did not answer its hello".into())),
        }
    }

    pub fn call(&mut self, command: schedule::Command) -> Result<schedule::Reply, Refusal> {
        self.next = self.next.checked_add(1).ok_or(Refusal::Exhausted {
            what: "peer request IDs",
            bound: usize::MAX,
        })?;
        let bytes = wire::request(
            self.next,
            &wire::Request::Call(wire::Call::Schedule(Box::new(command))),
            wire::MAX_MESSAGE,
        )
        .map_err(Refusal::from)?;
        write_frame(&mut self.input, &bytes)?;
        let bytes = read_frame(&mut self.output)?
            .ok_or_else(|| Refusal::Broken("peer tunnel closed".into()))?;
        let (header, answer) = answer(&bytes)?;
        if header.id != self.next {
            return Err(Refusal::Broken("peer answered a different request".into()));
        }
        match answer {
            wire::Answer::Scheduled(reply) => Ok(*reply),
            wire::Answer::Refused(why) => Err(why),
            _ => Err(Refusal::Broken(
                "peer did not answer scheduler control".into(),
            )),
        }
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Remote end of SSH's authenticated stdio channel. The local socket enforces its normal owner
/// credentials and every call uses hex-client's common reader and worker admission.
pub fn serve(path: &Path, input: &mut impl Read, output: &mut impl Write) -> Result<(), Refusal> {
    let client = hex_client::Client::connect(path)?;
    let mut greeted = false;
    while let Some(bytes) = read_frame(input)? {
        let (id, answer, finish) = match wire::read_request_apart(&bytes, wire::MAX_MESSAGE) {
            Ok((header, wire::Request::Hello { .. })) => {
                greeted = true;
                (
                    header.id,
                    wire::Answer::Hello(wire::Hello {
                        build: wire::build(),
                        classes: Vec::new(),
                        limits: client.limits(),
                        budgets: client.budgets(),
                    }),
                    false,
                )
            }
            Err(wire::Bad {
                id,
                fault: wire::Fault::Wire { .. },
            }) => (
                id,
                wire::Answer::Hello(wire::Hello {
                    build: wire::build(),
                    classes: Vec::new(),
                    limits: client.limits(),
                    budgets: client.budgets(),
                }),
                true,
            ),
            Ok((header, wire::Request::Call(wire::Call::Schedule(command)))) if greeted => (
                header.id,
                client
                    .schedule(*command)
                    .map(|reply| wire::Answer::Scheduled(Box::new(reply)))
                    .unwrap_or_else(wire::Answer::Refused),
                false,
            ),
            Ok((header, _)) => (
                header.id,
                wire::Answer::Refused(Refusal::Invalid {
                    what: "scheduler tunnel",
                    why: "hello then scheduler control are required".into(),
                }),
                true,
            ),
            Err(bad) => (bad.id, wire::Answer::Refused(bad.fault.into()), true),
        };
        write_frame(
            output,
            &wire::answer(id, &answer, wire::MAX_MESSAGE).map_err(Refusal::from)?,
        )?;
        if finish {
            break;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lb03_peer_frame_refuses_length_before_body() {
        let count = u32::try_from(wire::MAX_MESSAGE + 1).unwrap().to_be_bytes();
        assert!(matches!(
            read_frame(&mut &count[..]),
            Err(Refusal::Exhausted { .. })
        ));
        assert!(read_frame(&mut &count[..2]).is_err());
        assert_eq!(read_frame(&mut &[][..]).unwrap(), None);
    }
    #[test]
    fn lb03_peer_frame_is_the_existing_control_frame() {
        let bytes = wire::request(
            7,
            &wire::Request::Hello {
                build: wire::build(),
            },
            wire::MAX_MESSAGE,
        )
        .unwrap();
        let mut framed = Vec::new();
        write_frame(&mut framed, &bytes).unwrap();
        assert_eq!(framed, stream::frame(&bytes, wire::MAX_MESSAGE).unwrap());
        assert_eq!(read_frame(&mut &framed[..]).unwrap(), Some(bytes));
    }
}
