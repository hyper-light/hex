//! The guest's layer store's eviction (docs/design/shards-guest.md §4a): when the file system says it has no
//! room, the least recently used layer no box's record names goes, and the write is tried again; a layer a
//! box names never goes, and with none left to evict, `ENOSPC` is the answer.

use std::cell::Cell;

use super::*;

/// A layer folder in the store named `n` repeated (64 hex digits), last used at `secs`, holding a file.
fn layer(store: &Path, n: char, secs: i64) -> String {
    let name: String = std::iter::repeat_n(n, 64).collect();
    std::fs::create_dir(store.join(&name)).unwrap();
    std::fs::write(store.join(&name).join("f"), b"f").unwrap();
    std::fs::write(store.join(format!("{name}{SIZE}")), b"1 1\n").unwrap();
    let at = Timespec {
        tv_sec: secs,
        tv_nsec: 0,
    };
    rustix::fs::utimensat(
        rustix::fs::CWD,
        store.join(&name).as_path(),
        &Timestamps {
            last_access: at,
            last_modification: at,
        },
        AtFlags::empty(),
    )
    .unwrap();
    name
}

/// HEX-GUEST-14 (§4a's store, the lead's eviction rule: least recently used, no number picked): a store of
/// layers A (used first), B (later) and C (named by box `kept`'s record); a write the file system refuses
/// with ENOSPC once, then twice, then always: expect A evicted and the write done, then B, and then the
/// write refused ENOSPC with C still held.
#[test]
fn no_room_evicts_the_least_recently_used_unreferenced_layer_and_tries_again() {
    let dir = tempfile::tempdir().unwrap();
    let staged = dir.path();
    let store = staged.join(STORE);
    std::fs::create_dir(&store).unwrap();
    let a = layer(&store, 'a', 1_000);
    let b = layer(&store, 'b', 2_000);
    let c = layer(&store, 'c', 500);
    std::fs::create_dir(staged.join("kept")).unwrap();
    std::fs::write(staged.join("kept").join(RECORD), format!("{c}\n")).unwrap();
    let (stage, _) = Stage::begin(staged, BoxId::new("t").unwrap(), &[]).unwrap();
    let refusals = Cell::new(1);
    let write = || {
        if refusals.get() == 0 {
            return Ok(());
        }
        refusals.set(refusals.get() - 1);
        Err(Errno::NOSPC)
    };
    assert_eq!(stage.room(write), Ok(()));
    assert!(
        !store.join(&a).exists(),
        "the least recently used went first"
    );
    assert!(!store.join(format!("{a}{SIZE}")).exists());
    assert!(store.join(&b).exists() && store.join(&c).exists());
    assert_eq!(
        stage.room(|| -> Result<(), Errno> { Err(Errno::NOSPC) }),
        Err(Errno::NOSPC)
    );
    assert!(!store.join(&b).exists(), "the next least recently used");
    assert!(
        store.join(&c).exists(),
        "a layer a box names is never evicted"
    );
}
