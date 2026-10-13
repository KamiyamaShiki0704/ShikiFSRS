//! Original process-lifetime overlay host. Renderer ABI comes from the consumer's
//! pinned hudhook dependency, not a FromSoftware SDK. Preparation and installation
//! are separate, observable attempts; no automatic retry, eject or DLL unload.
use std::sync::{Arc, Mutex};

#[cfg(windows)]
pub use hudhook;

/// A successful hook installation is not a game/render/feature acceptance result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OverlayState {
    Preparing,
    PreparationFailed(String),
    /// An install attempt began. Failure or panic may have left partial hooks.
    Installing,
    Installed,
    InstallationFailed(String),
    WorkerPanicked {
        installation_attempted: bool,
    },
}

/// A status handle, not an engine reference, hook owner or safe-unload guard.
/// Dropping it does not undo installation or stop the worker/render callbacks.
#[derive(Debug, Clone)]
pub struct OverlayTicket {
    state: Arc<Mutex<OverlayState>>,
}
impl OverlayTicket {
    pub fn state(&self) -> OverlayState {
        lock(&self.state).clone()
    }
}
fn lock(state: &Mutex<OverlayState>) -> std::sync::MutexGuard<'_, OverlayState> {
    state.lock().unwrap_or_else(|error| error.into_inner())
}

/// Internal worker has a testable backend; the original offline tests never
/// instantiate or call the real hook installer.
#[cfg(any(windows, test))]
fn run_worker(
    ticket: &OverlayTicket,
    prepare: impl FnOnce() -> Result<(), String>,
    install: impl FnOnce() -> Result<(), String>,
) {
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if let Err(message) = prepare() {
            *lock(&ticket.state) = OverlayState::PreparationFailed(message);
            return;
        }
        *lock(&ticket.state) = OverlayState::Installing;
        let completed = match install() {
            Ok(()) => OverlayState::Installed,
            Err(message) => OverlayState::InstallationFailed(message),
        };
        *lock(&ticket.state) = completed;
    }));
    if outcome.is_err() {
        let attempted = matches!(*lock(&ticket.state), OverlayState::Installing);
        *lock(&ticket.state) = OverlayState::WorkerPanicked {
            installation_attempted: attempted,
        };
    }
}

/// Starts one worker. Spawn failure is prior to preparation/installation; failed
/// attempted installation remains visible in the ticket, without a retry.
#[cfg(any(windows, test))]
fn launch(
    prepare: impl FnOnce() -> Result<(), String> + Send + 'static,
    install: impl FnOnce() -> Result<(), String> + Send + 'static,
) -> std::io::Result<OverlayTicket> {
    let ticket = OverlayTicket {
        state: Arc::new(Mutex::new(OverlayState::Preparing)),
    };
    let worker_ticket = ticket.clone();
    std::thread::Builder::new()
        .name("ShikiFSRS-overlay".into())
        .spawn(move || run_worker(&worker_ticket, prepare, install))?;
    Ok(ticket)
}

/// Returns a status ticket after dispatching preparation to a background thread.
/// The render-loop factory also runs there, only after successful preparation.
/// Call once from the host's process-attach/initialization path. This function
/// does not initialize an ER/NR profile, poll game memory or grant native access.
///
/// # Safety
/// Caller pins this DLL, hudhook and all consumer render/task callback code and
/// data for the process lifetime, including while preparation is pending or an
/// installation fails. Preparation must independently verify the owned executable
/// and required initialized game/renderer state. Caller supplies a permitted hook
/// installation phase outside loader-lock work, prevents duplicate/conflicting
/// installs, and retains any partial-hook obligations. Native game operations in
/// consumer callbacks require their separate SDK phase/lifetime contracts.
/// No safe hot-unload or hook rollback is promised by this status ticket.
#[cfg(windows)]
pub unsafe fn start_dx12(
    prepare: impl FnOnce() -> Result<(), String> + Send + 'static,
    render_loop: impl FnOnce() -> Box<dyn hudhook::ImguiRenderLoop + Send + Sync> + Send + 'static,
) -> std::io::Result<OverlayTicket> {
    launch(prepare, move || {
        // No with_hmodule/eject path: the host explicitly retains the DLL for
        // its process lifetime instead of attaching an unsafe unload capability.
        hudhook::Hudhook::builder()
            .with::<hudhook::hooks::dx12::ImguiDx12Hooks>(RenderLoop(render_loop()))
            .build()
            .apply()
            .map_err(|error| format!("hudhook installation: {error:?}"))
    })
}

