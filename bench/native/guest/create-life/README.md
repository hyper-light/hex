# HEX-GUEST-22: a box's life through a real guest, the layer store against whole-root staging

`crates/hex-cli/tests/guest_create_bench.rs` creates and deletes an `oci` box of each image in turn, through
`GuestRuntime` and a real Shards guest, and times each create, its delete and the two together (the store
moves cost between them: whole-root staging removed a box's root at its delete). The comparison of record is
an A/B interleaved round by round, taken under the host's own co-tenant load (Ada's rule: never by waiting for
quiet):

1. `arm.sh` stages each arm ahead: a worktree at its commit, the same bench file in both (it needs no store
   API), its guest image and its bench binary. Whole-root staging's arm stages from the images' own
   `layer<i>.tar.gz` (`layers.txt`); the store's from the same files linked as `blobs/sha256/<hex>`
   (`blobs.txt`), the names it takes digests from.
2. `pair.sh` starts both benches, each its own guest, and gives them turns a round at a time in ABBA order,
   recording before each turn the host's load, its running Hex units and its top CPU users (`load.txt`).
3. `summary.py` judges on the pairs: each arm's p50 and p99 over the same rounds (round 0, each guest's first
   create after its boot, aside), and the median of B − A.

The sizings: the guest's memory past Shards' least is `MEMX` × the images' layers unpacked, its tmpfs (where
it stages) half its memory. "Room" is the smallest store that holds every image's real footprint plus the
room a stage needs (the largest stage's real room, `.layers/.need`): for alpine, caddy and grafana, about
6×. 2× is pressure, 4× a partial fit.

No round is dropped unless by a rule stated before the run.
