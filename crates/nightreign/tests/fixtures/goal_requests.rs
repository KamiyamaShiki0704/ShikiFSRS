use super::*;
use shikifsrs_nightreign::goal_requests::GoalActionRequestBackend;
#[derive(Clone, Copy)]
enum Mode {
    Normal,
    ErrorAfterWrites,
    ReplacedImplementation,
}
struct GoalFixture<'a> {
    inner: OperationFixture<'a>,
    mode: Mode,
}
impl<'a> GoalFixture<'a> {
    fn new(fixture: &'a MappedFixture<'a>, mode: Mode) -> Self {
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
}
impl AddressSpace for GoalFixture<'_> {
    fn read_exact(&self, address: u64, output: &mut [u8]) -> Result<()> {
        self.inner.read_exact(address, output)
    }
}
// SAFETY: Mutates only owned synthetic control bytes; no real Goal pipeline or native calls.
unsafe impl GoalActionRequestBackend for GoalFixture<'_> {
    unsafe fn invoke_goal_request(
        &self,
        entry: u64,
        implementation: u64,
        interface: u64,
    ) -> Result<u8> {
        assert_eq!(entry, self.inner.inner.base + 0x2dcdb0);
        assert_eq!(implementation, 0x310000);
        assert_eq!(interface, 0x300018);
        self.inner.calls.borrow_mut().push(entry);
        let value = f32::from_bits(self.read_u32(0x300060)?);
        self.put(0x300318, &(value.trunc() as i32).to_le_bytes());
        self.put(0x30031c, &[1]);
        match self.mode {
            Mode::Normal => Ok(1),
            Mode::ErrorAfterWrites => Err(Error::NativeOperationRejected(
                "synthetic post-publication error",
            )),
            Mode::ReplacedImplementation => {
                self.put(0x3000a8, &0x50000u64.to_le_bytes());
                Ok(1)
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
pub(super) fn check_goal_request_receipts(file: &NightreignFile<'_>, fixture: &MappedFixture<'_>) {
    let mut source = clone_fixture(fixture);
    source.dword(0x300318, 99);
    source.put(0x30031c, &[0]);
    for mode in [
        Mode::Normal,
        Mode::ErrorAfterWrites,
        Mode::ReplacedImplementation,
    ] {
        let adapter = GoalFixture::new(&source, mode);
        let requests = file
            .bind_goal_action_requests(&adapter, source.base)
            .unwrap();
        // SAFETY: Synthetic initialized context and same-process owned-byte adapter only.
        let receipt = unsafe { requests.publish_from_context(0x300000) }.unwrap();
        assert_eq!(receipt.plan.requested_id_bits, 42);
        assert_eq!(receipt.plan.before.raw_id_bits, 99);
        assert_eq!(adapter.inner.calls.borrow().len(), 1);
        match mode {
            Mode::Normal => {
                assert_eq!(receipt.native_result, Ok(1));
                let after = receipt.after.unwrap();
                assert_eq!(after.raw_id_bits, 42);
                assert_eq!(after.raw_flag, 1);
                assert_eq!(after.raw_word_14, -1);
            }
            Mode::ErrorAfterWrites => {
                assert!(receipt.native_result.is_err());
                assert_eq!(receipt.after.unwrap().raw_id_bits, 42);
            }
            Mode::ReplacedImplementation => {
                assert_eq!(receipt.native_result, Ok(1));
                assert!(receipt.after.is_err());
            }
        }
    }
    for value in [f32::NAN, 2147483648.0] {
        let mut invalid = clone_fixture(&source);
        invalid.dword(0x300060, value.to_bits());
        let adapter = GoalFixture::new(&invalid, Mode::Normal);
        let requests = file
            .bind_goal_action_requests(&adapter, invalid.base)
            .unwrap();
        // SAFETY: Conversion failure must occur entirely within SDK preflight.
        assert!(unsafe { requests.publish_from_context(0x300000) }.is_err());
        assert!(adapter.inner.calls.borrow().is_empty());
    }
    let mut follow = clone_fixture(&source);
    follow.object(0x310000, 0x2c03e90);
    let adapter = GoalFixture::new(&follow, Mode::Normal);
    let requests = file
        .bind_goal_action_requests(&adapter, follow.base)
        .unwrap();
    // SAFETY: FollowPath's additional side effects are not this SDK operation.
    assert!(unsafe { requests.publish_from_context(0x300000) }.is_err());
    assert!(adapter.inner.calls.borrow().is_empty());
    let mut patched = clone_fixture(&source);
    patched.changed_byte = Some((0x2c03e48 + 0x10, 0));
    let adapter = GoalFixture::new(&patched, Mode::Normal);
    let requests = file
        .bind_goal_action_requests(&adapter, patched.base)
        .unwrap();
    // SAFETY: Patched loaded Goal virtual entry must be rejected before invocation.
    assert!(unsafe { requests.publish_from_context(0x300000) }.is_err());
    assert!(adapter.inner.calls.borrow().is_empty());
}
