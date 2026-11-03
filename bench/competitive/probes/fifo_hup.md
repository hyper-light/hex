# A poll on a FIFO reports a hang-up while its writer is opening it (goal 5 extended, step 6)

The competitive bench saw one start refused under load: "start i006b003224: reading the exec fifo failed: Resource
temporarily unavailable", the socket driver's second round of `20261103T064217Z` (goal 5 extended, step 6's
`reuse_pass.md`, run 6). A start
opened the exec FIFO's read end without blocking, polled it, and read the init's byte
(`crates/hex-runtime/src/linux/oci/run.rs`, `start_with`, before the design below). The container's init opens the
write end, writes one byte and closes it (`init.rs`). The read got EAGAIN after the poll had returned.

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
(GCC 16.2.1; source sha256 be80baa58f275140, this file before its `rdwr` mode, its read-only path the same; binary
bfe63dbbc0eb2885) and run on fedora, 6.19.10-300.fc44.x86_64,
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

**What it means for Hex.** A poll's hang-up on a FIFO opened read-only doesn't say its writer is gone, and the read
after it can get EAGAIN with the writer there; so the start no longer opens its end read-only (below). In the full
lifecycle the race needs the start's open to come before the init's, which a loaded host makes rare: 3,600,000 interleaved lifecycles of the
stress unit (`20261103T081843Z`, `20261103T082650Z`, E-cores 16-31) and 1,200,000 sustained ones before met none.

**Seen again, with goal 5 extended step 6's diagnostics (25ae3d7), in a lifecycle at concurrency 32.** A binary
built without the fix, a merge of steps 2, 3 and 6, refused a start after 3,325 of 20,000 lifecycles on fedora's
E-cores, 2026-11-03T10:03Z (`create-serial-checker-20261103T100249Z`): "start s003304: reading the exec fifo failed:
Resource temporarily unavailable (os error 11), after the poll's PollFlags(HUP); the fifo held Ok((1048807, 41631)),
the one at /run/hex-node/6a63ac065461cc09-s003304/exec.fifo Ok((1048807, 41631))". The poll's hang-up alone, the
FIFO the start held the one at its path, and a writer there with no byte: this race, not another reader, another
writer or another FIFO.

**What would falsify it.** A kernel whose `pipe_poll` judges the hang-up under the pipe's lock, or whose open raises
the writers before the counter, would show no hang-up alone here.

## Designed out: the start holds both of the FIFO's ends (2026-11-03)

Ada's call, through the lead: a start that never depends on `pipe_poll`'s hang-up, in place of one that reads past it.
A hang-up needs a FIFO with no writer (`fs/pipe.c:691-692`), and an error one with no reader (`:702-703`). The start
opens its end to read and to write: Linux never blocks that open and counts it a reader and a writer both
(`:1202-1216`), so the init's blocking open to write returns at once, and for as long as the start holds its end no
poll of it can report either. The init's end comes from its pidfd, polled beside the FIFO. The init's byte is counted
(`FIONREAD`, under the pipe's lock, `:625-637`) and never read, so every start that holds the FIFO sees it, and the
FIFO's removal says which of them reports the start (`run.rs`, `wait_for_byte`; decision 3 of
`docs/design/runtime-architecture.md` sets runc's and crun's rendezvous beside it).

`fifo_hup.c`'s `rdwr` mode opens its reader so. Built with `gcc -O2 -Wall -Wextra -pthread` (GCC 16.2.1; source
sha256 b6762266edf37723, binary 04da786d19de818e) and run in both modes on fedora on 2026-11-03 from 20:16Z, in the
unit `hex-g5x-s6-verify21-20261103T201127Z` on the E-cores after that unit's profiles, `nice -n 10`; one-minute load
6.1 at its start and 4.7 at its end.

| reader's open | reader, writer CPU | iterations | poll: input alone | input and hang-up | hang-up alone | then EAGAIN | then the byte |
|---|---|---|---|---|---|---|---|
| read-only | 16, 17 | 1,000,000 | 13,315 | 986,682 | 3 | 0 | 3 |
| read-only | 16, 24 | 1,000,000 | 50,144 | 949,855 | 1 | 0 | 1 |
| read-only | 20, 21 | 1,000,000 | 9,203 | 990,797 | 0 | 0 | 0 |
| read-only | 16, 17 | 5,000,000 | 63,347 | 4,936,645 | 8 | 0 | 8 |
| read-only | 16, 24 | 5,000,000 | 262,336 | 4,737,649 | 15 | 0 | 15 |
| read-only | 20, 21 | 5,000,000 | 70,376 | 4,929,615 | 9 | 4 | 5 |
| to read and to write | 16, 17 | 1,000,000 | 1,000,000 | 0 | 0 | 0 | 0 |
| to read and to write | 16, 24 | 1,000,000 | 1,000,000 | 0 | 0 | 0 | 0 |
| to read and to write | 20, 21 | 1,000,000 | 1,000,000 | 0 | 0 | 0 | 0 |
| to read and to write | 16, 17 | 5,000,000 | 5,000,000 | 0 | 0 | 0 | 0 |
| to read and to write | 16, 24 | 5,000,000 | 5,000,000 | 0 | 0 | 0 | 0 |
| to read and to write | 20, 21 | 5,000,000 | 5,000,000 | 0 | 0 | 0 | 0 |

Read-only, 36 of 18,000,000 polls reported a hang-up with the writer there, and 4 of the reads after them got EAGAIN.
Opened to read and to write, none of 18,000,000 did: every poll reported input alone. (Most read-only polls report
input and a hang-up both because the writer has written and closed by then.)

**The start's tests** (`run::tests`, AC-T7.1.4), in the same unit: the new start passes its 16, and hex-node its 26.
With the start's end opened read-only, as before, and nothing else changed, two fail: the poll a writer that came and
went leaves behind reports its hang-up (`a_writer_that_came_and_went_is_no_end_while_the_init_lives`), and an init
that ended without its byte reads as end-of-file rather than as its end
(`an_init_that_ended_without_its_byte_is_stopped_by_its_pidfd`).

The design's last two tests, in the unit `hex-g5x-s6-verify22-20261103T212847Z`: the start passes all 18, and hex-node
its 26. With the byte read rather than counted, the start that loses two at once waits out its patience (round 0 of
`of_two_starts_at_once_one_starts_and_the_other_finds_the_container_running`: "Ok(()), Err(Broken("container c0 did
not take the start within 10s"))"). Without the start-time check, a pid given to a later process is taken for the init
(`a_pid_given_to_a_later_process_is_never_taken_for_the_init`: "c1: Ok(())").

**Found by stress, in the unit `hex-g5x-s6-fdh10-20261103T215657Z`:** the lib tests repeated, the two-starts test ended
"round 58: Ok(()), Err(Broken("container c58 did not take the start within 10s"))". A FIFO's pipe lives only while
something holds it open (`put_pipe_info`, `fs/pipe.c:709-721`), and an open with none makes a new one (`fifo_open`,
`:1129-1147`). So an open whose lookup came before the winner's removal, and which landed after the winner's close, held
a new pipe, with no byte ever to come. The winner removes the FIFO before it closes it, so the start now looks for the
FIFO's name after its open, and a FIFO with none says the container is running
(`an_open_crossing_anothers_removal_and_close_finds_no_name`).

**What would falsify it.** A hang-up or an error polled from a FIFO held to read and to write: a kernel whose
`pipe_poll` reported either with a writer and a reader there would show it in the rows above opened so.
