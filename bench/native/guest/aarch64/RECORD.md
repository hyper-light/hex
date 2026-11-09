# The aarch64 guest image (docs/design/shards-guest.md §4)

The bring-up's path (§4): Shards' released kernel and a static musl `hex`, made by
`scripts/guest/image.sh` on adas-mac-studio-2.local (macOS arm64, M4 Max) on 2026-11-06, because no
arm64 Linux host could build: Docker Desktop there answers nothing, and the LAN's resolver is down. The
from-source path (`scripts/guest/build.sh`: Shards' builder, static glibc) has not run for aarch64.
Kept at `~/.cache/hex-t7/native-s5/guest/aarch64/` on that Mac.

| Output | SHA-256 | Source |
|---|---|---|
| `Image-6.18.48-aarch64` | `87fbe8c0114522cdeb80686050911db0c1b6cf62b5b8544857b373e2f53571a6` | Shards' release, https://github.com/hyper-light/shards/releases/download/kernel-6.18.48-64a9427a8997/Image-6.18.48-aarch64, its hash pinned by `crates/shards/src/kernel.rs:36-38` at `90de623f` |
| `config-6.18.48-aarch64` | `66c40d3d0508b9228af6bd7c0ffcbb8b5470d3c63784d7d862a5e02353e7b5a5` | the same release, in its `SHA256SUMS`; has every line of `scripts/guest/kernel-needs.config`, `CONFIG_VIRTIO_FS` and `CONFIG_FUSE_FS` among them |
| `hex` | `c2beac8e4357dabd9fe1c656ba2bfea046bb7be1a6bdd05e24a9d499b1899fae` | Hex `7b7f0f8`, `aarch64-unknown-linux-musl`, Rust 1.99.1, `rust-lld`, release; not stripped |
| `initramfs-aarch64.cpio` | `201faa2db0fa0f6966998b95a502f7a7ae11cfd75587397fc5a336e706b54810` | `scripts/guest/initramfs.py` over that `hex` |

The initramfs holds `/dev/`, `/dev/console` (5:1) and `/init` (the `hex` above), and nothing from a
host's userland: the oci class's runtime is that one binary, which runs nothing else (the only programs
it starts are its own re-execs and a spec's hooks).

## How a test is given it

```
HEX_GUEST_KERNEL=~/.cache/hex-t7/native-s5/guest/aarch64/Image-6.18.48-aarch64
HEX_GUEST_INITRD=~/.cache/hex-t7/native-s5/guest/aarch64/initramfs-aarch64.cpio
SHARDS_BIN=~/.cache/hex-t7/native-s5/a/target/debug/shards
```

The command line is `console=ttyS0 panic=-1`, with `hex.shares=rw,ro,…` before any `--` where the guest
has shares. It needs no `-- guest init`: `hex` as PID 1 is the guest's init (HEX-GUEST-12).

## Boots

`scripts/guest/boot_check.py`, on that Mac under HVF, 2026-11-06 05:37Z, load 3.7 (noisy, not a timing of
record), 2 vCPUs, 1,024 MiB, `SHARDS_BIN` a debug build of Shards `hex/shards-api`, SHA-256
`286f3dcd4439ac4460e5ece540e30f07cc283dde6ac332414a7c080d9418cd8d`. READY is a `CONNECT` to the relay's
control port answered `OK`, then the connection's token.

| Run | Result |
|---|---|
| `--no-console`, three boots | READY after 0.083, 0.143 and 0.057 s, token 0 |
| with a console | READY after 0.139 s, token 0, `hex guest ready` on it |
| `--share A`, `hex.shares=rw` | READY after 0.110 s, tag `shards0` found |
| `--share A --share B:ro`, `hex.shares=rw,ro` | READY after 0.184 s, tag `shards1` found |
| `hex.shares=rw` with no share device | the guest ends, exit 3: `hex guest: mounting share shards0 on /run/hex/shares/0: Invalid argument (os error 22)` |
| `hex.shares=rx` | the guest ends, exit 3, with a kernel panic on init's exit; its message was interleaved with the panic on the console |

An earlier image (`e8c0bb3e…`, Hex `429ab75`) booted with a console and ended with none, exit 3,
`Attempted to kill init! exitcode=0x00000200`: the fault HEX-GUEST-12 fixes.

READY shows the mounts succeeded and the relay reached the node's socket; no box has run in this guest
yet, and nothing here has read or written through a share. That is the conformance suite's (§6).
