# A poll on a FIFO reports a hang-up while its writer is opening it (goal 5 extended, step 6)

The competitive bench saw one start refused under load: "start i006b003224: reading the exec fifo failed: Resource
temporarily unavailable", the socket driver's second round of `20261103T064217Z` (goal 5 extended, step 6's
`reuse_pass.md`, run 6). A start
opens the exec FIFO's read end without blocking, polls it, and reads the init's byte
(`crates/hex-runtime/src/linux/oci/run.rs`, `start_with`). The container's init opens the write end, writes one byte
and closes it (`init.rs`). The read got EAGAIN after the poll had returned.

**Why** (Linux v6.19, `fs/pipe.c`, read at the tag):
- `pipe_poll` reads the pipe's state without its lock (`:670`). It reports a hang-up when the pipe has no writer and
  the writers' counter has moved past the one the reader's open recorded (`:691-692`). A reader's open made without
  blocking, with no writer there, records the counter (`:1172-1174`).
- An open for writing raises the counter, then the writers (`:1192-1193`). A poll whose two loads fall across those
  two stores sees no writer and a moved counter: a hang-up, with the writer there and no byte yet.
- The read after it finds the pipe empty with a writer open, and returns EAGAIN to a non-blocking reader
  (`:374-376`). With no writer it would return end-of-file (`:370-371`).
- So a start that opens its end before the init opens the init's own, and polls as the init opens, sees a hang-up
  that isn't one. The same code is in v7.2 (`:789`, `:1290-1291`) and in master as read on 2026-11-03 (`:774`,
  `:1275-1276`).

`fifo_hup.c` does what the start and the init do, on two pinned threads. Each iteration the reader opens the FIFO
with no writer open, tells the writer to go, waits a random 0 to 2047 spins, then polls and reads. The writer waits a
random 0 to 2047 spins after the go, then opens, writes its byte and closes. The writer always writes before it
closes, so every poll that reports a hang-up without input is the race. The probe counts what the read after each
one got; a read that got EAGAIN polls and reads again until it has the byte. Built with `gcc -O2 -Wall -pthread`
(GCC 16.2.1; source sha256 be80baa58f275140, binary bfe63dbbc0eb2885) and run on fedora, 6.19.10-300.fc44.x86_64,
on 2026-11-03 under `nice -n 10`. The E-cores were shared with the stress unit
`hex-g5x-s6-fifo-20261103T081757Z` until 08:34:51Z, load 26 to 10.

| when (UTC) | reader, writer CPU | iterations | poll: input (and hang-up) | hang-up alone | then EAGAIN | then the byte |
|---|---|---|---|---|---|---|
| 08:32 | 16, 17 (one L2) | 1,000,000 | 999,999 | 1 | 0 | 1 |
| 08:32 | 16, 24 (two L2s) | 1,000,000 | 999,997 | 3 | 0 | 3 |
| 08:32 | 20, 21 (one L2) | 1,000,000 | 999,995 | 5 | 2 | 3 |
| 08:34 | 16, 17 | 5,000,000 | 4,999,988 | 12 | 5 | 7 |
| 08:34 | 16, 24 | 5,000,000 | 4,999,999 | 1 | 0 | 1 |
| 08:35 | 20, 21 | 5,000,000 | 5,000,000 | 0 | 0 | 0 |

Over 18,000,000 polls, 22 reported a hang-up with the writer there. 7 of the reads after them got EAGAIN, the start's
refusal; the other 15 found the byte, written between the poll and the read. No read got end-of-file and no poll
timed out.

**What it means for Hex.** A poll's hang-up on the exec FIFO doesn't say the init is gone; the read does.
End-of-file is an init that opened its end and died. EAGAIN is an init whose end is open and whose byte hasn't come,
so the start polls again within the same patience (`run.rs`, `take_byte`). In the full lifecycle the race needs the
start's open to come before the init's, which a loaded host makes rare: 3,600,000 interleaved lifecycles of the
stress unit (`20261103T081843Z`, `20261103T082650Z`, E-cores 16-31) and 1,200,000 sustained ones before met none.

**Seen again, with 25ae3d7's diagnostics, in a lifecycle at concurrency 32.** A binary without the fix
(`create_serial.md` run 18's mr8, on `reuse_pass.md` run 8's merge) refused a start after 3,325 of 20,000
lifecycles on fedora's E-cores, 2026-11-03T10:03Z (`create-serial-checker-20261103T100249Z`): "start s003304:
reading the exec fifo failed: Resource temporarily unavailable (os error 11), after the poll's PollFlags(HUP); the
fifo held Ok((1048807, 41631)), the one at /run/hex-node/6a63ac065461cc09-s003304/exec.fifo Ok((1048807, 41631))".
The poll's hang-up alone, the FIFO the start held the one at its path, and a writer there with no byte: this race,
not another reader, another writer or another FIFO.

**What would falsify it.** A kernel whose `pipe_poll` judges the hang-up under the pipe's lock, or whose open raises
the writers before the counter, would show no hang-up alone here.
