//! The guest's layer store (docs/design/shards-guest.md §4a, the lead's rule of 2026-11-06): a create never
//! evicts. A layer is admitted to the store only into room already free, leaving free what the largest stage
//! yet has needed; a stage past that stages the rest of its layers into the box, as whole-root staging did,
//! and once it's answered the reclaimer frees what it bypassed by, taking the least recently used layers no
//! box names, never one a box names.

use std::path::PathBuf;

use super::*;

/// A layer folder in the store named `n` repeated (64 hex digits), last used at `secs`, holding a file, its
/// size file saying 1 entry and `bytes`.
fn layer(store: &Path, n: char, secs: i64, bytes: u64) -> String {
    let name: String = std::iter::repeat_n(n, 64).collect();
    std::fs::create_dir(store.join(&name)).unwrap();
    std::fs::write(store.join(&name).join("f"), b"f").unwrap();
    std::fs::write(store.join(format!("{name}{SIZE}")), format!("1 {bytes}\n")).unwrap();
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

/// A staged folder under this user's cache (CLAUDE.md: labs under ~/.cache), with an empty store.
fn lab() -> tempfile::TempDir {
    let cache = PathBuf::from(std::env::var_os("HOME").unwrap()).join(".cache/hex-t7/relay");
    std::fs::create_dir_all(&cache).unwrap();
    let dir = tempfile::Builder::new()
        .prefix("store-")
        .tempdir_in(&cache)
        .unwrap();
    std::fs::create_dir(dir.path().join(STORE)).unwrap();
    dir
}

fn entry(path: &str, what: What) -> Entry {
    Entry {
        path: path.to_owned(),
        mode: 0o644,
        what,
        meta: None,
    }
}

/// Stages one layer of a file `f` of `size` bytes through `stage`, its stream's root `root`.
fn one_layer(stage: &mut Stage, size: u64) -> Spend {
    stage.entry(&entry("root", What::Layer)).unwrap();
    stage.entry(&entry("root/f", What::File { size })).unwrap();
    let bytes = vec![b'x'; usize::try_from(size).unwrap()];
    stage.data(&bytes, true).unwrap();
    stage.end().unwrap()
}

fn names_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}

/// HEX-GUEST-14 (§4a; the lead's rule: a create never evicts, a miss with no free room is staged straight
/// into the box): a store holding an unreferenced layer `a`, and the room a stage needs past what's free;
/// stage a named layer: expect it a folder of the box's own (`layers/0`, no link), nothing of it in the store,
/// `a` untouched, and what it holds returned as what the stage bypassed the store by.
#[test]
fn a_stage_without_room_stages_into_the_box_and_evicts_nothing() {
    let dir = lab();
    let store = dir.path().join(STORE);
    let a = layer(&store, 'a', 1_000, 1);
    std::fs::write(store.join(NEED), format!("0 {}\n", u64::MAX)).unwrap();
    let (mut stage, budget) =
        Stage::begin(dir.path(), BoxId::new("b").unwrap(), &[Some([0xb0; 32])]).unwrap();
    assert_eq!(budget.present, [false]);
    let block = stage.free.block;
    assert_eq!(one_layer(&mut stage, 3), charge(3, block));
    let own = dir.path().join("b").join(BOX_LAYERS).join("0");
    assert!(
        std::fs::symlink_metadata(&own).unwrap().is_dir(),
        "the box's own folder"
    );
    assert_eq!(std::fs::read(own.join("f")).unwrap(), b"xxx");
    assert_eq!(
        names_in(&store),
        [
            NEED.to_owned(),
            LOCK.to_owned(),
            a.clone(),
            format!("{a}{SIZE}")
        ]
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>()
    );
}

/// HEX-GUEST-14 (§4a, admission into room already free): the room a stage needs set 512 KiB short of what
/// the stage learned is free; stage a named layer of a 1 MiB file: expect the layer, begun in the store, moved
/// into the box when its file would cut into that room, the file whole there, and no partial or layer left in
/// the store.
#[test]
fn a_layer_that_would_cut_into_the_room_a_stage_needs_moves_into_the_box() {
    let dir = lab();
    let store = dir.path().join(STORE);
    let (mut stage, _) =
        Stage::begin(dir.path(), BoxId::new("b").unwrap(), &[Some([0xc0; 32])]).unwrap();
    stage.need = Spend {
        entries: 0,
        bytes: stage.free.bytes - (512 << 10),
    };
    assert_eq!(one_layer(&mut stage, 1 << 20).bytes, 1 << 20);
    let own = dir.path().join("b").join(BOX_LAYERS).join("0");
    assert!(std::fs::symlink_metadata(&own).unwrap().is_dir());
    assert_eq!(std::fs::metadata(own.join("f")).unwrap().len(), 1 << 20);
    assert!(
        names_in(&store)
            .iter()
            .all(|n| !n.starts_with(PARTIAL) && !is_layer_name(n)),
        "{:?}",
        names_in(&store)
    );
}

