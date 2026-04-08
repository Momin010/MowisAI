use libagent::buckets::BucketStore;
use tempfile::tempdir;

#[test]
fn bucket_store_persistence() {
    let dir = tempdir().unwrap();
    let mut store = BucketStore::new(dir.path()).unwrap();
    store.put("foo", "bar").unwrap();
    let read = store.get("foo").unwrap().unwrap();
    assert_eq!(read, "bar");
    // drop and reopen to ensure persistence
    drop(store);
    let store2 = BucketStore::new(dir.path()).unwrap();
    let read2 = store2.get("foo").unwrap().unwrap();
    assert_eq!(read2, "bar");
}
