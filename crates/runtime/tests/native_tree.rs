use shikifsrs_runtime::{
    Error, Result,
    memory::{AddressSpace, SliceMemory},
    native_tree::U32PointerTree,
};
use std::cell::Cell;

const BASE: u64 = 0x100000;
const HEAD: usize = 0x40;
const ROOT: usize = 0x80;
const LEFT: usize = 0xc0;
const RIGHT: usize = 0x100;

fn word(bytes: &mut [u8], at: usize, value: u64) {
    bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
}

fn node(bytes: &mut [u8], at: usize, key: u32, value: u64) {
    word(bytes, at, BASE + HEAD as u64);
    word(bytes, at + 16, BASE + HEAD as u64);
    bytes[at + 32..at + 36].copy_from_slice(&key.to_le_bytes());
    word(bytes, at + 40, value);
}

fn fixture() -> Vec<u8> {
    let mut bytes = vec![0; 0x180];
    word(&mut bytes, 8, BASE + HEAD as u64);
    word(&mut bytes, 16, 3);
    bytes[HEAD + 25] = 1;
    word(&mut bytes, HEAD + 8, BASE + ROOT as u64);
    node(&mut bytes, ROOT, 0x80000000, 42);
    node(&mut bytes, LEFT, 1, 0);
    node(&mut bytes, RIGHT, u32::MAX, 99);
    word(&mut bytes, ROOT, BASE + LEFT as u64);
    word(&mut bytes, ROOT + 16, BASE + RIGHT as u64);
    bytes
}

#[test]
fn reads_unsigned_keys_and_distinguishes_null_value_from_absent_key() {
    let bytes = fixture();
    let memory = SliceMemory::new(BASE, &bytes).unwrap();
    let tree = U32PointerTree::new(&memory, BASE, 3).unwrap();
    assert_eq!(tree.find(1).unwrap(), Some(0));
    assert_eq!(tree.find(0x80000000).unwrap(), Some(42));
    assert_eq!(tree.find(u32::MAX).unwrap(), Some(99));
    assert_eq!(tree.find(0).unwrap(), None);
    assert_eq!(tree.find(2).unwrap(), None);
    assert_eq!(tree.find(0xfffffffe).unwrap(), None);
}

#[test]
fn node_lookup_does_not_read_pointer_payload_for_other_proved_formats() {
    let bytes = fixture();
    let memory = SliceMemory::new(BASE, &bytes[..ROOT + 36]).unwrap();
    let tree = U32PointerTree::new(&memory, BASE, 3).unwrap();
    assert_eq!(
        tree.find_node(0x80000000).unwrap(),
        Some(BASE + ROOT as u64)
    );
    assert!(tree.find(0x80000000).is_err());
}

#[test]
fn empty_tree_does_not_read_nonexistent_node_values() {
    let mut bytes = fixture();
    word(&mut bytes, 16, 0);
    word(&mut bytes, HEAD + 8, BASE + HEAD as u64);
    bytes.truncate(HEAD + 26);
    let memory = SliceMemory::new(BASE, &bytes).unwrap();
    assert_eq!(
        U32PointerTree::new(&memory, BASE, 3)
            .unwrap()
            .find(1)
            .unwrap(),
        None
    );
}

#[test]
fn rejects_cycles_invalid_ordering_null_children_and_unexpected_sentinels() {
    for variant in 0..4 {
        let mut bytes = fixture();
        match variant {
            0 => word(&mut bytes, ROOT, BASE + ROOT as u64),
            1 => bytes[LEFT + 32..LEFT + 36].copy_from_slice(&u32::MAX.to_le_bytes()),
            2 => word(&mut bytes, ROOT, 0),
            3 => bytes[LEFT + 25] = 1,
            _ => unreachable!(),
        }
        let memory = SliceMemory::new(BASE, &bytes).unwrap();
        assert!(matches!(
            U32PointerTree::new(&memory, BASE, 3).unwrap().find(0),
            Err(Error::InvalidProfile(_))
        ));
    }
}

#[test]
fn refuses_bad_head_impossible_count_and_truncated_or_overflowed_addresses() {
    for variant in 0..5 {
        let mut bytes = fixture();
        match variant {
            0 => word(&mut bytes, 8, 0),
            1 => bytes[HEAD + 25] = 0,
            2 => word(&mut bytes, 16, 4),
            3 => word(&mut bytes, 16, 0),
            4 => word(&mut bytes, 16, 1),
            _ => unreachable!(),
        }
        let memory = SliceMemory::new(BASE, &bytes).unwrap();
        assert!(
            U32PointerTree::new(&memory, BASE, 3)
                .unwrap()
                .find(0)
                .is_err()
        );
    }
    let bytes = fixture();
    let memory = SliceMemory::new(BASE, &bytes[..ROOT + 44]).unwrap();
    assert!(matches!(
        U32PointerTree::new(&memory, BASE, 3)
            .unwrap()
            .find(0x80000000),
        Err(Error::OutOfBounds { .. })
    ));
    assert!(U32PointerTree::new(&memory, 0, 3).is_err());
    assert!(U32PointerTree::new(&memory, BASE, 0).is_err());
    assert_eq!(
        U32PointerTree::new(&memory, u64::MAX, 3).unwrap().find(1),
        Err(Error::AddressOverflow)
    );
}

