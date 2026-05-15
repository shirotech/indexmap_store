use indexmap_store::{IndexMapStore, StoreConfig};
use std::fs::OpenOptions;
use std::io::Write;

fn store_path(dir: &tempfile::TempDir) -> std::path::PathBuf {
    dir.path().join("store.log")
}

#[test]
fn basic_insert_get() {
    let dir = tempfile::tempdir().unwrap();
    let mut s: IndexMapStore<String, u64> = IndexMapStore::open(store_path(&dir)).unwrap();
    s.insert("a".into(), 1).unwrap();
    s.insert("b".into(), 2).unwrap();
    s.insert("c".into(), 3).unwrap();
    assert_eq!(s.get(&"a".to_string()), Some(&1));
    assert_eq!(s.get(&"b".to_string()), Some(&2));
    assert_eq!(s.len(), 3);
    let order: Vec<_> = s.keys().cloned().collect();
    assert_eq!(order, vec!["a", "b", "c"]);
}

#[test]
fn modify_edits_in_place() {
    let dir = tempfile::tempdir().unwrap();
    let mut s: IndexMapStore<String, Vec<i32>> = IndexMapStore::open(store_path(&dir)).unwrap();
    s.insert("xs".into(), vec![1, 2, 3]).unwrap();
    let pushed = s
        .modify(&"xs".to_string(), |v| {
            v.push(4);
            v.len()
        })
        .unwrap();
    assert_eq!(pushed, Some(4));
    assert_eq!(s.get(&"xs".to_string()), Some(&vec![1, 2, 3, 4]));
}

#[test]
fn persistence_across_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = store_path(&dir);
    {
        let mut s: IndexMapStore<String, u64> = IndexMapStore::open(&path).unwrap();
        s.insert("a".into(), 1).unwrap();
        s.insert("b".into(), 2).unwrap();
        s.insert("c".into(), 3).unwrap();
        s.modify(&"b".to_string(), |v| *v = 22).unwrap();
        s.remove(&"a".to_string()).unwrap();
        s.flush().unwrap();
    }
    let s: IndexMapStore<String, u64> = IndexMapStore::open(&path).unwrap();
    assert_eq!(s.len(), 2);
    assert_eq!(s.get(&"a".to_string()), None);
    assert_eq!(s.get(&"b".to_string()), Some(&22));
    assert_eq!(s.get(&"c".to_string()), Some(&3));
    let order: Vec<_> = s.keys().cloned().collect();
    assert_eq!(order, vec!["b", "c"]);
}

#[test]
fn compaction_shrinks_log() {
    let dir = tempfile::tempdir().unwrap();
    let path = store_path(&dir);
    let cfg = StoreConfig {
        compact_ratio: 2.0,
        min_compact_bytes: 0,
        sync_on_write: false,
        buf_capacity: 4096,
    };
    let mut s: IndexMapStore<u32, u64> = IndexMapStore::open_with(&path, cfg.clone()).unwrap();
    for _ in 0..1000 {
        s.insert(1, 1).unwrap();
    }
    s.flush().unwrap();
    let size_after = std::fs::metadata(&path).unwrap().len();
    assert!(
        size_after < 200,
        "expected compaction to shrink log, got {}",
        size_after
    );

    let s2: IndexMapStore<u32, u64> = IndexMapStore::open_with(&path, cfg).unwrap();
    assert_eq!(s2.len(), 1);
    assert_eq!(s2.get(&1), Some(&1));
}

#[test]
fn recovers_from_torn_tail() {
    let dir = tempfile::tempdir().unwrap();
    let path = store_path(&dir);
    {
        let mut s: IndexMapStore<String, u64> = IndexMapStore::open(&path).unwrap();
        s.insert("a".into(), 1).unwrap();
        s.insert("b".into(), 2).unwrap();
        s.flush().unwrap();
    }
    {
        let mut f = OpenOptions::new().append(true).open(&path).unwrap();
        f.write_all(&[0xff, 0xff, 0xff, 0xff, 0xde, 0xad]).unwrap();
        f.sync_all().unwrap();
    }
    let s: IndexMapStore<String, u64> = IndexMapStore::open(&path).unwrap();
    assert_eq!(s.len(), 2);
    assert_eq!(s.get(&"a".to_string()), Some(&1));
    assert_eq!(s.get(&"b".to_string()), Some(&2));
}

