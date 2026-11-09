# HEX-GUEST-21: a staged root against containerd's own unpack

Each image's own layers, staged as a tarball root (docs/design/shards-guest.md §4a), must hold exactly what
containerd's unpack of the same layers holds: kind, mode, owner, mtime, link text, hard-link group, xattrs and
sha256 of every entry. The stated differences, and no others:

- atime;
- the root directory itself;
- the mtime of directories mkparent made or made a child in (both sides stamp them when the unpack runs);
- security.selinux labels the host assigned, which the image's layers don't carry.

## The reference (fedora, as root)

`reference.py HEX OUT IMAGE...` drives the host's containerd from a privileged fedora:44 container (by digest)
sharing `/run/containerd` and `/var/lib/containerd`, in a containerd namespace of its own (`hexg21`):

1. the image is pulled by digest for linux/amd64 (`ctr images pull --platform linux/amd64`), or, for the lab
   images, written by `lab_image.py` and imported (`ctr images import --platform linux/amd64`, which unpacks);
2. its layer blobs are saved byte-exact from the content store, in the manifest's order
   (`ctr content get <digest>` → `layer<i>.tar.gz`, listed in `layers.txt`);
3. its snapshot is viewed read-only at its chain id (`ctr snapshots view`) and mounted as `ctr snapshots
   mounts` prints it, except that an overlay's one `lowerdir=a:b:…` option becomes one `lowerdir+=` per layer
   in the same order (the new mount API, fsconfig(2), Linux 6.8+; ctr's one option passes the page mount(2)
   takes once an image has many layers: caddy's failed, exit 32). On lab:tree both forms are mounted and their
   walks are byte-identical (RECORD);
4. the mount is walked as root by `hex selftest-box tree`, then unmounted and the view removed, all in one
   shell, since a view's lease ends with the ctr process that made it.

`RECORD.txt` holds ctr's and containerd's versions, the snapshotter, the kernel, each image's digests, ctr's
printed mount and the one used. ctr 2.3.6's `images mount` gave an empty tmpfs on fedora, so it isn't used.

The lab images (`lab_image.py`, Python's tarfile, PAX, gzip mtime 0) hold what no build tool emits on demand:
- `lab:tree`: whiteout, opaque directory, replacements of every kind, a file under an earlier layer's link
  (`bin/tool`), parents no layer made (`usr/local/bin/x`), files through this layer's own links;
- `lab:file`: a file under an earlier layer's file;
- `lab:hardlink`: a hard link to an earlier layer's file, whose unpack ctr fails (RECORD).

## Hex's side

`cargo test -p hex-cli --test guest_tree` (crates/hex-cli/tests/guest_tree.rs) stages each image's layers and
walks the staged root with the same walker: in a real guest (the Mac, jupiter), or on Linux as root with
`HEX_GUEST21_LOCAL=1` (the relay in-process; the check before a guest run). Its header says what it takes.
Run in a container, its `/tmp` (where it stages) is a tmpfs, as the guest's staged folder is: a container's
own overlayfs root refuses overlayfs's 0/0 whiteout device ("Operation not permitted", fedora 6.19).