#[test]
fn changed_root_head_or_count_refuses_a_successful_lookup() {
    struct Changing {
        bytes: Vec<u8>,
        address: u64,
        reads: Cell<usize>,
    }
    impl AddressSpace for Changing {
        fn read_exact(&self, address: u64, output: &mut [u8]) -> Result<()> {
            SliceMemory::new(BASE, &self.bytes)?.read_exact(address, output)?;
            if address == self.address {
                let reads = self.reads.get();
                self.reads.set(reads + 1);
                if reads > 0 {
                    output.fill(0);
                }
            }
            Ok(())
        }
    }
    for address in [BASE + 8, BASE + 16, BASE + HEAD as u64 + 8] {
        let memory = Changing {
            bytes: fixture(),
            address,
            reads: Cell::new(0),
        };
        assert!(matches!(
            U32PointerTree::new(&memory, BASE, 3)
                .unwrap()
                .find(0x80000000),
            Err(Error::InvalidProfile(_))
        ));
    }
}

#[test]
fn full_snapshot_preserves_duplicate_unsigned_keys_and_opaque_values() {
    use shikifsrs_runtime::native_tree::TreeKeys;
    let mut bytes = fixture();
    bytes[LEFT + 32..LEFT + 36].copy_from_slice(&0x80000000u32.to_le_bytes());
    bytes[RIGHT + 32..RIGHT + 36].copy_from_slice(&0x80000000u32.to_le_bytes());
    let memory = SliceMemory::new(BASE, &bytes).unwrap();
    let tree = U32PointerTree::new(&memory, BASE, 3).unwrap();
    assert!(tree.snapshot(TreeKeys::Unique).is_err());
    let entries = tree.snapshot(TreeKeys::Repeated).unwrap();
    assert_eq!(entries.len(), 3);
    assert_eq!(
        entries.iter().map(|x| x.value).collect::<Vec<_>>(),
        vec![42, 0, 99]
    );
    assert!(entries.iter().all(|x| x.key == 0x80000000));
}

#[test]
fn full_snapshot_checks_off_lookup_path_and_exact_reachable_count() {
    use shikifsrs_runtime::native_tree::TreeKeys;
    for variant in 0..5 {
        let mut bytes = fixture();
        match variant {
            0 => word(&mut bytes, RIGHT, BASE + RIGHT as u64),
            1 => word(&mut bytes, RIGHT + 16, 0),
            2 => word(&mut bytes, 16, 4),
            3 => bytes[RIGHT + 25] = 1,
            4 => bytes[RIGHT + 32..RIGHT + 36].copy_from_slice(&0u32.to_le_bytes()),
            _ => unreachable!(),
        }
        let memory = SliceMemory::new(BASE, &bytes).unwrap();
        assert_eq!(
            U32PointerTree::new(&memory, BASE, 4)
                .unwrap()
                .find(1)
                .unwrap(),
            Some(0)
        );
        assert!(
            U32PointerTree::new(&memory, BASE, 4)
                .unwrap()
                .snapshot(TreeKeys::Repeated)
                .is_err()
        );
    }
    let mut bytes = fixture();
    word(&mut bytes, 16, 0);
    word(&mut bytes, HEAD + 8, BASE + HEAD as u64);
    bytes.truncate(HEAD + 26);
    assert!(
        U32PointerTree::new(&SliceMemory::new(BASE, &bytes).unwrap(), BASE, 3)
            .unwrap()
            .snapshot(TreeKeys::Repeated)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn snapshot_refuses_visible_payload_or_child_change_after_copy() {
    use shikifsrs_runtime::native_tree::TreeKeys;
    struct Changing {
        bytes: Vec<u8>,
        watched: u64,
        reads: Cell<usize>,
    }
    impl AddressSpace for Changing {
        fn read_exact(&self, address: u64, output: &mut [u8]) -> Result<()> {
            SliceMemory::new(BASE, &self.bytes)?.read_exact(address, output)?;
            if address == self.watched {
                let reads = self.reads.get();
                self.reads.set(reads + 1);
                if reads > 0 {
                    output[0] ^= 1;
                }
            }
            Ok(())
        }
    }
    for watched in [
        BASE + RIGHT as u64,
        BASE + RIGHT as u64 + 25,
        BASE + RIGHT as u64 + 32,
        BASE + RIGHT as u64 + 40,
        BASE + 8,
        BASE + 16,
        BASE + HEAD as u64 + 8,
    ] {
        let memory = Changing {
            bytes: fixture(),
            watched,
            reads: Cell::new(0),
        };
        assert!(
            U32PointerTree::new(&memory, BASE, 3)
                .unwrap()
                .snapshot(TreeKeys::Repeated)
                .is_err()
        );
    }
}
