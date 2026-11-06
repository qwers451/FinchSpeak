//! Event-driven WASAPI process loopback. All COM objects live on the capture MTA.
use crate::audio_loopback::LoopbackAudioError;
use std::mem::{ManuallyDrop, size_of};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use windows::Wdk::System::SystemServices::RtlGetVersion;
use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_FAILED};
use windows::Win32::Media::Audio::*;
use windows::Win32::System::Com::StructuredStorage::{
    PROPVARIANT, PROPVARIANT_0, PROPVARIANT_0_0, PROPVARIANT_0_0_0,
};
use windows::Win32::System::Com::{BLOB, COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};
use windows::Win32::System::SystemInformation::OSVERSIONINFOW;
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
use windows::Win32::System::Variant::VT_BLOB;
use windows::core::{HRESULT, Interface, PCWSTR, Ref, implement};

fn wasapi(error: windows::core::Error) -> LoopbackAudioError {
    LoopbackAudioError::Wasapi(format!("HRESULT {:#010X}: {error}", error.code().0 as u32))
}
fn failure(message: &str) -> LoopbackAudioError {
    LoopbackAudioError::Wasapi(format!("HRESULT 0x80004005: {message}"))
}

pub(crate) fn wait_for_activation(
    completed: &mpsc::Receiver<()>,
) -> Result<(), LoopbackAudioError> {
    completed
        .recv_timeout(Duration::from_secs(2))
        .map_err(|_| failure("ActivateCompleted timed out"))
}

/// Version gate requested by the client. Activation failure still falls back at runtime.
pub fn process_loopback_supported() -> bool {
    let mut version = OSVERSIONINFOW {
        dwOSVersionInfoSize: size_of::<OSVERSIONINFOW>() as u32,
        ..Default::default()
    };
    // SAFETY: version has the ABI size and remains valid for the call.
    unsafe { RtlGetVersion(&mut version).is_ok() && version.dwBuildNumber >= 19041 }
}

#[implement(IActivateAudioInterfaceCompletionHandler)]
struct Completion(mpsc::Sender<()>);
impl IActivateAudioInterfaceCompletionHandler_Impl for Completion_Impl {
    fn ActivateCompleted(
        &self,
        _: Ref<'_, IActivateAudioInterfaceAsyncOperation>,
    ) -> windows::core::Result<()> {
        let _ = self.0.send(());
        Ok(())
    }
}
struct Apartment;
impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}
struct Event(HANDLE);
impl Drop for Event {
    fn drop(&mut self) {
        if let Err(error) = unsafe { CloseHandle(self.0) } {
            tracing::warn!("{}", wasapi(error));
        }
    }
}

pub struct ProcessLoopback {
    running: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}