/// HEX-GUEST-14 (§4a): with room, stage a named layer, then a second box of the same layer: expect the
/// first admitted (the store holds it, the box's `layers/0` a link to it, nothing bypassed), the second told
/// it's held and linked to the same folder; and the room a stage needs, learned from what each took of the
/// file system, never lowered by the second, which took nothing.
#[test]
fn an_admitted_layer_is_linked_from_its_box_and_the_room_a_stage_needs_only_rises() {
    let dir = lab();
    let store = dir.path().join(STORE);
    let digest = [0xd0; 32];
    let (mut one, _) =
        Stage::begin(dir.path(), BoxId::new("one").unwrap(), &[Some(digest)]).unwrap();
    assert_eq!(one_layer(&mut one, 5), Spend::default());
    let hex = files::digest_hex(&digest);
    assert!(store.join(&hex).join("f").exists());
    let link =
        |id: &str| std::fs::read_link(dir.path().join(id).join(BOX_LAYERS).join("0")).unwrap();
    assert_eq!(link("one"), Path::new("../../.layers").join(&hex));
    let need = read_spend(&std::fs::File::open(&store).unwrap().into(), NEED);
    let (mut two, budget) =
        Stage::begin(dir.path(), BoxId::new("two").unwrap(), &[Some(digest)]).unwrap();
    assert_eq!(budget.present, [true]);
    assert_eq!(two.end().unwrap(), Spend::default());
    assert_eq!(link("two"), link("one"));
    let after = read_spend(&std::fs::File::open(&store).unwrap().into(), NEED);
    assert!(
        after.entries >= need.entries && after.bytes >= need.bytes,
        "{after:?} < {need:?}"
    );
}

/// HEX-GUEST-14 (§4a, the reclaimer): a store of layers A (used first, 10 bytes), B (later, 10 bytes) and C
/// (oldest, named by box `kept`'s record); reclaim 5 bytes, then 100: expect A taken first and alone (its
/// 10 bytes cover 5), then B, never C, and no `.gone-` folder left; what each pass freed returned.
#[test]
fn the_reclaimer_takes_the_least_recently_used_unreferenced_layers_until_what_was_bypassed_is_freed()
 {
    let dir = lab();
    let store = dir.path().join(STORE);
    let a = layer(&store, 'a', 1_000, 10);
    let b = layer(&store, 'b', 2_000, 10);
    let c = layer(&store, 'c', 500, 10);
    std::fs::create_dir(dir.path().join("kept")).unwrap();
    std::fs::write(dir.path().join("kept").join(RECORD), format!("{c}\n")).unwrap();
    let freed = reclaim(
        dir.path(),
        Spend {
            entries: 0,
            bytes: 5,
        },
    )
    .unwrap();
    assert_eq!(
        freed,
        Spend {
            entries: 1,
            bytes: 10
        }
    );
    assert!(!store.join(&a).exists() && !store.join(format!("{a}{SIZE}")).exists());
    assert!(store.join(&b).exists() && store.join(&c).exists());
    let freed = reclaim(
        dir.path(),
        Spend {
            entries: 0,
            bytes: 100,
        },
    )
    .unwrap();
    assert_eq!(
        freed,
        Spend {
            entries: 1,
            bytes: 10
        }
    );
    assert!(!store.join(&b).exists(), "the next least recently used");
    assert!(
        store.join(&c).exists(),
        "a layer a box names is never taken"
    );
    assert!(names_in(&store).iter().all(|n| !n.starts_with(GONE)));
}

/// A file system's free room as `learned` reads it: `n` entries and bytes free, in blocks of `block` bytes.
fn room(n: u64, block: u64) -> Free {
    Free {
        entries: n,
        bytes: n,
        used: 0,
        capacity: n,
        block,
    }
}

