//! A recorded workload, as text: the cluster it ran on and the work that arrived, so hex-sim can
//! replay a real day (G2 step 3; `AC-T7.4.3`'s "recorded kiln workload").
//!
//! ```text
//! # source: <where and when it was recorded>
//! node,<cpu_milli>,<mem_bytes>,<gpus>,<background_milli>
//! leg,<arrive_ns>,<runs_for_ns>,<cpu_milli>,<mem_bytes>,<gpus>,<plain|build|benchmark|session>
//! ```
//!
//! Lines starting `#` are comments, and blank lines are skipped. Every node comes before the first
//! leg, and legs come in arrival order. Ids are given by position. The parser is a pure function
//! over bounded input (CLAUDE.md §2): past [`Limits`], and at the first line it can't read, it refuses
//! with a typed [`TraceError`] naming the line.

use crate::model::{Cluster, Kind, NodeId, NodeSpec, Resources, WorkloadId, WorkloadSpec};

/// What a trace may hold. Past any bound the parse is refused, never truncated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub max_bytes: usize,
    pub max_nodes: usize,
    pub max_legs: usize,
}

impl Default for Limits {
    /// A day of kiln CI with room to spare: tens of thousands of legs on tens of runners.
    fn default() -> Self {
        Self {
            max_bytes: 64 << 20,
            max_nodes: 100_000,
            max_legs: 1_000_000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TraceError {
    #[error("the trace is {0} bytes, over its bound")]
    TooLarge(usize),
    #[error("line {line}: {why}")]
    Line { line: usize, why: &'static str },
    #[error("more than {0} nodes")]
    TooManyNodes(usize),
    #[error("more than {0} legs")]
    TooManyLegs(usize),
    #[error("the trace names no node")]
    NoNodes,
}

fn bad(line: usize, why: &'static str) -> TraceError {
    TraceError::Line { line, why }
}

pub fn parse(text: &str, limits: &Limits) -> Result<Cluster, TraceError> {
    if text.len() > limits.max_bytes {
        return Err(TraceError::TooLarge(text.len()));
    }
    let mut nodes: Vec<NodeSpec> = Vec::new();
    let mut workloads: Vec<WorkloadSpec> = Vec::new();
    for (at, raw) in text.lines().enumerate() {
        let line = at.saturating_add(1);
        let row = raw.trim();
        if row.is_empty() || row.starts_with('#') {
            continue;
        }
        let mut fields = row.split(',').map(str::trim);
        match fields.next() {
            Some("node") => {
                if !workloads.is_empty() {
                    return Err(bad(line, "a node after the first leg"));
                }
                if nodes.len() >= limits.max_nodes {
                    return Err(TraceError::TooManyNodes(limits.max_nodes));
                }
                let id = NodeId(u32::try_from(nodes.len()).map_err(|_| bad(line, "node id"))?);
                let cpu_milli = number(&mut fields, line)?;
                let mem_bytes = number(&mut fields, line)?;
                let gpus = u32::try_from(number(&mut fields, line)?)
                    .map_err(|_| bad(line, "gpus past u32"))?;
                let background_milli = number(&mut fields, line)?;
                end(&mut fields, line)?;
                if cpu_milli == 0 {
                    return Err(bad(line, "a node with no CPU"));
                }
                nodes.push(NodeSpec {
                    id,
                    capacity: Resources {
                        cpu_milli,
                        mem_bytes,
                        gpus,
                    },
                    background_milli,
                });
            }
            Some("leg") => {
                if workloads.len() >= limits.max_legs {
                    return Err(TraceError::TooManyLegs(limits.max_legs));
                }
                let id =
                    WorkloadId(u64::try_from(workloads.len()).map_err(|_| bad(line, "leg id"))?);
                let arrives = number(&mut fields, line)?;
                let runs_for = number(&mut fields, line)?;
                let cpu_milli = number(&mut fields, line)?;
                let mem_bytes = number(&mut fields, line)?;
                let gpus = u32::try_from(number(&mut fields, line)?)
                    .map_err(|_| bad(line, "gpus past u32"))?;
                let kind = match fields.next() {
                    Some("plain") => Kind::Plain,
                    Some("build") => Kind::Build,
                    Some("benchmark") => Kind::Benchmark,
                    Some("session") => Kind::Session,
                    Some(_) => {
                        return Err(bad(line, "a kind not plain, build, benchmark or session"));
                    }
                    None => return Err(bad(line, "a missing field")),
                };
                end(&mut fields, line)?;
                if workloads.last().is_some_and(|w| w.arrives > arrives) {
                    return Err(bad(line, "a leg arriving before the one above it"));
                }
                if runs_for == 0 {
                    return Err(bad(line, "a leg that runs for no time"));
                }
                workloads.push(WorkloadSpec {
                    id,
                    need: Resources {
                        cpu_milli,
                        mem_bytes,
                        gpus,
                    },
                    kind,
                    arrives,
                    runs_for: Some(runs_for),
                });
            }
            Some(_) | None => return Err(bad(line, "a line that is neither node nor leg")),
        }
    }
    if nodes.is_empty() {
        return Err(TraceError::NoNodes);
    }
    Ok(Cluster { nodes, workloads })
}

fn number<'a>(fields: &mut impl Iterator<Item = &'a str>, line: usize) -> Result<u64, TraceError> {
    let field = fields.next().ok_or(bad(line, "a missing field"))?;
    if field.is_empty() || !field.bytes().all(|b| b.is_ascii_digit()) {
        return Err(bad(line, "a field that is not a whole number"));
    }
    field
        .parse::<u64>()
        .map_err(|_| bad(line, "a number past u64"))
}

fn end<'a>(fields: &mut impl Iterator<Item = &'a str>, line: usize) -> Result<(), TraceError> {
    match fields.next() {
        None => Ok(()),
        Some(_) => Err(bad(line, "more fields than the line's kind has")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SMALL: &str = "# source: test\nnode,4000,8589934592,0,500\nnode,8000,17179869184,2,0\n\nleg,0,1000,1000,1024,0,build\nleg,10,2000,500,1024,1,plain\nleg,10,500,250,1024,0,session\n";

    /// G2 step 3: a well-formed trace reads back as its nodes and legs, ids by position.
    #[test]
    fn a_trace_reads_as_its_cluster_and_legs() {
        let c = parse(SMALL, &Limits::default()).unwrap();
        assert_eq!(c.nodes.len(), 2);
        assert_eq!(c.nodes[1].capacity.gpus, 2);
        assert_eq!(c.nodes[0].background_milli, 500);
        assert_eq!(c.workloads.len(), 3);
        assert_eq!(
            (c.workloads[2].id, c.workloads[2].kind),
            (WorkloadId(2), Kind::Session)
        );
        assert_eq!(c.workloads[1].runs_for, Some(2000));
    }

    /// CLAUDE.md §2 and §6 (hostile input): each malformed trace is refused, typed, naming its line.
    #[test]
    fn malformed_traces_are_refused_by_line() {
        let cases = [
            (
                "node,1,1,0,0\nleg,5,1,1,1,0,plain\nleg,4,1,1,1,0,plain\n",
                3,
            ),
            ("node,1,1,0,0\nleg,0,1,1,1,0,plain\nnode,1,1,0,0\n", 3),
            ("node,1,1,0\n", 1),
            ("node,1,1,0,0,9\n", 1),
            ("node,-1,1,0,0\n", 1),
            ("node,1,1,99999999999,0\n", 1),
            ("node,1,1,0,0\nleg,0,1,1,1,0,daemon\n", 2),
            ("node,1,1,0,0\nleg,0,0,1,1,0,plain\n", 2),
            ("node,0,1,0,0\n", 1),
            ("pod,1\n", 1),
            ("node,18446744073709551616,1,0,0\n", 1),
        ];
        for (text, line) in cases {
            match parse(text, &Limits::default()) {
                Err(TraceError::Line { line: l, .. }) => assert_eq!(l, line, "{text:?}"),
                other => panic!("{text:?}: {other:?}"),
            }
        }
        assert_eq!(
            parse("# nothing\n", &Limits::default()),
            Err(TraceError::NoNodes)
        );
    }

    #[test]
    fn traces_past_their_bounds_are_refused() {
        let limits = Limits {
            max_bytes: 10,
            max_nodes: 1,
            max_legs: 1,
        };
        assert_eq!(
            parse(SMALL, &limits),
            Err(TraceError::TooLarge(SMALL.len()))
        );
        let limits = Limits {
            max_bytes: 1 << 20,
            ..limits
        };
        assert_eq!(parse(SMALL, &limits), Err(TraceError::TooManyNodes(1)));
        let one_node = "node,1,1,0,0\nleg,0,1,1,1,0,plain\nleg,0,1,1,1,0,plain\n";
        assert_eq!(parse(one_node, &limits), Err(TraceError::TooManyLegs(1)));
    }

    proptest::proptest! {
        /// Hostile input: any bytes at all are read or refused, never a panic.
        #[test]
        fn any_text_is_read_or_refused(text in "\\PC*") {
            let _ = parse(&text, &Limits::default());
        }

        /// Hostile input shaped like a trace: lines of the right words and numbers in any order.
        #[test]
        fn any_trace_shaped_text_is_read_or_refused(
            rows in proptest::collection::vec("(node|leg|#|x)(,[0-9]{0,22}){0,7}(,(plain|build|benchmark|session|zz))?", 0..40)
        ) {
            let _ = parse(&rows.join("\n"), &Limits::default());
        }
    }
}