impl ProcessLoopback {
    /// Exclude pid's process tree unless include is true. Samples are 48 kHz stereo f32.
    pub fn start(
        pid: u32,
        include: bool,
        on_samples: impl FnMut(&[f32]) + Send + 'static,
    ) -> Result<Self, LoopbackAudioError> {
        let running = Arc::new(AtomicBool::new(true));
        let flag = Arc::clone(&running);
        let (tx, rx) = mpsc::channel();
        let thread = thread::Builder::new()
            .name("finch-process-loopback".into())
            .spawn(move || {
                let result = capture(pid, include, &flag, on_samples, &tx);
                if let Err(error) = result {
                    // Delivers initialization failures synchronously; runtime failures are logged.
                    tracing::error!("Process loopback stopped: {error}");
                    let _ = tx.send(Err(error));
                }
                flag.store(false, Ordering::Release);
            })?;
        let mut capture = Self {
            running,
            thread: Some(thread),
        };
        match rx.recv_timeout(Duration::from_secs(3)) {
            Ok(Ok(())) => Ok(capture),
            Ok(Err(error)) => {
                capture.stop();
                Err(error)
            }
            Err(_) => {
                capture.stop();
                Err(failure("activation timed out or capture thread closed"))
            }
        }
    }
    pub fn stop(&mut self) {
        self.running.store(false, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
impl Drop for ProcessLoopback {
    fn drop(&mut self) {
        self.stop();
    }
}

fn capture(
    pid: u32,
    include: bool,
    running: &AtomicBool,
    mut on_samples: impl FnMut(&[f32]),
    ready: &mpsc::Sender<Result<(), LoopbackAudioError>>,
) -> Result<(), LoopbackAudioError> {
    // SAFETY: all pointers below refer to live ABI-compatible objects; all COM interfaces,
    // event and apartment are released on this thread, in reverse initialization order.
    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED)
            .ok()
            .map_err(wasapi)?;
        let _apartment = Apartment;
        let mut params = AUDIOCLIENT_ACTIVATION_PARAMS {
            ActivationType: AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
            Anonymous: AUDIOCLIENT_ACTIVATION_PARAMS_0 {
                ProcessLoopbackParams: AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS {
                    TargetProcessId: pid,
                    ProcessLoopbackMode: if include {
                        PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE
                    } else {
                        PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE
                    },
                },
            },
        };
        // Borrowed BLOB, never PropVariantClear (the payload is stack-owned).
        let prop = ManuallyDrop::new(PROPVARIANT {
            Anonymous: PROPVARIANT_0 {
                Anonymous: ManuallyDrop::new(PROPVARIANT_0_0 {
                    vt: VT_BLOB,
                    Anonymous: PROPVARIANT_0_0_0 {
                        blob: BLOB {
                            cbSize: size_of::<AUDIOCLIENT_ACTIVATION_PARAMS>() as u32,
                            pBlobData: (&mut params as *mut AUDIOCLIENT_ACTIVATION_PARAMS).cast(),
                        },
                    },
                    ..Default::default()
                }),
            },
        });
        let (tx, rx) = mpsc::channel();
        let handler: IActivateAudioInterfaceCompletionHandler = Completion(tx).into();
        let operation = ActivateAudioInterfaceAsync(
            VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK,
            &IAudioClient::IID,
            Some(&*prop),
            &handler,
        )
        .map_err(wasapi)?;
        wait_for_activation(&rx)?;
        let mut hr = HRESULT(0);
        let mut unknown = None;
        operation
            .GetActivateResult(&mut hr, &mut unknown)
            .map_err(wasapi)?;
        hr.ok().map_err(wasapi)?;
        let client: IAudioClient = unknown
            .ok_or_else(|| failure("activation returned no interface"))?
            .cast()
            .map_err(wasapi)?;
        // WAVE_FORMAT_IEEE_FLOAT = 3; process loopback does not implement GetMixFormat.
        let format = WAVEFORMATEX {
            wFormatTag: 3,
            nChannels: 2,
            nSamplesPerSec: 48_000,
            nAvgBytesPerSec: 384_000,
            nBlockAlign: 8,
            wBitsPerSample: 32,
            cbSize: 0,
        };
        client
            .Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                AUDCLNT_STREAMFLAGS_LOOPBACK
                    | AUDCLNT_STREAMFLAGS_EVENTCALLBACK
                    | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
                2_000_000,
                0,
                &format,
                None,
            )
            .map_err(wasapi)?;
        let event = Event(CreateEventW(None, false, false, PCWSTR::null()).map_err(wasapi)?);
        client.SetEventHandle(event.0).map_err(wasapi)?;
        let reader: IAudioCaptureClient = client.GetService().map_err(wasapi)?;
        client.Start().map_err(wasapi)?;
        let _ = ready.send(Ok(()));
        let result = (|| {
            // Fixed scratch space, also handles silent packets larger than this slice.
            let zeros = [0.0f32; 2048];
            while running.load(Ordering::Acquire) {
                if WaitForSingleObject(event.0, 100) == WAIT_FAILED {
                    return Err(wasapi(windows::core::Error::from_thread()));
                }
                while running.load(Ordering::Acquire)
                    && reader.GetNextPacketSize().map_err(wasapi)? > 0
                {
                    let (mut data, mut frames, mut flags) = (std::ptr::null_mut(), 0, 0);
                    reader
                        .GetBuffer(&mut data, &mut frames, &mut flags, None, None)
                        .map_err(wasapi)?;
                    let count = frames as usize * 2;
                    if flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0 {
                        let mut remaining = count;
                        while remaining > 0 {
                            let len = remaining.min(zeros.len());
                            on_samples(&zeros[..len]);
                            remaining -= len;
                        }
                    } else if !data.is_null() && count > 0 {
                        on_samples(std::slice::from_raw_parts(data.cast::<f32>(), count));
                    }
                    reader.ReleaseBuffer(frames).map_err(wasapi)?;
                }
            }
            Ok(())
        })();
        let stopped = client.Stop().map_err(wasapi);
        result.and(stopped)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};
    #[test]
    fn version_check_is_true_on_this_machine() {
        assert!(process_loopback_supported());
    }
    #[test]
    fn excluding_self_starts_and_stops() {
        let mut capture = ProcessLoopback::start(std::process::id(), false, |_| {}).unwrap();
        capture.stop();
    }
    #[test]
    fn stop_joins_capture_thread_quickly() {
        let mut capture = ProcessLoopback::start(std::process::id(), false, |_| {}).unwrap();
        let start = Instant::now();
        capture.stop();
        assert!(start.elapsed() < Duration::from_millis(300));
    }
    #[test]
    fn silence_produces_no_error() {
        let mut capture = ProcessLoopback::start(std::process::id(), false, |_| {}).unwrap();
        std::thread::sleep(Duration::from_millis(500));
        assert!(!capture.thread.as_ref().unwrap().is_finished());
        capture.stop();
    }
}
