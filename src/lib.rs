//! Mutable, persistent key-value store backed by an in-memory [`IndexMap`] and an
//! append-only write-ahead log on disk.
//!
//! Design:
//!
//! * Reads, lookups, and iteration go through the in-memory map — O(1) hashed
//!   access while preserving insertion order.
//! * Every mutation appends one length-prefixed record to a single log file.
//!   Records are written through a [`BufWriter`] so bulk writes coalesce into
//!   page-sized syscalls. With `sync_on_write = false` (the default) the OS
//!   page cache absorbs writes; turn it on for fsync-per-op durability.
//! * Recovery reads the log front-to-back, replays records into the map, and
//!   truncates a torn tail so a crash mid-record never bricks the store.
//! * When the log grows past a configurable threshold and accumulates enough
//!   dead records (updates/deletes that supersede earlier entries), the store
//!   rewrites a minimal snapshot to a temp file and atomically renames it
//!   into place.
//!
//! The store owns the file; concurrent writers are not supported.
//!
//! # Quick start
//!
//! ```
//! use indexmap_store::IndexMapStore;
//!
//! let dir = tempfile::tempdir().unwrap();
//! let path = dir.path().join("users.log");
//!
//! let mut store: IndexMapStore<String, u64> = IndexMapStore::open(&path)?;
//! store.insert("alice".into(), 1)?;
//! store.insert("bob".into(), 2)?;
//! store.modify(&"alice".into(), |v| *v += 100)?;
//! store.remove(&"bob".into())?;
//! store.flush()?;
//!
//! assert_eq!(store.get(&"alice".into()), Some(&101));
//! assert_eq!(store.len(), 1);
//!
//! // Reopen: state is recovered from the log.
//! drop(store);
//! let store: IndexMapStore<String, u64> = IndexMapStore::open(&path)?;
//! assert_eq!(store.get(&"alice".into()), Some(&101));
//! # Ok::<(), std::io::Error>(())
//! ```
//!
//! # Custom configuration
//!
//! ```
//! use indexmap_store::{IndexMapStore, StoreConfig};
//!
//! let dir = tempfile::tempdir().unwrap();
//! let cfg = StoreConfig {
//!     sync_on_write: true,        // fsync every mutation
//!     min_compact_bytes: 4 << 20, // wait until log > 4 MiB before compacting
//!     compact_ratio: 3.0,         // compact when total_records >= 3 * live
//!     buf_capacity: 64 * 1024,
//! };
//! let mut store: IndexMapStore<u32, String> =
//!     IndexMapStore::open_with(dir.path().join("kv.log"), cfg)?;
//! store.insert(1, "one".into())?;
//! # Ok::<(), std::io::Error>(())
//! ```
//!
//! # Iteration preserves insertion order
//!
//! ```
//! use indexmap_store::IndexMapStore;
//!
//! let dir = tempfile::tempdir().unwrap();
//! let mut store: IndexMapStore<String, i32> =
//!     IndexMapStore::open(dir.path().join("ord.log"))?;
//! for (k, v) in [("c", 3), ("a", 1), ("b", 2)] {
//!     store.insert(k.into(), v)?;
//! }
//! let keys: Vec<_> = store.keys().cloned().collect();
//! assert_eq!(keys, ["c", "a", "b"]);
//! # Ok::<(), std::io::Error>(())
//! ```

use indexmap::IndexMap;
use serde::{Serialize, de::DeserializeOwned};
use std::fs::{self, File, OpenOptions};
use std::hash::Hash;
use std::io::{self, BufWriter, ErrorKind, Read, Write};
use std::path::{Path, PathBuf};

const LEN_BYTES: usize = 4;
const TAG_INSERT: u8 = 0;
const TAG_REMOVE: u8 = 1;

/// Configuration for an [`IndexMapStore`].
#[derive(Clone, Debug)]
pub struct StoreConfig {
    /// Compaction fires when `total_records > live_records * compact_ratio`
    /// AND the log is larger than `min_compact_bytes`.
    pub compact_ratio: f64,
    /// Minimum log size before compaction is even considered.
    pub min_compact_bytes: u64,
    /// If true, every mutation flushes the buffer and fsyncs the file. Slow
    /// but durable. Leave off for bulk loads and call [`IndexMapStore::flush`]
    /// at safe points instead.
    pub sync_on_write: bool,
    /// Capacity of the internal [`BufWriter`].
    pub buf_capacity: usize,
}

impl Default for StoreConfig {
    fn default() -> Self {
        Self {
            compact_ratio: 2.0,
            min_compact_bytes: 1 << 20,
            sync_on_write: false,
            buf_capacity: 1024 * 1024,
        }
    }
}

/// A mutable, persistent [`IndexMap`].
pub struct IndexMapStore<K, V> {
    map: IndexMap<K, V, ahash::RandomState>,
    log: BufWriter<File>,
    path: PathBuf,
    log_bytes: u64,
    live_records: u64,
    total_records: u64,
    cfg: StoreConfig,
    scratch: Vec<u8>,
}

