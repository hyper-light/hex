//! Joint plan step 14 (2c; ROOT, 2026-11-13): an append-only native-helper request for the slowest stop response the
//! helper's managed CPU control has seen in its own holds. Original (red): no such call is read or written.
use super::*;

/// The request asking the slowest response seen. Original (red): refused.
pub fn encode(version: u16, id: u64) -> Result<Vec<u8>, Fault> {
    let _ = (version, id);
    Err(invalid("native CPU response request"))
}

/// The response request in `bytes`. Original (red): refused.
pub fn read(bytes: &[u8], fds: usize) -> Result<(u16, u64), Fault> {
    let _ = (bytes, fds);
    Err(invalid("native CPU response request"))
}

/// The answer naming `slowest`. Original (red): refused.
pub fn encode_answer(version: u16, id: u64, slowest: Option<Duration>) -> Result<Vec<u8>, Fault> {
    let _ = (version, id, slowest);
    Err(invalid("native CPU response answer"))
}

/// The response answer in `bytes`. Original (red): refused.
pub fn read_answer(bytes: &[u8], fds: usize) -> Result<(u16, u64, Option<Duration>), Fault> {
    let _ = (bytes, fds);
    Err(invalid("native CPU response answer"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Joint plan step 14 (2c): the slowest response a helper has seen crosses to its node whole and alone. Do encode
    /// the request and read it back, with a descriptor and cut short at every length; then answer it with 3.5 ms and
    /// with none (no hold seen yet), each read back, cut short, and read by another kind's reader. Expect each back
    /// exactly and every other refused.
    #[test]
    fn w14_the_slowest_response_seen_crosses_whole_and_alone() {
        let bytes = encode(VERSION, 41).unwrap();
        assert_eq!(read(&bytes, 0).unwrap(), (VERSION, 41));
        assert!(read(&bytes, 1).is_err());
        for size in 0..bytes.len() {
            assert!(read(&bytes[..size], 0).is_err());
        }
        assert!(read_request(&bytes, 0).is_err());
        assert!(rate::read(&bytes, 0).is_err());
        for slowest in [Some(Duration::from_micros(3_500)), None] {
            let answer = encode_answer(VERSION, 41, slowest).unwrap();
            assert_eq!(read_answer(&answer, 0).unwrap(), (VERSION, 41, slowest));
            assert!(read_answer(&answer, 1).is_err());
            for size in 0..answer.len() {
                assert!(read_answer(&answer[..size], 0).is_err());
            }
            assert!(super::super::read_answer(&answer, 0).is_err());
            assert!(read(&answer, 0).is_err());
        }
        assert!(encode_answer(VERSION, 41, Some(Duration::ZERO)).is_err());
    }
}
