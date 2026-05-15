# indexmap_store

[![Crates.io](https://img.shields.io/crates/v/indexmap_store.svg)](https://crates.io/crates/indexmap_store)
[![Docs.rs](https://docs.rs/indexmap_store/badge.svg)](https://docs.rs/indexmap_store)

Mutable, persistent key-value store backed by an in-memory
[`IndexMap`](https://docs.rs/indexmap) and an append-only write-ahead log on
disk.

- O(1) hashed lookup, insertion-order iteration.
- Single log file, length-prefixed records, buffered writes.
- Crash-safe recovery: torn tail is truncated, never bricks the store.
- Automatic compaction when the log accumulates dead records.
- Generic over any `Serialize + DeserializeOwned` key and value (via `bincode`).

The store owns its file. Single-writer; no concurrent writers.

## Install

```toml
[dependencies]
indexmap_store = "0.2"
```

## Quick start

```rust
use indexmap_store::IndexMapStore;

let mut store: IndexMapStore<String, u64> = IndexMapStore::open("users.log")?;

store.insert("alice".into(), 1)?;
store.insert("bob".into(), 2)?;
store.modify(&"alice".into(), |v| *v += 100)?;
store.remove(&"bob".into())?;
store.flush()?;

assert_eq!(store.get(&"alice".into()), Some(&101));
assert_eq!(store.len(), 1);

// Reopen — state is recovered from the log.
drop(store);
let store: IndexMapStore<String, u64> = IndexMapStore::open("users.log")?;
assert_eq!(store.get(&"alice".into()), Some(&101));
# Ok::<(), std::io::Error>(())
```

## Configuration

```rust
use indexmap_store::{IndexMapStore, StoreConfig};

let cfg = StoreConfig {
    sync_on_write: true,         // fsync every mutation (durable, slow)
    min_compact_bytes: 4 << 20,  // don't compact until log > 4 MiB
    compact_ratio: 3.0,          // compact when total_records >= 3 * live
    buf_capacity: 64 * 1024,     // BufWriter capacity
};
let mut store: IndexMapStore<u32, String> =
    IndexMapStore::open_with("kv.log", cfg)?;
# Ok::<(), std::io::Error>(())
```

Defaults: `compact_ratio = 2.0`, `min_compact_bytes = 1 MiB`,
`sync_on_write = false`, `buf_capacity = 1 MiB`.

## API at a glance

| Method                             | Effect                                        |
| ---------------------------------- | --------------------------------------------- |
| `open` / `open_with`               | Open or create a store, replay log            |
| `insert(k, v)`                     | Append `Insert` record, return previous value |
| `remove(&k)`                       | Append `Remove` record (shift-remove)         |
| `modify(&k, f)`                    | Mutate value in place, append new `Insert`    |
| `get`, `contains_key`, `get_index` | Read-only lookups                             |
| `iter`, `keys`, `values`           | Insertion-ordered iteration                   |
| `flush`                            | Flush buffer + `sync_data`                    |
| `compact`                          | Rewrite log to one record per live entry      |

All mutating methods return `io::Result`. Errors come from the underlying
file or from `bincode` serialization (wrapped as `InvalidData`).

## Durability

With `sync_on_write = false` (default), records land in the kernel page cache
on each call but are not fsynced until you `flush()` or the OS flushes its
own writeback. Suitable for bulk loads where you call `flush()` at safe
checkpoints. Set `sync_on_write = true` for fsync-per-mutation.

A crash mid-record leaves a partial trailer on disk. The next `open` reads
records front-to-back, stops at the first short or undecodable trailer, and
`set_len`-truncates the file to the last valid record boundary. No special
recovery tool needed.

## Compaction

Every mutation increments `total_records`. Inserts/removes also update
`live_records`. When the log exceeds `min_compact_bytes` and
`total_records / live_records >= compact_ratio`, the store writes a fresh
log (one `Insert` per live entry) to `path.compact.tmp`, fsyncs, then
`rename(2)`s it over the original. Call `compact()` manually at any time.

## Limitations

- Single-writer; no concurrent access from multiple processes or threads.
- Whole-record serialization — large values are rewritten on every `modify`.
- No range queries; lookup is by exact key.

## License

[MIT](LICENSE)
