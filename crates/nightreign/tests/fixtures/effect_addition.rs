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
    TierAdvance,
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
        let (observed_id, parameter) = if matches!(self.mode, AdditionMode::TierAdvance) {
            (124i32, 0x2013d0)
        } else {
            (id, 0x201000)
        };
        self.word(0x210000, parameter);
        self.put(0x210008, &observed_id.to_le_bytes());
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
            AdditionMode::Instant | AdditionMode::TierAdvance => unreachable!(),
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
    {
        let mut invalid = clone_fixture(fixture);
        invalid.put(0x200008, &3u16.to_le_bytes());
        let adapter = AdditionFixture::new(&invalid, AdditionMode::Persistent);
        let operations = file.bind_effect_addition(&adapter, invalid.base).unwrap();
        // SAFETY: SDK must reject wrong schema before the synthetic invocation seam.
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
    check_tier_addition(file, fixture, limits);
}

fn tier_fixture<'a>(fixture: &MappedFixture<'a>, category: i16, next: bool) -> MappedFixture<'a> {
    let mut copied = clone_fixture(fixture);
    for address in [0x20113e, 0x1b013e] {
        copied.put(address, &category.to_le_bytes());
        copied.put(address + 2, &[1]);
    }
    copied.dword(0x1a0068, 0x80000000); // Native remapping includes this excluded match.
    if next {
        copied.put(0x20000a, &2u16.to_le_bytes());
        copied.dword(0x1ffff4, 2);
        copied.dword(0x200118, 124);
        copied.dword(0x20011c, 1);
        copied.word(0x200060, 0x13d0);
        copied.put(0x2013d0, &[0; 976]);
        copied.put(0x201526, &449u16.to_le_bytes());
    }
    copied
}
struct ChangingEffective<'a> {
    inner: AdditionFixture<'a>,
    copies: RefCell<usize>,
}
impl AddressSpace for ChangingEffective<'_> {
    fn read_exact(&self, address: u64, output: &mut [u8]) -> Result<()> {
        self.inner.read_exact(address, output)?;
        if address == 0x2013d0 && output.len() == 976 {
            let mut count = self.copies.borrow_mut();
            *count += 1;
            if *count > 1 {
                output[975] ^= 1;
            }
        }
        Ok(())
    }
}
// SAFETY: Same isolated owned-byte adapter; altered reads exercise SDK preflight only.
unsafe impl EffectAdditionBackend for ChangingEffective<'_> {
    unsafe fn invoke_add_default(&self, entry: u64, character: u64, id: i32) -> Result<bool> {
        // SAFETY: Delegates solely to the synthetic adapter with the same lifetime.
        unsafe { self.inner.invoke_add_default(entry, character, id) }
    }
}
fn check_tier_addition(
    file: &NightreignFile<'_>,
    fixture: &MappedFixture<'_>,
    limits: EffectAdditionLimits,
) {
    use shikifsrs_nightreign::effect_tiers::EffectTierOutcome;
    let next = tier_fixture(fixture, 204, true);
    let adapter = AdditionFixture::new(&next, AdditionMode::TierAdvance);
    let operations = file.bind_effect_addition(&adapter, next.base).unwrap();
    // SAFETY: Synthetic adapter asserts original requested ID 123, then emits planned ID 124.
    let receipt = unsafe { operations.add_default(0x50000, 123, limits) }.unwrap();
    assert_eq!(receipt.id_plan.effective_id, 124);
    assert_eq!(receipt.id_plan.outcome, EffectTierOutcome::NextTier);
    assert_eq!(receipt.effective_parameter_address, 0x2013d0);
    assert_eq!(receipt.parameter_address, 0x201000);
    assert_eq!(receipt.after.unwrap().nodes[1].id, 124);
    assert_eq!(adapter.inner.calls.borrow().len(), 1);
    let exhausted = tier_fixture(fixture, 204, false);
    let adapter = AdditionFixture::new(&exhausted, AdditionMode::Persistent);
    let operations = file.bind_effect_addition(&adapter, exhausted.base).unwrap();
    // SAFETY: Native rejection sentinel must be handled without reaching the adapter.
    assert!(unsafe { operations.add_default(0x50000, 123, limits) }.is_err());
    assert!(adapter.inner.calls.borrow().is_empty());
    let keep = tier_fixture(fixture, 206, false);
    let adapter = AdditionFixture::new(&keep, AdditionMode::Refresh);
    let operations = file.bind_effect_addition(&adapter, keep.base).unwrap();
    // SAFETY: Owned-byte final-tier refresh outcome only; no game code is invoked.
    let receipt = unsafe { operations.add_default(0x50000, 123, limits) }.unwrap();
    assert_eq!(receipt.id_plan.outcome, EffectTierOutcome::ExhaustedKeep);
    assert_eq!(receipt.effective_parameter_address, 0x201000);
    assert_eq!(receipt.native_result, Ok(true));
    let changing = ChangingEffective {
        inner: AdditionFixture::new(&next, AdditionMode::TierAdvance),
        copies: RefCell::new(0),
    };
    let operations = file.bind_effect_addition(&changing, next.base).unwrap();
    // SAFETY: Changing candidate payload must be rejected by preflight without invocation.
    assert!(unsafe { operations.add_default(0x50000, 123, limits) }.is_err());
    assert!(changing.inner.inner.calls.borrow().is_empty());
    let mut unreadable = tier_fixture(fixture, 204, true);
    unreadable.objects.remove(&0x20179f);
    let adapter = AdditionFixture::new(&unreadable, AdditionMode::TierAdvance);
    let operations = file
        .bind_effect_addition(&adapter, unreadable.base)
        .unwrap();
    // SAFETY: An indexed but incompletely readable effective row must not be invoked.
    assert!(unsafe { operations.add_default(0x50000, 123, limits) }.is_err());
    assert!(adapter.inner.calls.borrow().is_empty());
}
