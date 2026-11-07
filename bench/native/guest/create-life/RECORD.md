# HEX-GUEST-22 of record: the layer store against whole-root staging

A box's life (create, then delete, through `GuestRuntime` and a real Shards guest), per image, alpine, caddy
and grafana interleaved within each round. Arms: **A** = whole-root staging, 7fc4aa4 (the images' own
`layer<i>.tar.gz`); **B** = the layer store (the images' blobs, `blobs/sha256/<hex>`). Each arm uses the same
bench file and its own live guest, and the two take turns a round at a time in ABBA order (README). Each
round's co-tenant load is recorded beside it (`load.txt`). Round 0, each guest's first create after its boot,
is aside. **No round is dropped**: that rule was stated before every run below. All times are ms; p50 and p99
are by nearest rank; B−A is the median of the paired differences.

Host: adas-mac-studio-2.local, Apple M4 Max, 14 cores, 36 GiB, macOS, HVF. Guest: 1 vCPU, aarch64 kernel
6.18.48 (Shards' release), Shards a0cced87 debug build. Raw outputs (`A.out`, `B.out`, `load.txt`,
`summary.txt`) are under the launchd job's folder, named per run below.

## Sizing: what "room" is

The guest stages on a tmpfs, half its memory, and its memory past Shards' least is MEMX × the images'
layers unpacked. Room, by the lead's rule, is defined by the data: the smallest store that holds every image's
real footprint plus the learned reserve, which is the room the largest stage takes (`.layers/.need`, the drop
in the file system's free room across grafana's stage, page-rounded, about 1.5 GB). Σ real footprints
(≈ 1.6 GB) + need (≈ 1.5 GB) ≈ 3.1 GB, which takes about 6× (tmpfs ≈ 4 GB); 4× (≈ 2.9 GB) is a partial fit,
and 2× is pressure.

## N, and what a p99 claim means here

A sample's maximum bounds the true p99 with probability 1 − 0.99ⁿ. A p99 claim at 95% needs 0.99ⁿ ≤ 0.05,
so n ≥ 299 (Hex's (1−f)ⁿ rule; s4's criterion 2 takes the same N). The 2× run takes 300 rounds, 299 pairs.
The 4× and 6× runs take 31 rounds, 30 pairs: their p50 verdicts stand on 30 pairs, since their margins are
orders of magnitude, and **their p99 figures are the max of 30**, which bounds the true p99 only with
probability 1 − 0.99³⁰ ≈ 26%.

## 2× (pressure): 299 pairs

Run: launchd hex.ns5route.pairs2x.20261107T014622Z, 2026-11-07 01:47-02:24Z. B = e9df8ca. Co-tenancy over
600 turns: load 2.73 (min), 4.16 (p50), 15.50 (max); other Hex jobs beside it in 65 turns (hex.ns5c
×3, Shards end-to-end).

| image | | A p50 | A p99 | B p50 | B p99 | B−A |
|---|---|---|---|---|---|---|
| grafana | create | 4,223.1 | 6,519.6 | 2,465.7 | 3,866.6 | −1,758.6 |
| | delete | 48.5 | 57.7 | 34.5 | 40.8 | −14.0 |
| | **life** | 4,271.7 | 6,576.0 | **2,499.9** | **3,908.4** | **−1,772.2** |
| alpine | create | 30.2 | 37.3 | 30.3 | 33.6 | +0.0 |
| | delete | 1.5 | 1.8 | 1.6 | 1.9 | +0.0 |
| | **life** | 31.7 | 39.1 | **31.8** | **35.6** | **+0.0** |
| caddy | create | 205.3 | 236.6 | 204.8 | 229.0 | −0.5 |
| | delete | 2.9 | 3.4 | 3.0 | 3.5 | +0.1 |
| | **life** | 208.2 | 239.9 | **207.7** | **232.4** | **−0.4** |

Under pressure, every layer of alpine and caddy goes into the box (the store can't leave the room a stage
needs), and their lives match whole-root staging's: a paired median of +0.0 and −0.4 ms, with p99 better.
grafana's base layers stay cached, and its life is 1.8 s shorter.

## 4× (partial fit) and 6× (room): 30 pairs

Run: launchd hex.ns5route.pairs.20261107T012411Z, 2026-11-07 01:25-01:34Z. B = 5dc783c (e9df8ca changed only
the opening's bookkeeping, measured at 0.13 ms in all). Load: 3.10-4.36 at 4×, 2.91-6.16 at 6×; no other Hex
job ran beside 4× or 6×. The p99 columns are the **max of 30**.

| sizing | image | A life p50 | A max of 30 | B life p50 | B max of 30 | B−A |
|---|---|---|---|---|---|---|
| 4× | grafana | 4,259.5 | 4,344.3 | **2.6** | **2.9** | −4,261.5 |
| | alpine | 31.5 | 32.8 | **2.0** | **2.2** | −29.5 |
| | caddy | 207.4 | 211.8 | **159.9** | **164.3** | −47.5 |
| 6× | grafana | 4,250.3 | 4,308.1 | **2.6** | **2.8** | −4,249.7 |
| | alpine | 31.6 | 32.4 | **2.0** | **2.3** | −29.6 |
| | caddy | 207.2 | 232.4 | **1.9** | **2.2** | −205.3 |

The hit cost, at room: a box's life of about 2 ms (create about 1 ms, delete about 1 ms), against 31.6 ms
(alpine), 207 ms (caddy) and 4.25 s (grafana) staged whole.

## Why a store, under Ada's rule (fix the fault; a cache is no fix for one)

**The fault.** A guest on macOS or Windows can't run a Linux image's layers where the host keeps them. APFS
is case- and normalization-insensitive, so a layer holding `a` and `A`, or two normalizations of one name,
unpacks onto the host as one file (Shards' `docs/design/image-storage.md` §2.2, point 5). NTFS keeps no Linux
modes or owners. A host folder shared over virtio-fs keeps ownership only in a side xattr
(`com.docker.grpcfuse.ownership`) or WSL's `$LX` EAs. So the guest must hold each layer in a Linux file system
of its own, and whole-root staging copied every layer across at every create: 31.6 ms for alpine, 207 ms for
caddy and 4.25 s for grafana above, the same work again for the same bytes.

**What the store is.** It doesn't hide the copy, it does it once per layer. That is sound for three reasons:

- **Content-addressed.** A layer is named by its blob's SHA-256, the name an OCI image layout gives it
  (image-spec `image-layout.md`, "Blobs"). One digest is one set of bytes, so a hit is the layer itself,
  never a stale copy. A digest the host learned for a `docker save` tarball is tied to its file's identity
  (device, inode, size, mtime, ctime) and is hashed again if any of these changes, never trusted.
- **Verified admission.** A named layer's bytes are hashed as they cross. The relay keeps the layer only if
  they are its digest, refusing it by name otherwise. A layer is admitted only into room already free, the
  learned room of the largest stage left over. A layer that would cut into that room goes into the box, so a
  create's worst case is whole-root staging's own (2× above: alpine +0.0 ms, caddy −0.4 ms paired).
- **Adaptive eviction.** The room the store leaves rises with the largest stage the guest has seen
  (`.layers/.need`). It evicts only when a stage hits `ENOSPC`, taking the least recently used layer no box
  names. It has no reclaimer, because one measured worse (a03dec9: grafana 4,483 ms with it, 2,629 without).

**Its worth** is this record, at 079e268. With room, a box's life is about 2 ms against 31.6 ms, 207 ms and
4.25 s staged whole. Under pressure (299 pairs) it is no worse for alpine and caddy and 1.8 s shorter for
grafana, at p50 and p99 alike.

**What would falsify it** is a path with no copy at all that does no worse. The lead's diagnosis (2026-11-07)
is per-layer EROFS images built once per digest on the host, read in place over one read-only virtio-fs share
and mounted file-backed under the guest's overlay. That keeps Linux's own metadata in an image file, so APFS
and NTFS never see a Linux name, and it removes both the guest's copy and its admission and eviction. It is
measured against this store, interleaved, at 299 pairs on macOS HVF and Windows WHP, first creates included
with their image builds. If it is no worse at p50 and p99 everywhere, it replaces the store. If it loses, the
store stands for the reasons above.

## How it got here (each measured, each a commit)

- 22b26ba: the store; first record (studio-2, 7fc4aa4 → 22b26ba): no better, and alpine 2.2× slower.
- 3422737: digests from blob names, no re-hash at create (it was 0.9 s of grafana's).
- a7d7abc: no eviction on the create's path, bypass into the box, box links into the store (the lead's
  design (b)); f823255: no nice (Ada).
- 677ba0f, 0edcefd, 8232a8f: admission counts the file system's real, page-rounded room; the last resort
  on ENOSPC; reserve only the largest stage learned.
- d51a4da: the budget counts what the last resort can take (alpine was refused by the packer).
- 899f086: the verifying hash on its own thread (a full miss had cost 0.7 s more than whole-root staging).
- 3f08849: free room read 3 times for 515 entries, not 517; in-flight evictions counted and waited on.
- 2a209e4: only a stage that may admit checks digests (the check was 1 ms of alpine's, 6 of caddy's, under
  pressure).
- a03dec9: no reclaimer. Under pressure it took cached layers that would have been hits, for layers never
  admitted: grafana's life was 4,483 ms with it, 2,629 without (launchd
  hex.ns5route.g22diag.20261107T003038Z, 2×).
- 5dc783c: interleaved ABBA pairs, co-tenancy recorded, no waiting for quiet (Ada); e9df8ca: one store
  listing at a stage's opening.

Dropped runs, each for a reason stated before it was read: hex.ns5route.g22ab3.20261107T004634Z (a second Hex
job, D's guest boots, was started beside it, before the rule that co-tenancy is recorded, not avoided), and
hex.ns5route.g22final.20261107T011259Z (its own gate, since removed, stopped it after 2×).