/// HEX-GUEST-14 (§4a; HEX-GUEST-22's records): the store admits an entry only while what its file system has
/// free now, less the entry, still holds the largest stage learned (`need`, the most a stage has taken of
/// the file system, so a tmpfs's whole pages count: at 899f086 counting bytes undercounted grafana and its
/// create hit ENOSPC). It leaves nothing more: at 0edcefd it also left room for a stage as large as the one
/// under way, which held the cache to half the room, so with room for every image grafana never cached whole
/// (4x: 1.8-2.5 s a round against 2 ms); a guest's first stage, nothing learned, is admitted whole, and a
/// next create it crowds is the last resort's (`room`). Free now 100:
/// - nothing learned: an entry of 60 admitted (40 left);
/// - 70 learned: an entry of 40 not (60 can't hold 70), of 20 yes (80 holds 70);
/// - 50 free now, 49 learned: an entry of 2 not (48 can't hold 49).
#[test]
fn the_store_leaves_the_room_the_largest_stage_learned_needs() {
    let entry = |n| Spend {
        entries: n,
        bytes: n,
    };
    assert!(admits(room(100, 1), entry(60), Spend::default()));
    assert!(!admits(room(100, 1), entry(40), entry(70)));
    assert!(admits(room(100, 1), entry(20), entry(70)));
    assert!(!admits(room(50, 1), entry(2), entry(49)));
}

/// HEX-GUEST-14 (§4a): an entry's room is its bytes in whole blocks of its file system (a tmpfs page, a block),
/// at least one block for a file holding any: expect 1 byte one 4096-byte block, 4097 two, 0 none.
#[test]
fn an_entry_takes_its_bytes_in_whole_blocks() {
    assert_eq!(
        charge(1, 4096),
        Spend {
            entries: 1,
            bytes: 4096
        }
    );
    assert_eq!(
        charge(4097, 4096),
        Spend {
            entries: 1,
            bytes: 8192
        }
    );
    assert_eq!(
        charge(0, 4096),
        Spend {
            entries: 1,
            bytes: 0
        }
    );
}