#[test]
fn recovers_from_truncated_payload() {
    let dir = tempfile::tempdir().unwrap();
    let path = store_path(&dir);
    let original_size;
    {
        let mut s: IndexMapStore<String, u64> = IndexMapStore::open(&path).unwrap();
        s.insert("a".into(), 1).unwrap();
        s.insert("b".into(), 2).unwrap();
        s.flush().unwrap();
        original_size = std::fs::metadata(&path).unwrap().len();
    }
    {
        let f = OpenOptions::new().write(true).open(&path).unwrap();
        f.set_len(original_size - 2).unwrap();
    }
    let s: IndexMapStore<String, u64> = IndexMapStore::open(&path).unwrap();
    assert_eq!(s.len(), 1);
    assert_eq!(s.get(&"a".to_string()), Some(&1));
    let restored_size = std::fs::metadata(&path).unwrap().len();
    assert!(restored_size < original_size - 2);
}

#[test]
fn remove_preserves_remaining_order() {
    let dir = tempfile::tempdir().unwrap();
    let mut s: IndexMapStore<u32, u32> = IndexMapStore::open(store_path(&dir)).unwrap();
    for i in 0..5 {
        s.insert(i, i * 10).unwrap();
    }
    s.remove(&2).unwrap();
    let order: Vec<_> = s.keys().copied().collect();
    assert_eq!(order, vec![0, 1, 3, 4]);
}

#[test]
fn update_keeps_position() {
    let dir = tempfile::tempdir().unwrap();
    let mut s: IndexMapStore<u32, u32> = IndexMapStore::open(store_path(&dir)).unwrap();
    s.insert(10, 100).unwrap();
    s.insert(20, 200).unwrap();
    s.insert(30, 300).unwrap();
    s.insert(20, 999).unwrap();
    let order: Vec<_> = s.iter().map(|(k, v)| (*k, *v)).collect();
    assert_eq!(order, vec![(10, 100), (20, 999), (30, 300)]);
}

#[test]
fn sync_on_write_durable() {
    let dir = tempfile::tempdir().unwrap();
    let path = store_path(&dir);
    let cfg = StoreConfig {
        sync_on_write: true,
        ..StoreConfig::default()
    };
    {
        let mut s: IndexMapStore<u32, u32> = IndexMapStore::open_with(&path, cfg.clone()).unwrap();
        s.insert(1, 1).unwrap();
        s.insert(2, 2).unwrap();
        // No explicit flush — sync_on_write should have already persisted.
        std::mem::forget(s);
    }
    let s: IndexMapStore<u32, u32> = IndexMapStore::open_with(&path, cfg).unwrap();
    assert_eq!(s.get(&1), Some(&1));
    assert_eq!(s.get(&2), Some(&2));
}

#[test]
fn empty_after_full_delete_compacts() {
    let dir = tempfile::tempdir().unwrap();
    let path = store_path(&dir);
    let cfg = StoreConfig {
        compact_ratio: 2.0,
        min_compact_bytes: 0,
        sync_on_write: false,
        buf_capacity: 4096,
    };
    let mut s: IndexMapStore<u32, u32> = IndexMapStore::open_with(&path, cfg.clone()).unwrap();
    for i in 0..100 {
        s.insert(i, i).unwrap();
    }
    for i in 0..100 {
        s.remove(&i).unwrap();
    }
    s.flush().unwrap();
    let size = std::fs::metadata(&path).unwrap().len();
    assert_eq!(
        size, 0,
        "fully drained store should compact to 0 bytes, got {}",
        size
    );
}

#[test]
fn many_inserts_and_iter() {
    let dir = tempfile::tempdir().unwrap();
    let mut s: IndexMapStore<u32, u32> = IndexMapStore::open(store_path(&dir)).unwrap();
    for i in 0..10_000 {
        s.insert(i, i.wrapping_mul(3)).unwrap();
    }
    s.flush().unwrap();
    assert_eq!(s.len(), 10_000);
    let sum: u64 = s.values().map(|v| *v as u64).sum();
    let expected: u64 = (0u32..10_000).map(|i| i.wrapping_mul(3) as u64).sum();
    assert_eq!(sum, expected);
}

#[test]
fn modify_missing_returns_none() {
    let dir = tempfile::tempdir().unwrap();
    let mut s: IndexMapStore<String, u64> = IndexMapStore::open(store_path(&dir)).unwrap();
    let r: Option<()> = s.modify(&"missing".to_string(), |_| {}).unwrap();
    assert!(r.is_none());
}
