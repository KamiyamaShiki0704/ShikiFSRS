use super::*;

#[derive(Default)]
struct Probe {
    id: i32,
    force: u8,
    calls: usize,
    receipt: u8,
}

unsafe extern "system" fn apply_probe(target: *mut c_void, id: i32, force: u8) {
    // SAFETY: The test supplies one uniquely borrowed live Probe allocation.
    let probe = unsafe { &mut *target.cast::<Probe>() };
    probe.id = id;
    probe.force = force;
    probe.calls += 1;
}
unsafe extern "system" fn remove_probe(container: *mut c_void, id: i32) -> u8 {
    // SAFETY: The test supplies one uniquely borrowed live Probe allocation.
    let probe = unsafe { &mut *container.cast::<Probe>() };
    probe.id = id;
    probe.calls += 1;
    probe.receipt
}

#[test]
fn native_apply_abi_preserves_id_pointer_and_normalized_force() {
    let mut probe = Probe::default();
    let pointer = (&mut probe as *mut Probe) as u64;
    for (id, force) in [(899000, true), (i32::MAX, false)] {
        // SAFETY: Original synthetic ABI entry; no game code or pointers are used.
        unsafe { invoke_apply(apply_probe, pointer, EffectId::new(id).unwrap(), force) };
        assert_eq!((probe.id, probe.force), (id, u8::from(force)));
    }
    assert_eq!(probe.calls, 2);
}

#[test]
fn native_remove_abi_distinguishes_absent_removed_and_invalid_receipt() {
    let mut probe = Probe::default();
    for receipt in [0, 1, 255] {
        probe.receipt = receipt;
        let pointer = (&mut probe as *mut Probe) as u64;
        // SAFETY: Only the synthetic test container and entry are passed through the native ABI.
        let result =
            unsafe { invoke_remove(remove_probe, pointer, EffectId::new(899001).unwrap()) };
        if receipt == 255 {
            assert!(result.is_err());
        } else {
            assert_eq!(result.unwrap(), receipt == 1);
        }
        assert_eq!(probe.id, 899001);
    }
    assert_eq!(probe.calls, 3);
}

#[test]
fn unknown_file_is_rejected_before_reading_any_current_process_address() {
    // SAFETY: Invalid bytes must fail identity before the deliberately impossible base is read.
    let result = unsafe { CurrentEffectEngine::bind(Arc::from(vec![0u8; 128]), u64::MAX, 4096, 1) };
    assert!(matches!(result, Err(Error::UnsupportedImage { .. })));
}