impl<K, V> IndexMapStore<K, V>
where
    K: Eq + Hash + Serialize + DeserializeOwned,
    V: Serialize + DeserializeOwned,
{
    /// Open (or create) a store at `path` with default configuration.
    pub fn open<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        Self::open_with(path, StoreConfig::default())
    }

    /// Open (or create) a store at `path` with the given configuration.
    pub fn open_with<P: AsRef<Path>>(path: P, cfg: StoreConfig) -> io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent)?;
        }

        let mut map: IndexMap<K, V, ahash::RandomState> =
            IndexMap::with_hasher(ahash::RandomState::new());
        let mut valid_len: u64 = 0;
        let mut total_records: u64 = 0;

        // Single open: read+append+create. O_APPEND only affects writes, so
        // the initial read_to_end at offset 0 still works. set_len for the
        // torn-tail case also works on this handle (ftruncate requires write
        // access; O_APPEND satisfies that).
        let mut file = OpenOptions::new()
            .read(true)
            .append(true)
            .create(true)
            .open(&path)?;
        let total_on_disk = file.metadata()?.len();

        if total_on_disk > 0 {
            let capacity_hint = (total_on_disk / 24) as usize;
            if capacity_hint > 0 {
                map.reserve(capacity_hint);
            }
            let mut buf: Vec<u8> = Vec::with_capacity(total_on_disk as usize);
            file.read_to_end(&mut buf)?;

            let mut offset: usize = 0;
            while offset + LEN_BYTES <= buf.len() {
                let len = u32::from_le_bytes(buf[offset..offset + LEN_BYTES].try_into().unwrap())
                    as usize;
                let payload_start = offset + LEN_BYTES;
                let payload_end = payload_start + len;
                if payload_end > buf.len() || len == 0 {
                    break;
                }
                let tag = buf[payload_start];
                let body = &buf[payload_start + 1..payload_end];
                match tag {
                    TAG_INSERT => {
                        let (k, v): (K, V) = match bincode::deserialize(body) {
                            Ok(r) => r,
                            Err(_) => break,
                        };
                        map.insert(k, v);
                    }
                    TAG_REMOVE => {
                        let k: K = match bincode::deserialize(body) {
                            Ok(r) => r,
                            Err(_) => break,
                        };
                        map.shift_remove(&k);
                    }
                    _ => break,
                }
                valid_len += (LEN_BYTES + len) as u64;
                total_records += 1;
                offset = payload_end;
            }

            if valid_len != total_on_disk {
                file.set_len(valid_len)?;
                file.sync_all()?;
            }
        }

        let live_records = map.len() as u64;

        Ok(Self {
            map,
            log: BufWriter::with_capacity(cfg.buf_capacity, file),
            path,
            log_bytes: valid_len,
            live_records,
            total_records,
            cfg,
            scratch: Vec::with_capacity(256),
        })
    }

    /// Number of live entries.
    #[inline]
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// True if there are no live entries.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// True if `k` is present.
    #[inline]
    pub fn contains_key(&self, k: &K) -> bool {
        self.map.contains_key(k)
    }

    /// Look up a value by key.
    #[inline]
    pub fn get(&self, k: &K) -> Option<&V> {
        self.map.get(k)
    }

    /// Look up a (key, value) pair by insertion index.
    #[inline]
    pub fn get_index(&self, idx: usize) -> Option<(&K, &V)> {
        self.map.get_index(idx)
    }

    /// Iterate entries in insertion order.
    #[inline]
    pub fn iter(&self) -> indexmap::map::Iter<'_, K, V> {
        self.map.iter()
    }

    /// Iterate keys in insertion order.
    #[inline]
    pub fn keys(&self) -> indexmap::map::Keys<'_, K, V> {
        self.map.keys()
    }

    /// Iterate values in insertion order.
    #[inline]
    pub fn values(&self) -> indexmap::map::Values<'_, K, V> {
        self.map.values()
    }

    /// Borrow the underlying [`IndexMap`] read-only. All mutations must go
    /// through the store API so the log stays in sync.
    #[inline]
    pub fn as_indexmap(&self) -> &IndexMap<K, V, ahash::RandomState> {
        &self.map
    }

    /// Insert `k -> v`, returning the previous value if any. If `k` already
    /// existed the entry keeps its insertion position (standard
    /// [`IndexMap::insert`] semantics).
    #[inline]
    pub fn insert(&mut self, k: K, v: V) -> io::Result<Option<V>> {
        begin_record(&mut self.scratch, TAG_INSERT);
        bincode::serialize_into(&mut self.scratch, &(&k, &v)).map_err(serialize_err)?;
        self.flush_scratch()?;
        let prev = self.map.insert(k, v);
        if prev.is_none() {
            self.live_records += 1;
        }
        self.total_records += 1;
        self.maybe_compact()?;
        Ok(prev)
    }

    /// Remove the entry for `k` (shift-remove — preserves the order of the
    /// remaining entries). Returns the previous value if any.
    #[inline]
    pub fn remove(&mut self, k: &K) -> io::Result<Option<V>> {
        if !self.map.contains_key(k) {
            return Ok(None);
        }
        begin_record(&mut self.scratch, TAG_REMOVE);
        bincode::serialize_into(&mut self.scratch, k).map_err(serialize_err)?;
        self.flush_scratch()?;
        let prev = self.map.shift_remove(k);
        if prev.is_some() {
            self.live_records -= 1;
        }
        self.total_records += 1;
        self.maybe_compact()?;
        Ok(prev)
    }

    /// Edit the value for `k` in place via a closure. The post-edit value is
    /// appended to the log as a fresh `Insert` record, so the change survives
    /// a restart. Returns `None` if `k` is absent, else `Some(f's return)`.
    #[inline]
    pub fn modify<F, R>(&mut self, k: &K, f: F) -> io::Result<Option<R>>
    where
        F: FnOnce(&mut V) -> R,
    {
        let v_mut = match self.map.get_mut(k) {
            Some(v) => v,
            None => return Ok(None),
        };
        let result = f(v_mut);
        let v_ref: &V = v_mut;

        begin_record(&mut self.scratch, TAG_INSERT);
        bincode::serialize_into(&mut self.scratch, &(k, v_ref)).map_err(serialize_err)?;

        self.flush_scratch()?;
        self.total_records += 1;
        self.maybe_compact()?;
        Ok(Some(result))
    }

    /// Flush the internal buffer and fsync the file.
    pub fn flush(&mut self) -> io::Result<()> {
        self.log.flush()?;
        self.log.get_ref().sync_data()
    }

    /// Rewrite the log to contain exactly one `Insert` per live entry, then
    /// atomically replace the original. Safe to call at any time; runs
    /// automatically when the dead-record ratio crosses the threshold.
    pub fn compact(&mut self) -> io::Result<()> {
        self.log.flush()?;
        let tmp_path = self.path.with_extension("compact.tmp");
        {
            let tmp = OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .open(&tmp_path)?;
            let mut writer = BufWriter::with_capacity(self.cfg.buf_capacity, tmp);
            let mut buf = Vec::with_capacity(256);
            for (k, v) in &self.map {
                buf.clear();
                buf.push(TAG_INSERT);
                bincode::serialize_into(&mut buf, &(k, v)).map_err(serialize_err)?;
                writer.write_all(&(buf.len() as u32).to_le_bytes())?;
                writer.write_all(&buf)?;
            }
            writer.flush()?;
            writer.get_ref().sync_all()?;
        }
        fs::rename(&tmp_path, &self.path)?;

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        let new_len = file.metadata()?.len();
        self.log = BufWriter::with_capacity(self.cfg.buf_capacity, file);
        self.log_bytes = new_len;
        self.live_records = self.map.len() as u64;
        self.total_records = self.map.len() as u64;
        Ok(())
    }

    #[inline]
    fn flush_scratch(&mut self) -> io::Result<()> {
        // Callers reserve LEN_BYTES at the front of `scratch`; fill the length
        // in place so the length-prefix and payload land in a single write.
        let payload_len = (self.scratch.len() - LEN_BYTES) as u32;
        // SAFETY: `begin_record` reserves and initializes ≥ LEN_BYTES + 1 bytes
        // at the head of `scratch` before bincode appends the payload, so the
        // first four bytes are always within `scratch.len()` and allocated.
        unsafe {
            std::ptr::write_unaligned(self.scratch.as_mut_ptr() as *mut u32, payload_len.to_le());
        }
        self.log.write_all(&self.scratch)?;
        self.log_bytes += self.scratch.len() as u64;
        if self.cfg.sync_on_write {
            self.log.flush()?;
            self.log.get_ref().sync_data()?;
        }
        Ok(())
    }

    #[inline]
    fn maybe_compact(&mut self) -> io::Result<()> {
        if self.log_bytes < self.cfg.min_compact_bytes {
            return Ok(());
        }
        if self.live_records == 0 {
            return self.compact();
        }
        let ratio = self.total_records as f64 / self.live_records as f64;
        if ratio >= self.cfg.compact_ratio {
            self.compact()?;
        }
        Ok(())
    }
}

impl<K, V> Drop for IndexMapStore<K, V> {
    fn drop(&mut self) {
        let _ = self.log.flush();
    }
}

#[inline]
fn serialize_err(e: bincode::Error) -> io::Error {
    io::Error::new(ErrorKind::InvalidData, e)
}

#[inline]
fn begin_record(scratch: &mut Vec<u8>, tag: u8) {
    scratch.clear();
    scratch.reserve(LEN_BYTES + 1);
    // SAFETY: `reserve` guarantees capacity ≥ LEN_BYTES + 1 from offset 0.
    // We write the placeholder length (overwritten by `flush_scratch`) and the
    // tag as a single u32 + u8 store, then commit `len` to 5. The whole
    // window is fully initialized before `set_len` returns.
    unsafe {
        let p = scratch.as_mut_ptr();
        std::ptr::write_unaligned(p as *mut u32, 0u32);
        std::ptr::write(p.add(LEN_BYTES), tag);
        scratch.set_len(LEN_BYTES + 1);
    }
}
