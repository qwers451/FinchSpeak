//! Thin Win32 probe. The owned handle prevents PID reuse from keeping a different process active.
use std::ffi::c_void;
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, RECT, WAIT_TIMEOUT};
use windows_sys::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
};
use windows_sys::Win32::Storage::FileSystem::{
    GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
};
use windows_sys::Win32::System::Threading::{
    OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
    QueryFullProcessImageNameW, WaitForSingleObject,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowRect, GetWindowThreadProcessId,
};

use super::classification::{classify, game_title};
use super::tracking::Probe;

pub struct Process(isize);

impl Drop for Process {
    fn drop(&mut self) {
        // This handle is created by OpenProcess and has exactly one owner.
        unsafe {
            CloseHandle(self.0 as HANDLE);
        }
    }
}

pub struct WindowsProbe;

impl Probe for WindowsProbe {
    type Process = Process;

    fn foreground(&self) -> Option<(Process, String)> {
        unsafe {
            let window = GetForegroundWindow();
            if window.is_null() {
                return None;
            }
            let mut pid = 0;
            GetWindowThreadProcessId(window, &mut pid);
            if pid == 0 || pid == std::process::id() {
                return None;
            }
            let handle = OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                0,
                pid,
            );
            if handle.is_null() {
                return None;
            }
            let process = Process(handle as isize);
            let mut path = vec![0u16; 32768];
            let mut length = path.len() as u32;
            if QueryFullProcessImageNameW(handle, 0, path.as_mut_ptr(), &mut length) == 0 {
                return None;
            }
            let path = String::from_utf16_lossy(&path[..length as usize]);
            let mut rect = RECT::default();
            let monitor = MonitorFromWindow(window, MONITOR_DEFAULTTONEAREST);
            let mut info = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            let covers_monitor = GetWindowRect(window, &mut rect) != 0
                && GetMonitorInfoW(monitor, &mut info) != 0
                && rect.left == info.rcMonitor.left
                && rect.top == info.rcMonitor.top
                && rect.right == info.rcMonitor.right
                && rect.bottom == info.rcMonitor.bottom;
            let fallback = classify(&path, covers_monitor)?;
            let (description, product) = version_titles(&path).unwrap_or_default();
            Some((
                process,
                game_title(description.as_deref(), product.as_deref(), &fallback),
            ))
        }
    }

    fn alive(&self, process: &Process) -> bool {
        unsafe { WaitForSingleObject(process.0 as HANDLE, 0) == WAIT_TIMEOUT }
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

unsafe fn version_value(data: &[u64], key: &str) -> Option<(*mut c_void, u32)> {
    unsafe {
        let mut value = std::ptr::null_mut();
        let mut len = 0;
        if VerQueryValueW(
            data.as_ptr().cast(),
            wide(key).as_ptr(),
            &mut value,
            &mut len,
        ) == 0
            || value.is_null()
        {
            None
        } else {
            Some((value, len))
        }
    }
}

unsafe fn version_titles(path: &str) -> Option<(Option<String>, Option<String>)> {
    unsafe {
        let path = wide(path);
        let size = GetFileVersionInfoSizeW(path.as_ptr(), std::ptr::null_mut());
        if size == 0 || size > 1024 * 1024 {
            return None;
        }
        let mut data = vec![0u64; (size as usize).div_ceil(8)];
        if GetFileVersionInfoW(path.as_ptr(), 0, size, data.as_mut_ptr().cast()) == 0 {
            return None;
        }
        let mut translations = Vec::new();
        if let Some((value, bytes)) = version_value(&data, "\\VarFileInfo\\Translation") {
            for index in 0..bytes as usize / 4 {
                let pair = value.cast::<u16>().add(index * 2);
                translations.push((pair.read_unaligned(), pair.add(1).read_unaligned()));
            }
        }
        translations.extend([(0x0409, 0x04b0), (0x0409, 0x04e4)]);
        let field = |name: &str| {
            translations.iter().find_map(|(language, codepage)| {
                let key = format!("\\StringFileInfo\\{language:04x}{codepage:04x}\\{name}");
                let (value, len) = version_value(&data, &key)?;
                let text = String::from_utf16_lossy(std::slice::from_raw_parts(
                    value.cast(),
                    len as usize,
                ));
                let text = text.trim_matches('\0').trim();
                (!text.is_empty()).then(|| text.to_string())
            })
        };
        Some((field("FileDescription"), field("ProductName")))
    }
}
