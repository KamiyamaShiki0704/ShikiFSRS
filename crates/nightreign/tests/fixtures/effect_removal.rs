use super::*;
use shikifsrs_nightreign::effect_operations::EffectRemovalBackend;

#[derive(Clone, Copy)]
enum RemovalMode {
    Normal,
    NativeErrorAfterMutation,
    OwnerChangedAfterMutation,
}
struct RemovalFixture<'a> {
    inner: OperationFixture<'a>,
    retired: RefCell<Vec<u64>>,
    mode: RemovalMode,
}
impl<'a> RemovalFixture<'a> {
    fn new(fixture: &'a MappedFixture<'a>, mode: RemovalMode) -> Self {
        Self {
            inner: OperationFixture {
                inner: fixture,
                writes: RefCell::new(BTreeMap::new()),
                calls: RefCell::new(Vec::new()),
            },
            retired: RefCell::new(Vec::new()),
            mode,
        }
    }
    fn word(&self, address: u64, value: u64) {
        let mut writes = self.inner.writes.borrow_mut();
        for (offset, byte) in value.to_le_bytes().into_iter().enumerate() {
            writes.insert(address + offset as u64, byte);
        }
    }
}
impl AddressSpace for RemovalFixture<'_> {
    fn read_exact(&self, address: u64, output: &mut [u8]) -> Result<()> {
        let end = address
            .checked_add(output.len() as u64)
            .ok_or(Error::AddressOverflow)?;
        if self
            .retired
            .borrow()
            .iter()
            .any(|start| address < start + 0x88 && end > *start)
        {
            return Err(Error::OutOfBounds {
                address,
                length: output.len(),
            });
        }
        self.inner.read_exact(address, output)
    }
}
// SAFETY: This adapter mutates and retires only owned synthetic bytes. It never calls game
// code, heap or callbacks; it supplies observable engine outcomes for SDK receipt tests.
unsafe impl EffectRemovalBackend for RemovalFixture<'_> {
    unsafe fn invoke_remove_id(&self, entry: u64, effects: u64, id: i32) -> Result<bool> {
        assert_eq!(entry, self.inner.inner.base + 0x504e80);
        assert_eq!(effects, 0x190000);
        self.inner.calls.borrow_mut().push(entry);
        let mut node = self.read_u64(effects + 8)?;
        while node != 0 && self.read_u32(node + 8)? as i32 != id {
            node = self.read_u64(node + 0x38)?;
        }
        if node == 0 {
            return Ok(false);
        }
        let next = self.read_u64(node + 0x38)?;
        let previous = self.read_u64(node + 0x40)?;
        let param = self.read_u64(node)?;
        let mut state = [0; 2];
        self.read_exact(param + 0x156, &mut state)?;
        let state = u16::from_le_bytes(state);
        if previous == 0 {
            self.word(effects + 8, next);
        } else {
            self.word(previous + 0x38, next);
        }
        if next != 0 {
            self.word(next + 0x40, previous);
        }
        // Saturated bucket outcome: count remaining nodes, including excluded peers.
        let shift = ((u32::from(state) % 15) + u32::from(state != 0)) * 4;
        let mut count = 0u64;
        let mut peer = self.read_u64(effects + 8)?;
        while peer != 0 {
            let p = self.read_u64(peer)?;
            let mut value = [0; 2];
            self.read_exact(p + 0x156, &mut value)?;
            let value = u16::from_le_bytes(value);
            if ((u32::from(value) % 15) + u32::from(value != 0)) * 4 == shift {
                count += 1;
            }
            peer = self.read_u64(peer + 0x38)?;
        }
        let summary =
            (self.read_u64(effects + 0x28)? & !(0xfu64 << shift)) | (count.min(15) << shift);
        self.word(effects + 0x28, summary);
        self.retired.borrow_mut().push(node);
        match self.mode {
            RemovalMode::Normal => Ok(true),
            RemovalMode::NativeErrorAfterMutation => Err(Error::NativeOperationRejected(
                "synthetic failure after completed unlink",
            )),
            RemovalMode::OwnerChangedAfterMutation => {
                self.word(effects + 0x10, 0);
                Ok(true)
            }
        }
    }
}
fn clone_fixture<'a>(fixture: &MappedFixture<'a>) -> MappedFixture<'a> {
    MappedFixture {
        image: fixture.image,
        base: fixture.base,
        objects: fixture.objects.clone(),
        changed_byte: None,
    }
}
pub(super) fn check_effect_removal_receipts(
    file: &NightreignFile<'_>,
    fixture: &MappedFixture<'_>,
) {
    let normal = RemovalFixture::new(fixture, RemovalMode::Normal);
    let operations = file.bind_effect_removal(&normal, fixture.base).unwrap();
    // SAFETY: Isolated owned-byte backend; no game process, native function or real heap.
    let absent = unsafe { operations.remove_id(0x50000, 999, 8) }.unwrap();
    assert_eq!(absent.native_result, Ok(false));
    assert!(normal.retired.borrow().is_empty());
    // SAFETY: Invalid ID is rejected before reaching this synthetic invocation seam.
    assert!(unsafe { operations.remove_id(0x50000, 0, 8) }.is_err());
    assert_eq!(normal.inner.calls.borrow().len(), 1);
    // SAFETY: Same isolated synthetic graph and synchronous adapter.
    let removed = unsafe { operations.remove_id(0x50000, 123, 8) }.unwrap();
    assert_eq!(removed.native_result, Ok(true));
    assert!(removed.after.unwrap().nodes.is_empty());
    assert_eq!(removed.before.nodes[0].address, 0x1a0000);
    assert!(normal.read_u64(0x1a0000).is_err()); // Retired node must never be dereferenced by postflight.
    assert_eq!(*normal.retired.borrow(), [0x1a0000]);
    let mut duplicate = clone_fixture(fixture);
    duplicate.dword(0x1a0068, 0x80000000);
    duplicate.word(0x1a0038, 0x1a1000);
    duplicate.put(0x1a1000, &[0; 0x6c]);
    duplicate.word(0x1a1000, 0x1b0000);
    duplicate.dword(0x1a1008, 123);
    duplicate.word(0x1a1040, 0x1a0000);
    let adapter = RemovalFixture::new(&duplicate, RemovalMode::Normal);
    let operations = file.bind_effect_removal(&adapter, duplicate.base).unwrap();
    // SAFETY: Owned synthetic two-node list, including an excluded first ID match.
    let receipt = unsafe { operations.remove_id(0x50000, 123, 8) }.unwrap();
    let after = receipt.after.unwrap();
    assert_eq!(receipt.native_result, Ok(true));
    assert_eq!(after.nodes[0].address, 0x1a1000);
    assert_eq!(after.nodes[0].previous_address, 0);
    assert!(after.contains_id(123)); // True native result cannot mean all copies are gone.
    for mode in [
        RemovalMode::NativeErrorAfterMutation,
        RemovalMode::OwnerChangedAfterMutation,
    ] {
        let adapter = RemovalFixture::new(fixture, mode);
        let operations = file.bind_effect_removal(&adapter, fixture.base).unwrap();
        // SAFETY: Synthetic adapter deliberately reports or introduces post-call failure.
        let receipt = unsafe { operations.remove_id(0x50000, 123, 8) }.unwrap();
        assert_eq!(*adapter.retired.borrow(), [0x1a0000]);
        match mode {
            RemovalMode::NativeErrorAfterMutation => {
                assert!(receipt.native_result.is_err());
                assert!(receipt.after.unwrap().nodes.is_empty());
            }
            RemovalMode::OwnerChangedAfterMutation => {
                assert_eq!(receipt.native_result, Ok(true));
                assert!(receipt.after.is_err());
            }
            RemovalMode::Normal => unreachable!(),
        }
    }
    let mut corrupted = clone_fixture(fixture);
    corrupted.changed_byte = Some((0x2c307f8 + 0x510, 0));
    let adapter = RemovalFixture::new(&corrupted, RemovalMode::Normal);
    let operations = file.bind_effect_removal(&adapter, fixture.base).unwrap();
    // SAFETY: SDK must reject the synthetic patched virtual dependency before invocation.
    assert!(unsafe { operations.remove_id(0x50000, 123, 8) }.is_err());
    assert!(adapter.inner.calls.borrow().is_empty());
}
