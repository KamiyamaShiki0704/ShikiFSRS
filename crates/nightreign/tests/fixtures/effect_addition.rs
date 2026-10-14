use super::*;
use shikifsrs_nightreign::effect_addition::{EffectAdditionBackend, EffectAdditionLimits};
#[derive(Clone, Copy)]
enum AdditionMode {
    Persistent,
    Refresh,
    Instant,
    Rejected,
    ErrorAfterMutation,
    OwnerChanged,
}
struct AdditionFixture<'a> {
    inner: OperationFixture<'a>,
    mode: AdditionMode,
}
impl<'a> AdditionFixture<'a> {
    fn new(fixture: &'a MappedFixture<'a>, mode: AdditionMode) -> Self {
        Self {
            inner: OperationFixture {
                inner: fixture,
                writes: RefCell::new(BTreeMap::new()),
                calls: RefCell::new(Vec::new()),
            },
            mode,
        }
    }
    fn put(&self, address: u64, bytes: &[u8]) {
        let mut writes = self.inner.writes.borrow_mut();
        for (offset, byte) in bytes.iter().enumerate() {
            writes.insert(address + offset as u64, *byte);
        }
    }
    fn word(&self, address: u64, value: u64) {
        self.put(address, &value.to_le_bytes());
    }
}
impl AddressSpace for AdditionFixture<'_> {
    fn read_exact(&self, address: u64, output: &mut [u8]) -> Result<()> {
        self.inner.read_exact(address, output)
    }
}
// SAFETY: Owned-memory simulation only. Supplies observable engine outcomes without
// invoking a native game wrapper, callback, notification, allocator or network.
unsafe impl EffectAdditionBackend for AdditionFixture<'_> {
    unsafe fn invoke_add_default(&self, entry: u64, character: u64, id: i32) -> Result<bool> {
        assert_eq!(entry, self.inner.inner.base + 0x3c7fd0);
        assert_eq!(character, 0x50000);
        assert_eq!(id, 123);
        self.inner.calls.borrow_mut().push(entry);
        match self.mode {
            AdditionMode::Rejected => return Ok(false),
            AdditionMode::Instant => return Ok(true),
            AdditionMode::Refresh => {
                self.put(0x1a0068, &0u32.to_le_bytes());
                return Ok(true);
            }
            _ => {}
        }
        let head = self.read_u64(0x190008)?;
        self.put(0x210000, &[0; 0x6c]);
        self.word(0x210000, 0x201000);
        self.put(0x210008, &id.to_le_bytes());
        if head == 0 {
            self.word(0x190008, 0x210000);
        } else {
            let mut tail = head;
            while self.read_u64(tail + 0x38)? != 0 {
                tail = self.read_u64(tail + 0x38)?;
            }
            self.word(tail + 0x38, 0x210000);
            self.word(0x210040, tail);
        }
        self.word(0x190028, u64::MAX);
        if matches!(self.mode, AdditionMode::OwnerChanged) {
            self.word(0x190010, 0);
        }
        if matches!(self.mode, AdditionMode::ErrorAfterMutation) {
            Err(Error::NativeOperationRejected(
                "synthetic post-allocation error",
            ))
        } else {
            Ok(true)
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
pub(super) fn check_effect_addition_receipts(
    file: &NightreignFile<'_>,
    fixture: &MappedFixture<'_>,
) {
    let limits = EffectAdditionLimits {
        nodes: 8,
        parameter_rows: 8,
    };
    for mode in [
        AdditionMode::Persistent,
        AdditionMode::Refresh,
        AdditionMode::Rejected,
        AdditionMode::ErrorAfterMutation,
        AdditionMode::OwnerChanged,
    ] {
        let adapter = AdditionFixture::new(fixture, mode);
        let operations = file.bind_effect_addition(&adapter, fixture.base).unwrap();
        // SAFETY: Only synthetic owned memory; native code and game subsystems are never invoked.
        let receipt = unsafe { operations.add_default(0x50000, 123, limits) }.unwrap();
        assert_eq!(receipt.parameter_address, 0x201000);
        assert_eq!(adapter.inner.calls.borrow().len(), 1);
        match mode {
            AdditionMode::Persistent => {
                assert_eq!(receipt.native_result, Ok(true));
                assert_eq!(receipt.after.unwrap().nodes.len(), 2);
            }
            AdditionMode::Refresh => {
                assert_eq!(receipt.native_result, Ok(true));
                assert_eq!(receipt.after.unwrap().nodes[0].address, 0x1a0000);
            }
            AdditionMode::Rejected => {
                assert_eq!(receipt.native_result, Ok(false));
                assert_eq!(receipt.after.unwrap(), receipt.before);
            }
            AdditionMode::ErrorAfterMutation => {
                assert!(receipt.native_result.is_err());
                assert_eq!(receipt.after.unwrap().nodes.len(), 2);
            }
            AdditionMode::OwnerChanged => {
                assert_eq!(receipt.native_result, Ok(true));
                assert!(receipt.after.is_err());
            }
            AdditionMode::Instant => unreachable!(),
        }
    }
    let mut empty = clone_fixture(fixture);
    empty.word(0x190008, 0);
    empty.word(0x190028, 0);
    let adapter = AdditionFixture::new(&empty, AdditionMode::Instant);
    let operations = file.bind_effect_addition(&adapter, empty.base).unwrap();
    // SAFETY: Accepted immediate effect outcome on owned bytes, with no persistent node.
    let receipt = unsafe { operations.add_default(0x50000, 123, limits) }.unwrap();
    assert_eq!(receipt.native_result, Ok(true));
    assert!(receipt.after.unwrap().nodes.is_empty());
    for (offset, value) in [
        (0x200008, 3u16.to_le_bytes().to_vec()),
        (0x20113e, 204i16.to_le_bytes().to_vec()),
    ] {
        let mut invalid = clone_fixture(fixture);
        invalid.put(offset, &value);
        if offset == 0x20113e {
            invalid.put(0x201140, &[1]);
        }
        let adapter = AdditionFixture::new(&invalid, AdditionMode::Persistent);
        let operations = file.bind_effect_addition(&adapter, invalid.base).unwrap();
        // SAFETY: SDK must reject schema/tier mapping before the synthetic invocation seam.
        assert!(unsafe { operations.add_default(0x50000, 123, limits) }.is_err());
        assert!(adapter.inner.calls.borrow().is_empty());
    }
    let adapter = AdditionFixture::new(fixture, AdditionMode::Persistent);
    let operations = file.bind_effect_addition(&adapter, fixture.base).unwrap();
    // SAFETY: Missing/invalid IDs must fail entirely within checked SDK preflight.
    assert!(unsafe { operations.add_default(0x50000, 999, limits) }.is_err());
    // SAFETY: Invalid ID fails before native invocation.
    assert!(unsafe { operations.add_default(0x50000, 0, limits) }.is_err());
    assert!(adapter.inner.calls.borrow().is_empty());
    let mut modified = clone_fixture(fixture);
    modified.changed_byte = Some((0x32f3a50, 1));
    let adapter = AdditionFixture::new(&modified, AdditionMode::Persistent);
    assert!(file.bind_effect_addition(&adapter, modified.base).is_err());
    assert!(adapter.inner.calls.borrow().is_empty());
}
