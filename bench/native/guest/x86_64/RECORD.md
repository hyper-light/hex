# The x86_64 guest image (docs/design/shards-guest.md §4)

The from-source path: `scripts/guest/build.sh x86_64 SHARDS OUT [hyperv]` on fedora
(kiln-a094a12d.attlocal.net, i9-14900HX, Linux, Docker 29.8.2), in hex-slot units
`hex-ns5b-guest-20261106T053731Z` and `hex-ns5b-guest2-20261106T065022Z`, containers on CPUs 16-31 with 8 GiB
and OOM score 1000, 2026-11-06. Shards at `90de623f` (`origin/gpu`).

The `hyperv` variant is jupiter's, a Windows host whose WHP partitions keep Hyper-V's clocks
(Shards' `resources/kernel/README.md`). It is kept at `~/.cache/hex-t7/native-s5/guest/x86_64/` on jupiter
(`C:\Users\corph\.cache\hex-t7\native-s5\guest\x86_64`), copied from fedora with its SHA-256s checked at each
hop. The plain variant is KVM's, kept on fedora for the Linux boot.

## Outputs (jupiter's, `hyperv`, Hex `c5aa681`)

| Output | SHA-256 | Source |
|---|---|---|
| `kernel/vmlinux-6.18.48-x86_64-hyperv` | `d04b68b98b5ca6f1f451954a82d2fcfc275af1e8e6e3200ff9b27985b57508b3` | Shards' `scripts/build-kernel.sh x86_64 OUT hyperv` in Shards' pinned builder; byte-identical to Shards' release, whose hash `crates/shards/src/kernel.rs:24` pins |
| `kernel/config-6.18.48-x86_64-hyperv` | `52fc8480744e9e19ff7c77c611abe14f655ebec40dd275268601240e61dd709d` | the same build; has every line of `scripts/guest/kernel-needs.config`; identical to the release's |
| `hex` | `308ee8d1c905ce93d37d9a640de9f327f645b4fb26b36fbd3b570a48a45053a0` | Hex `c5aa681`, `x86_64-unknown-linux-gnu`, static glibc (`+crt-static`), release, not stripped |
| `initramfs-x86_64.cpio` | `f40353b35ec7aebf152b78fa7768261cb637e7422a102bb9913daee64e9f9709` | `scripts/guest/initramfs.py` over that `hex` |

The plain variant's kernel is `e23c31e89580f2fb8524cb0f560cbd356c22f2de6ae82081352d809151434261`, also
byte-identical to Shards' release (`kernel.rs:31`), with config `d14e366600fa…`. Its last complete image is
Hex `7b7f0f8`'s: `hex` `af4f1db5433248c9c5d2c44094013a108b51de9cc7d2ae34923d58ca9365fb8d`, initramfs
`235c42c47a9183ca554e09e27c1964b52d83d8233a77fa022ab7e6eb4af6a5ba`. Its rebuild at `c5aa681` ended 141 before
the initramfs (build.sh's `ldd | grep -q` under pipefail, fixed after).

Toolchains: the kernel by gcc 12.2.0-14+deb12u1 and GNU ld 2.40 (Debian bookworm at snapshot
`20260915T000000Z`, Shards' `resources/kernel/builder.env`); `hex` by rustc 1.99.1 (`58cf8f9c3`) in
`rust@sha256:7085845219c18e7c5b1f34ed9ce89b84f822f2e3d54a02774cedc996ac0f304f`, glibc 2.36-9+deb12u14.

The initramfs holds `/dev/`, `/dev/console` (5:1) and `/init` (the `hex` above), nothing from a host's
userland.

## How a test is given it

On jupiter:

```
HEX_GUEST_KERNEL=~/.cache/hex-t7/native-s5/guest/x86_64/kernel/vmlinux-6.18.48-x86_64-hyperv
HEX_GUEST_INITRD=~/.cache/hex-t7/native-s5/guest/x86_64/initramfs-x86_64.cpio
SHARDS_BIN=<a Windows shards build>   (none is on jupiter yet)
```

On fedora (KVM): the same names under `~/.cache/hex-t7/native-s5/b/guest/x86_64-hyperv/` or `…/x86_64/`, with
`SHARDS_BIN=~/.cache/hex-t7/native-s5/b/target-shards/release/shards` (Shards `90de623f`, release, SHA-256
`1a28fe53e5e02065c41b2b629c935e36a6296de58cccfc8c5f153b8771c517ce`). The command line is
`console=ttyS0 panic=-1`, with `hex.shares=…` where the guest has shares (HEX-GUEST-11).

## Boots

`scripts/guest/boot_check.py` on fedora under KVM, 2 vCPUs, 1,024 MiB, Shards above, 2026-11-06 06:50-07:20Z,
load 5-20 beside other agents' units (noisy, so no time here is a timing of record). READY is a `CONNECT` to
the relay's control port answered `OK`, then the connection's token.

| Kernel, image | `--no-console` boots | with a console |
|---|---|---|
| `hyperv`, Hex `c5aa681` (jupiter's image) | READY after 0.285, 0.302, 0.414 s | READY after 0.496 s |
| plain, Hex `7b7f0f8` | READY after 0.280, 0.242, 0.241 s | READY after 0.505 s |
| plain, Hex `7b7f0f8`, unit 1 | READY after 0.268 s | |

Every READY's token was 0, the relay's first control connection. Nothing has booted on jupiter itself yet:
it has no Shards build. No box has run in a guest; that is the conformance suite's (§6). No share was given
on fedora (they were proved on the Mac: `../aarch64/RECORD.md`).
