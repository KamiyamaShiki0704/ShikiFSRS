//! Original submission log, never a game/hook/resource backend.
use super::*;
use crate::objects::{ObjectKind, ObjectObservation};
#[derive(Default)]
struct Backend {
    calls: Vec<(GparamStage, u16)>,
    request_errors: usize,
    prime_errors: usize,
    mode: u8,
}
fn receipt<T>(id: CommonEventId, value: T, stage: GparamStage) -> NativeGparamCall<T> {
    NativeGparamCall {
        manager: ObjectObservation {
            address: 0x1000,
            kind: if stage == GparamStage::FileRequest {
                ObjectKind::FileResourceManager
            } else {
                ObjectKind::WorldSceneDrawParameters
            },
        },
        id,
        value,
        postflight: Ok(()),
    }
}
// SAFETY: Entirely original log backend; Err precedes its synthetic submission.
unsafe impl GparamTaskBackend for Backend {
    fn elapsed_ms(&self) -> Result<u64> {
        Ok(0)
    }
    unsafe fn request(&mut self, id: CommonEventId) -> Result<NativeGparamCall<Option<u64>>> {
        if self.request_errors != 0 {
            self.request_errors -= 1;
            return Err(Error::NativeOperationRejected("request not invoked"));
        }
        self.calls.push((GparamStage::FileRequest, id.value()));
        if self.mode == 4 {
            panic!("request partial attempt");
        }
        let mut call = receipt(
            id,
            if self.mode == 1 { None } else { Some(0x2000) },
            GparamStage::FileRequest,
        );
        if self.mode == 2 {
            call.postflight = Err(Error::InvalidProfile("request postflight"));
        }
        if self.mode == 3 {
            call.id = CommonEventId::new(6).unwrap();
        }
        Ok(call)
    }
    unsafe fn prime(&mut self, id: CommonEventId) -> Result<NativeGparamCall<()>> {
        if self.prime_errors != 0 {
            self.prime_errors -= 1;
            return Err(Error::NativeOperationRejected("prime not invoked"));
        }
        self.calls.push((GparamStage::Prime, id.value()));
        if self.mode == 6 {
            panic!("prime partial attempt");
        }
        let mut call = receipt(id, (), GparamStage::Prime);
        if self.mode == 5 {
            call.postflight = Err(Error::InvalidProfile("prime postflight"));
        }
        if self.mode == 7 {
            call.id = CommonEventId::new(6).unwrap();
        }
        Ok(call)
    }
}
fn config() -> GparamPreloadConfig {
    GparamPreloadConfig {
        start_delay_ms: 0,
        retry_every_frames: 1,
        attempts_per_id: 3,
        ..Default::default()
    }
}
fn tick(loader: &mut GparamPreloader, now: u64, backend: &mut Backend) -> Result<bool> {
    // SAFETY: Self-authored log backend; no native allocations, memory or calls.
    unsafe { loader.tick(now, backend) }
}
#[test]
fn default_delay_and_cadence_order_duplicates_and_each_frame_interval_one() {
    let defaults = GparamPreloadConfig::default();
    assert_eq!(
        (
            defaults.ids[0].value(),
            defaults.start_delay_ms,
            defaults.attempts_per_id,
            defaults.retry_every_frames
        ),
        (5, 5000, 120, 60)
    );
    let mut backend = Backend {
        request_errors: 2,
        ..Default::default()
    };
    let mut loader = GparamPreloader::new(defaults, 100).unwrap();
    for now in [100, 5099] {
        assert!(!tick(&mut loader, now, &mut backend).unwrap());
    }
    assert_eq!(loader.frame(), 0);
    assert!(tick(&mut loader, 5100, &mut backend).unwrap());
    assert_eq!(loader.submissions()[0].attempts, 1);
    for _ in 0..59 {
        assert!(!tick(&mut loader, 5100, &mut backend).unwrap());
    }
    assert!(tick(&mut loader, 5100, &mut backend).unwrap());
    assert_eq!(loader.submissions()[0].attempts, 2);
    for _ in 0..59 {
        assert!(!tick(&mut loader, 5100, &mut backend).unwrap());
    }
    assert!(tick(&mut loader, 5100, &mut backend).unwrap());
    assert_eq!(
        backend.calls,
        vec![(GparamStage::FileRequest, 5), (GparamStage::Prime, 5)]
    );
    let mut cfg = config();
    cfg.ids = vec![
        CommonEventId::new(5).unwrap(),
        CommonEventId::new(0).unwrap(),
        CommonEventId::new(5).unwrap(),
        CommonEventId::new(65535).unwrap(),
    ];
    let mut loader = GparamPreloader::new(cfg, 0).unwrap();
    let mut backend = Backend::default();
    tick(&mut loader, 0, &mut backend).unwrap();
    assert_eq!(
        backend.calls.iter().map(|x| x.1).collect::<Vec<_>>(),
        vec![5, 5, 0, 0, 5, 5, 65535, 65535]
    );
    assert_eq!(
        loader
            .submissions()
            .iter()
            .map(|x| x.ordinal)
            .collect::<Vec<_>>(),
        vec![0, 1, 2, 3]
    );
    assert!(loader.settled());
    let mut backend = Backend {
        request_errors: 2,
        ..Default::default()
    };
    let mut loader = GparamPreloader::new(config(), 0).unwrap();
    for _ in 0..3 {
        assert!(tick(&mut loader, 0, &mut backend).unwrap());
    }
    assert_eq!(loader.frame(), 3);
    assert_eq!(
        loader.submissions()[0].state,
        GparamSubmissionState::Submitted
    );
    assert!(!tick(&mut loader, 0, &mut backend).unwrap());
}
#[test]
fn retry_only_preflight_failures_and_retain_file_request_during_prime_retries() {
    let mut backend = Backend {
        prime_errors: 2,
        ..Default::default()
    };
    let mut loader = GparamPreloader::new(config(), 0).unwrap();
    for attempt in 1..=3 {
        tick(&mut loader, 0, &mut backend).unwrap();
        assert_eq!(loader.submissions()[0].attempts, attempt);
        assert_eq!(
            loader.submissions()[0].file_request.as_ref().unwrap().value,
            Some(0x2000)
        );
    }
    assert_eq!(
        backend.calls,
        vec![(GparamStage::FileRequest, 5), (GparamStage::Prime, 5)]
    );
    assert_eq!(
        loader.submissions()[0].state,
        GparamSubmissionState::Submitted
    );
    assert!(loader.submissions()[0].last_error.is_none());
    drop(loader);
    assert_eq!(backend.calls.len(), 2); // No resource release, Drop retry or hidden prime.
    let mut backend = Backend {
        request_errors: 20,
        ..Default::default()
    };
    let mut loader = GparamPreloader::new(config(), 0).unwrap();
    for _ in 0..3 {
        tick(&mut loader, 0, &mut backend).unwrap();
    }
    assert_eq!(
        loader.submissions()[0].state,
        GparamSubmissionState::Exhausted
    );
    assert_eq!(loader.submissions()[0].attempts, 3);
    assert!(loader.submissions()[0].file_request.is_none());
    assert!(!tick(&mut loader, 0, &mut backend).unwrap());
    assert_eq!(backend.request_errors, 17);
    let mut backend = Backend {
        prime_errors: 20,
        ..Default::default()
    };
    let mut loader = GparamPreloader::new(config(), 0).unwrap();
    for _ in 0..3 {
        tick(&mut loader, 0, &mut backend).unwrap();
    }
    assert_eq!(
        loader.submissions()[0].state,
        GparamSubmissionState::Exhausted
    );
    assert!(loader.submissions()[0].file_request.is_some());
    assert!(loader.submissions()[0].prime.is_none());
    assert_eq!(backend.calls, vec![(GparamStage::FileRequest, 5)]);
}
#[test]
fn null_failed_postflight_foreign_receipt_and_panics_block_following_ids_without_retry() {
    for mode in 1..=7 {
        let mut cfg = config();
        cfg.ids.push(CommonEventId::new(9).unwrap());
        let mut loader = GparamPreloader::new(cfg, 0).unwrap();
        let mut backend = Backend {
            mode,
            ..Default::default()
        };
        tick(&mut loader, 0, &mut backend).unwrap();
        let first = &loader.submissions()[0];
        assert_eq!(
            first.state,
            GparamSubmissionState::Uncertain(if mode <= 4 {
                GparamStage::FileRequest
            } else {
                GparamStage::Prime
            })
        );
        assert_eq!(first.attempts, 1);
        assert!(first.last_error.is_some());
        assert_eq!(first.file_request.is_some(), mode != 4);
        assert_eq!(first.prime.is_some(), [5, 7].contains(&mode));
        if mode == 1 {
            assert_eq!(first.file_request.as_ref().unwrap().value, None);
        }
        if mode == 2 {
            assert!(first.file_request.as_ref().unwrap().postflight.is_err());
        }
        assert!(loader.blocked());
        assert!(!loader.settled());
        let calls = backend.calls.clone();
        for now in [1, 100, 999] {
            assert!(!tick(&mut loader, now, &mut backend).unwrap());
        }
        assert_eq!(backend.calls, calls);
        assert!(calls.iter().all(|call| call.1 == 5));
        assert_eq!(loader.submissions()[1].attempts, 0);
        loader.cancel_pending();
        assert!(loader.settled());
        assert!(loader.blocked());
        assert_eq!(
            loader.submissions()[1].state,
            GparamSubmissionState::Cancelled
        );
    }
}
#[test]
fn disabled_empty_noop_selective_operations_stop_clock_and_boundaries() {
    for (request, prime) in [(false, false), (true, false), (false, true)] {
        let mut cfg = config();
        cfg.request_filecap = request;
        cfg.prime_drawparam = prime;
        let mut loader = GparamPreloader::new(cfg, 0).unwrap();
        let mut backend = Backend::default();
        tick(&mut loader, 0, &mut backend).unwrap();
        assert_eq!(
            backend.calls.len(),
            usize::from(request) + usize::from(prime)
        );
        assert_eq!(
            loader.submissions()[0].state,
            if request || prime {
                GparamSubmissionState::Submitted
            } else {
                GparamSubmissionState::NoOperations
            }
        );
    }
    let mut cfg = config();
    cfg.enabled = false;
    let mut loader = GparamPreloader::new(cfg, 0).unwrap();
    let mut backend = Backend::default();
    assert!(!tick(&mut loader, 0, &mut backend).unwrap());
    assert_eq!(
        loader.submissions()[0].state,
        GparamSubmissionState::Disabled
    );
    let mut cfg = config();
    cfg.ids.clear();
    assert!(GparamPreloader::new(cfg, 0).unwrap().settled());
    let mut loader = GparamPreloader::new(config(), 10).unwrap();
    assert!(tick(&mut loader, 9, &mut backend).is_err());
    assert_eq!(loader.frame(), 0);
    assert!(backend.calls.is_empty());
    loader.cancel_pending();
    assert!(!tick(&mut loader, 10, &mut backend).unwrap());
    assert_eq!(
        loader.submissions()[0].state,
        GparamSubmissionState::Cancelled
    );
    let mut cfg = config();
    cfg.start_delay_ms = 1;
    assert!(GparamPreloader::new(cfg, u64::MAX).is_err());
    for mutate in [0, 1, 2] {
        let mut cfg = config();
        match mutate {
            0 => cfg.attempts_per_id = 0,
            1 => cfg.retry_every_frames = 0,
            _ => cfg.ids.resize(257, CommonEventId::new(5).unwrap()),
        };
        assert!(GparamPreloader::new(cfg, 0).is_err());
    }
    let mut cfg = config();
    cfg.ids.resize(256, CommonEventId::new(5).unwrap());
    assert!(GparamPreloader::new(cfg, 0).is_ok());
    let mut loader = GparamPreloader::new(config(), 0).unwrap();
    loader.frame = u64::MAX;
    assert!(tick(&mut loader, 0, &mut backend).is_err());
    loader.frame = u64::MAX - 1;
    loader.next_due_frame = u64::MAX;
    assert!(tick(&mut loader, 0, &mut backend).is_err());
    assert_eq!(loader.submissions()[0].attempts, 0);
    assert!(backend.calls.is_empty());
}
