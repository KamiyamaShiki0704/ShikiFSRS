use super::*;
use shikifsrs_nightreign::action_id_requests::ActionIdRequestBackend;
#[derive(Clone, Copy)]
enum Mode {
    Normal,
    ErrorAfterWrites,
    ReplaceActor,
}
struct IdFixture<'a> {
    inner: OperationFixture<'a>,
    mode: Mode,
}
impl<'a> IdFixture<'a> {
    fn new(inner: &'a MappedFixture<'a>, mode: Mode) -> Self {
        Self {
            inner: OperationFixture {
                inner,
                writes: RefCell::new(BTreeMap::new()),
                calls: RefCell::new(Vec::new()),
            },
            mode,
        }
    }
    fn put(&self, address: u64, bytes: &[u8]) {
        for (j, b) in bytes.iter().enumerate() {
            self.inner
                .writes
                .borrow_mut()
                .insert(address + j as u64, *b);
        }
    }
}
impl AddressSpace for IdFixture<'_> {
    fn read_exact(&self, address: u64, output: &mut [u8]) -> Result<()> {
        self.inner.read_exact(address, output)
    }
}
// SAFETY: Only changes owned fixture bytes synchronously; no game/native CPU calls.
unsafe impl ActionIdRequestBackend for IdFixture<'_> {
    unsafe fn invoke_action_id_request(
        &self,
        entry: u64,
        interface: u64,
        id_bits: u32,
    ) -> Result<()> {
        assert_eq!(entry, self.inner.inner.base + 0x2c0dc0);
        assert_eq!(interface, 0x320000);
        assert_eq!(id_bits, 3001); // Forward original ID; 4001 would apply the adjustment twice.
        self.inner.calls.borrow_mut().push(entry);
        for address in [0x37c44c, 0x37c450] {
            self.put(address, &id_bits.to_le_bytes());
        }
        for address in [0x37de9c, 0x37dea0] {
            self.put(address, &4001u32.to_le_bytes());
        }
        self.put(0x37c445, &[1]);
        self.put(0x37e8d8, &[1]);
        match self.mode {
            Mode::Normal => Ok(()),
            Mode::ErrorAfterWrites => {
                Err(Error::InvalidProfile("owned adapter error after latches"))
            }
            Mode::ReplaceActor => {
                self.put(0x370000, &0u64.to_le_bytes());
                Ok(())
            }
        }
    }
}
pub(super) fn check_action_id_receipts(file: &NightreignFile<'_>, fixture: &MappedFixture<'_>) {
    for mode in [Mode::Normal, Mode::ErrorAfterWrites, Mode::ReplaceActor] {
        let adapter = IdFixture::new(fixture, mode);
        let operations = file
            .bind_action_id_requests(&adapter, fixture.base)
            .unwrap();
        // SAFETY: Fully owned simulated fields and synchronous adapter; no native invocation.
        let receipt = unsafe { operations.request(0x50000, 3001, 8, 8) }.unwrap();
        assert_eq!(receipt.resolution.requested_id_bits, 3001);
        assert_eq!(receipt.resolution.resolved_id_bits, 4001);
        assert!(receipt.predicted_table_member);
        assert_eq!(adapter.inner.calls.borrow().len(), 1);
        match mode {
            Mode::Normal => {
                assert!(receipt.native_result.is_ok());
                assert_eq!(receipt.latches_match_prediction, Some(true));
            }
            Mode::ErrorAfterWrites => {
                assert!(receipt.native_result.is_err());
                assert!(receipt.after.is_ok());
                assert_eq!(receipt.latches_match_prediction, Some(true));
            }
            Mode::ReplaceActor => {
                assert!(receipt.native_result.is_ok());
                assert!(receipt.after.is_err());
                assert_eq!(receipt.latches_match_prediction, None);
            }
        }
    }
    let patched = IdFixture::new(fixture, Mode::Normal);
    patched.put(fixture.base + 0x2bf9860 + 0x170, &0u64.to_le_bytes());
    let operations = file
        .bind_action_id_requests(&patched, fixture.base)
        .unwrap();
    // SAFETY: Owned adapter; preflight must reject patched slot before invocation.
    assert!(unsafe { operations.request(0x50000, 3001, 8, 8) }.is_err());
    assert!(patched.inner.calls.borrow().is_empty());
    let bounded = IdFixture::new(fixture, Mode::Normal);
    let operations = file
        .bind_action_id_requests(&bounded, fixture.base)
        .unwrap();
    // SAFETY: Owned adapter; invalid caller bounds fail before invocation.
    assert!(unsafe { operations.request(0x50000, 3001, 0, 8) }.is_err());
    assert!(bounded.inner.calls.borrow().is_empty());
}
