# One differential suite on every OS: the oracle and the comparison

The question (native plan step 7, CLAUDE.md §8): what makes "the same suite passes on Linux, macOS and
Windows" mean the classes behave alike, rather than that each passes checks written for it.

## 1. Differential testing: two comparable systems, one input, compare the outputs

US patent 5,754,860, "Method and apparatus for software testing using a differential testing technique to
test compilers" (William M. McKeeman, August C. Reinig, Andrew Payne; Digital Equipment Corporation; filed
1996-07-23, granted 1998-05-19; https://patents.google.com/patent/US5754860, accessed 2026-11-05). `DESIGN`:
the same conforming program is given to two language processors and their outputs compared; a test fails
"if the corresponding outputs 56a and 56b are not equivalent", which marks a probable fault in at least
one of them. The technique's paper (McKeeman, "Differential Testing for Software", Digital Technical
Journal 10(1):100-107, 1998) was not found online to read; it is named here by its bibliography only
(dblp, https://dblp.org/pid/40/4856), and nothing below rests on it.

What Hex takes from it: the comparison needs an equivalence over outputs that each system can meet. Raw
outputs differ by design across OSes (a refusal's wording, a pid, how Windows reports a KILL), so each
output is normalized to what the contract promises before it is compared.

## 2. The oracle Hex already has: the simulator

`hex_api::sim::SimRuntime` (crates/hex-api/src/sim.rs:1-8) is the contract as a model: boxes in memory,
bounds enforced as the kernel would. It already passes the conformance suite with each way of making a box
(crates/hex-api/src/sim.rs, `the_simulator_passes_the_conformance_suite`), and every real class passes
the same cases (`hex_api::conformance`, HEX-RT-1..13). Playing one script on the simulator and on a real
class, and comparing step by step, makes the simulator the second system: a real class that differs from
it at a step either breaks the contract or shows the model wrong, and the design says which is fixed.

## 3. What a comparison can't ask of an OS

- Timing: a real runtime reports an exit or a kill count after the event; the simulator at once. A step
  that reads a box therefore reads again until it shows the oracle's answer or `PATIENCE` passes
  (crates/hex-api/src/conformance.rs, `PATIENCE`), as CLAUDE.md §4 asks (wait on the fact, bounded).
- How a bound ends a program: Linux and the simulator end a program over its memory bound by KILL;
  Windows refuses the commit and the program exits with a failure (bench/native/windows/RESULT.md, "The
  memory peak overshoots the commit limit by a bounded amount"). Both are "didn't exit 0".
- How delete ends a program: KILL everywhere, which Windows reports through its own exit code mapping
  (crates/hex-runtime/src/windows/process.rs, `delete`). The comparison asks only "ended by a signal".
- A member's own refused fork: Windows keeps no §3-clean count
  (docs/research/runtime/windows-pids-refused.md), so the process bound stays in the conformance suite's
  HEX-RT-6 with its per-platform rule and out of the scenarios.

## 4. A container's init and the signals it takes

pid_namespaces(7) (Linux man-pages 6.19, 2026-05-13, https://man7.org/linux/man-pages/man7/pid_namespaces.7.html,
accessed 2026-11-05), `DESIGN`: a process in an ancestor namespace can "send signals to the "init" process
of a child PID namespace only if the "init" process" "has established a handler for that signal", and
"SIGKILL or SIGSTOP are treated exceptionally": "these signals are forcibly delivered when sent from an
ancestor PID namespace." `hex selftest-box wait` installs no handler (crates/hex-cli/src/selftest.rs:51-53,
it parks), so a TERM sent to an `oci` box ends its exec'd programs and not its init. The suite's first
fedora run showed it (HEX-DIFF-1 and 2 over the node's socket: the box stayed Running with no exit after
TERM, where the simulator showed it Empty with Signal(15)), and the simulator now models the rule.