/// HEX-GUEST-14 (§4a; the lead's rule, 2026-11-06: a create never fails where whole-root staging would have
/// staged it): a store whose cache holds the room, layers A (used first) and B (later) unreferenced and C
/// named by box `kept`'s record, as when a new image larger than any seen meets a cache the room it needs
/// hasn't learned yet; a stage's write the file system refuses ENOSPC once, then always: expect A evicted
/// on the stage's path and the write done, then B evicted, and then ENOSPC refused with C still held.
#[test]
fn a_stage_out_of_room_evicts_unreferenced_layers_as_a_last_resort() {
    let dir = lab();
    let store = dir.path().join(STORE);
    let a = layer(&store, 'a', 1_000, 1);
    let b = layer(&store, 'b', 2_000, 1);
    let c = layer(&store, 'c', 500, 1);
    std::fs::create_dir(dir.path().join("kept")).unwrap();
    std::fs::write(dir.path().join("kept").join(RECORD), format!("{c}\n")).unwrap();
    let (stage, _) = Stage::begin(dir.path(), BoxId::new("t").unwrap(), &[]).unwrap();
    let refusals = std::cell::Cell::new(1);
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

/// HEX-GUEST-14 (§4a; HEX-GUEST-22's 2x run at 8232a8f: the budget said 5 MiB, what the file system had free,
/// and the packer refused alpine before the relay could make room, though the cache held hundreds of MiB
/// its last resort could take): a store holding an unreferenced layer of 7 entries and 4096 bytes and a
/// referenced one: expect a stage's budget the file system's free room plus the unreferenced layer's, never
/// the referenced one's.
#[test]
fn a_stages_budget_counts_what_the_last_resort_can_take() {
    let dir = lab();
    let store = dir.path().join(STORE);
    let spare = layer(&store, 'a', 1_000, 4096);
    std::fs::write(store.join(format!("{spare}{SIZE}")), "7 4096\n").unwrap();
    let kept = layer(&store, 'c', 500, 1 << 30);
    std::fs::create_dir(dir.path().join("kept")).unwrap();
    std::fs::write(dir.path().join("kept").join(RECORD), format!("{kept}\n")).unwrap();
    let (stage, budget) = Stage::begin(dir.path(), BoxId::new("t").unwrap(), &[]).unwrap();
    let free = stage.free;
    assert!(
        budget.entries >= free.entries + 7 && budget.entries < free.entries + 7 + (1 << 10),
        "{} of {} free",
        budget.entries,
        free.entries
    );
    assert!(
        budget.bytes >= free.bytes + 4096 && budget.bytes < free.bytes + (1 << 30),
        "{} of {} free",
        budget.bytes,
        free.bytes
    );
}

/// Stages one layer of `n` one-byte files through `stage`, its stream's root `root`.
fn many_files(stage: &mut Stage, n: usize) -> Spend {
    stage.entry(&entry("root", What::Layer)).unwrap();
    for i in 0..n {
        stage
            .entry(&entry(&format!("root/f{i}"), What::File { size: 1 }))
            .unwrap();
        stage.data(b"x", true).unwrap();
    }
    stage.end().unwrap()
}

/// HEX-GUEST-14 (§4a; the lead's rule after HEX-GUEST-22's 4x run at d51a4da, alpine +1.1 ms: the bypass and
/// admission paths cost no more than whole-root staging): count the file system reads (fstatvfs) and store
/// lock takes a stage makes. A layer of 515 one-byte files (alpine's entries) admitted with room to spare:
/// expect at most 3 reads (d51a4da read once an entry, 516), and the lock taken at the stage's opening and at
/// most once more, to raise the room learned; a second stage of the same 515 files that takes no more: the lock
/// at its opening alone. A layer the store has no room for, 515 files into the box: one read, to find there's
/// no room, and none an entry.
#[test]
fn free_room_is_read_only_as_the_margin_nears_and_the_lock_only_to_learn() {
    let dir = lab();
    let (mut one, _) =
        Stage::begin(dir.path(), BoxId::new("one").unwrap(), &[Some([0xe0; 32])]).unwrap();
    many_files(&mut one, 515);
    assert!(one.reads <= 3, "{} reads for 515 entries", one.reads);
    assert!(
        one.locks <= 2,
        "the opening, and to raise what's learned: {}",
        one.locks
    );
    let (mut two, _) =
        Stage::begin(dir.path(), BoxId::new("two").unwrap(), &[Some([0xe1; 32])]).unwrap();
    two.need = Spend {
        entries: 0,
        bytes: 1,
    };
    many_files(&mut two, 515);
    assert!(two.reads <= 3, "{} reads", two.reads);
    let (mut three, _) = Stage::begin(
        dir.path(),
        BoxId::new("three").unwrap(),
        &[Some([0xe2; 32])],
    )
    .unwrap();
    three.need = Spend {
        entries: u64::MAX,
        bytes: u64::MAX,
    };
    many_files(&mut three, 515);
    assert_eq!(
        three.reads, 2,
        "one to find no room, one to learn what it took"
    );
    assert_eq!(
        three.locks, 1,
        "the opening alone: it took less than the room learned"
    );
}

/// HEX-GUEST-14 (§4a; HEX-GUEST-22's 2x run at d51a4da: grafana refused, its budget 1.1 GB, while the
/// reclaimer removed a layer it had taken, room neither free nor counted): a layer the reclaimer took and is
/// still removing (`.gone-<pid>-<n>`, its size file beside it, 3 entries and 8192 bytes): expect a stage's
/// budget to count it; and a write the file system refuses ENOSPC while it's there, with no layer left to
/// evict: expect the stage to finish that removal (no one holds its lock, as when its remover ended) and the
/// write done, the folder and its size file gone.
#[test]
fn an_in_flight_reclaim_counts_in_the_budget_and_a_stage_out_of_room_finishes_it() {
    let dir = lab();
    let store = dir.path().join(STORE);
    let gone = format!("{GONE}{}-{}", std::process::id(), u64::MAX);
    std::fs::create_dir(store.join(&gone)).unwrap();
    std::fs::write(store.join(&gone).join("f"), b"f").unwrap();
    std::fs::write(store.join(format!("{gone}{SIZE}")), "3 8192\n").unwrap();
    let (stage, budget) = Stage::begin(dir.path(), BoxId::new("t").unwrap(), &[]).unwrap();
    assert!(budget.entries >= stage.free.entries + 3, "{budget:?}");
    assert!(budget.bytes >= stage.free.bytes + 8192, "{budget:?}");
    let write = || {
        if store.join(&gone).exists() {
            Err(Errno::NOSPC)
        } else {
            Ok(())
        }
    };
    assert_eq!(stage.room(write), Ok(()));
    assert!(!store.join(&gone).exists() && !store.join(format!("{gone}{SIZE}")).exists());
}