// Owned forwarding adapter lets consumers keep all renderer callbacks, texture
// loading and message policy without assuming their concrete render-loop type.
#[cfg(windows)]
struct RenderLoop(Box<dyn hudhook::ImguiRenderLoop + Send + Sync>);
#[cfg(windows)]
impl hudhook::ImguiRenderLoop for RenderLoop {
    fn initialize<'a>(
        &'a mut self,
        ctx: &mut hudhook::imgui::Context,
        render: &'a mut dyn hudhook::RenderContext,
    ) {
        self.0.initialize(ctx, render);
    }
    fn before_render<'a>(
        &'a mut self,
        ctx: &mut hudhook::imgui::Context,
        render: &'a mut dyn hudhook::RenderContext,
    ) {
        self.0.before_render(ctx, render);
    }
    fn render(&mut self, ui: &mut hudhook::imgui::Ui) {
        self.0.render(ui);
    }
    fn before_wnd_proc(
        &self,
        hwnd: hudhook::windows::Win32::Foundation::HWND,
        message: u32,
        wparam: hudhook::windows::Win32::Foundation::WPARAM,
        lparam: hudhook::windows::Win32::Foundation::LPARAM,
    ) -> hudhook::BeforeWndProc {
        self.0.before_wnd_proc(hwnd, message, wparam, lparam)
    }
    fn after_wnd_proc(
        &self,
        hwnd: hudhook::windows::Win32::Foundation::HWND,
        message: u32,
        wparam: hudhook::windows::Win32::Foundation::WPARAM,
        lparam: hudhook::windows::Win32::Foundation::LPARAM,
    ) {
        self.0.after_wnd_proc(hwnd, message, wparam, lparam);
    }
    fn message_filter(&self, io: &hudhook::imgui::Io) -> hudhook::MessageFilter {
        self.0.message_filter(io)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    fn test_ticket() -> OverlayTicket {
        OverlayTicket {
            state: Arc::new(Mutex::new(OverlayState::Preparing)),
        }
    }
    #[test]
    fn preparation_failure_or_panic_never_attempts_installation() {
        let ticket = test_ticket();
        let calls = Cell::new(0);
        run_worker(
            &ticket,
            || Err("unknown executable".into()),
            || {
                calls.set(calls.get() + 1);
                Ok(())
            },
        );
        assert_eq!(
            ticket.state(),
            OverlayState::PreparationFailed("unknown executable".into())
        );
        assert_eq!(calls.get(), 0);
        run_worker(
            &ticket,
            || panic!("preparation panic"),
            || {
                calls.set(calls.get() + 1);
                Ok(())
            },
        );
        assert_eq!(
            ticket.state(),
            OverlayState::WorkerPanicked {
                installation_attempted: false
            }
        );
        assert_eq!(calls.get(), 0);
    }
    #[test]
    fn attempted_install_failure_and_panic_remain_visible_without_retry_or_drop_cleanup() {
        for panic in [false, true] {
            let ticket = test_ticket();
            let calls = Cell::new(0);
            run_worker(
                &ticket,
                || Ok(()),
                || {
                    assert_eq!(ticket.state(), OverlayState::Installing);
                    calls.set(calls.get() + 1);
                    if panic {
                        panic!("partial install panic");
                    }
                    Err("partial hook failure".into())
                },
            );
            let expected = if panic {
                OverlayState::WorkerPanicked {
                    installation_attempted: true,
                }
            } else {
                OverlayState::InstallationFailed("partial hook failure".into())
            };
            assert_eq!(ticket.state(), expected);
            let observer = ticket.clone();
            drop(ticket);
            assert_eq!(observer.state(), expected);
            assert_eq!(calls.get(), 1);
        }
    }
    #[test]
    fn worker_dispatch_is_nonblocking_and_orders_preparation_before_installation() {
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let (installed_tx, installed_rx) = std::sync::mpsc::channel();
        let ticket = launch(
            move || {
                entered_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                Ok(())
            },
            move || {
                installed_tx.send(()).unwrap();
                Ok(())
            },
        )
        .unwrap();
        entered_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        assert_eq!(ticket.state(), OverlayState::Preparing);
        assert!(installed_rx.try_recv().is_err());
        release_tx.send(()).unwrap();
        installed_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        // One completion notification can precede the final state store; no game
        // hook was installed and no runtime polling/unload test is claimed here.
        let ticket2 = test_ticket();
        run_worker(&ticket2, || Ok(()), || Ok(()));
        assert_eq!(ticket2.state(), OverlayState::Installed);
    }
}
