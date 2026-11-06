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
    assert_eq!(
        one_layer(&mut stage, 3),
        Spend {
            entries: 1,
            bytes: 3
        }
    );
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
/// it's held and linked to the same folder; and the room a stage needs raised to what the first spent, never
/// lowered by the second, which spent nothing.
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
    assert_eq!(std::fs::read_to_string(store.join(NEED)).unwrap(), "1 5\n");
    let (mut two, budget) =
        Stage::begin(dir.path(), BoxId::new("two").unwrap(), &[Some(digest)]).unwrap();
    assert_eq!(budget.present, [true]);
    assert_eq!(two.end().unwrap(), Spend::default());
    assert_eq!(link("two"), link("one"));
    assert_eq!(std::fs::read_to_string(store.join(NEED)).unwrap(), "1 5\n");
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

/// HEX-GUEST-14 (§4a; HEX-GUEST-22's 2x record, f823255: grafana's first stage, nothing learned yet, admitted
/// all of itself and left alpine 5 MiB, so alpine's create was refused where whole-root staging staged it):
/// a stage once released is cache, so the store admits it only while what stays free also holds a stage as
/// large as itself. Free 100 and nothing learned: expect 40 spent admitted (60 left holds 40), 60 not (40
/// left can't hold 60); with 70 learned, 40 not (60 left can't hold 70); with 30 learned, 20 is (80 left
/// holds both).
#[test]
fn the_store_leaves_room_for_a_stage_as_large_as_the_one_under_way() {
    let free = Free {
        entries: 100,
        bytes: 100,
        used: 0,
        capacity: 100,
    };
    let spent = |n| Spend {
        entries: n,
        bytes: n,
    };
    assert!(admits(free, spent(40), Spend::default()));
    assert!(!admits(free, spent(60), Spend::default()));
    assert!(!admits(free, spent(40), spent(70)));
    assert!(admits(free, spent(20), spent(30)));
}
