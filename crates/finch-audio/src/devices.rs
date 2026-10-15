//! Audio device enumeration and queries using `cpal`.

use cpal::traits::{DeviceTrait, HostTrait};

/// An enumerated audio device description.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioDeviceInfo {
    pub name: String,
    pub is_default: bool,
}

/// Enumerate all available audio input devices (microphones).
pub fn list_input_devices() -> Result<Vec<AudioDeviceInfo>, cpal::DefaultStreamConfigError> {
    let host = cpal::default_host();
    let default_name = host.default_input_device().and_then(|d| d.name().ok());

    let mut devices = Vec::new();
    if let Ok(input_devices) = host.input_devices() {
        for device in input_devices {
            if let Ok(name) = device.name() {
                let is_default = default_name.as_deref() == Some(&name);
                devices.push(AudioDeviceInfo { name, is_default });
            }
        }
    }
    Ok(devices)
}

/// Enumerate all available audio output devices (speakers/headphones).
pub fn list_output_devices() -> Result<Vec<AudioDeviceInfo>, cpal::DefaultStreamConfigError> {
    let host = cpal::default_host();
    let default_name = host.default_output_device().and_then(|d| d.name().ok());

    let mut devices = Vec::new();
    if let Ok(output_devices) = host.output_devices() {
        for device in output_devices {
            if let Ok(name) = device.name() {
                let is_default = default_name.as_deref() == Some(&name);
                devices.push(AudioDeviceInfo { name, is_default });
            }
        }
    }
    Ok(devices)
}

/// Which device to open for the user's choice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceChoice {
    /// The chosen device is present.
    Selected(String),
    /// Nothing was chosen: use the system default.
    Default,
    /// The chosen device is gone (unplugged): use the system default instead.
    FellBack { wanted: String },
}

/// Decide what to open given the names currently available and the user's choice.
pub fn resolve_device_choice(available: &[String], wanted: Option<&str>) -> DeviceChoice {
    match wanted.filter(|name| !name.is_empty()) {
        None => DeviceChoice::Default,
        Some(name) if available.iter().any(|a| a == name) => DeviceChoice::Selected(name.to_string()),
        Some(name) => DeviceChoice::FellBack { wanted: name.to_string() },
    }
}

/// Open the input device for the user's choice (falling back to the default when it is gone).
pub fn open_input_device(wanted: Option<&str>) -> (Option<cpal::Device>, DeviceChoice) {
    let host = cpal::default_host();
    let devices: Vec<cpal::Device> = host.input_devices().map(|d| d.collect()).unwrap_or_default();
    pick_device(devices, wanted, host.default_input_device())
}

/// Open the output device for the user's choice (falling back to the default when it is gone).
pub fn open_output_device(wanted: Option<&str>) -> (Option<cpal::Device>, DeviceChoice) {
    let host = cpal::default_host();
    let devices: Vec<cpal::Device> = host.output_devices().map(|d| d.collect()).unwrap_or_default();
    pick_device(devices, wanted, host.default_output_device())
}

fn pick_device(
    devices: Vec<cpal::Device>,
    wanted: Option<&str>,
    default: Option<cpal::Device>,
) -> (Option<cpal::Device>, DeviceChoice) {
    let available: Vec<String> = devices.iter().filter_map(|d| d.name().ok()).collect();
    let choice = resolve_device_choice(&available, wanted);
    let device = match &choice {
        DeviceChoice::Selected(name) => devices.into_iter().find(|d| d.name().ok().as_deref() == Some(name)),
        DeviceChoice::Default | DeviceChoice::FellBack { .. } => default,
    };
    (device, choice)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn test_device_enumeration() {
        // Just verify it doesn't panic
        let _ = list_input_devices();
        let _ = list_output_devices();
    }

    #[test]
    fn test_chosen_device_is_used_when_present() {
        let available = names(&["Speakers", "Headset"]);
        assert_eq!(resolve_device_choice(&available, Some("Headset")), DeviceChoice::Selected("Headset".into()));
    }

    /// An unplugged headset must not leave the user without sound: fall back and say so.
    #[test]
    fn test_missing_device_falls_back_to_default() {
        let available = names(&["Speakers"]);
        assert_eq!(
            resolve_device_choice(&available, Some("Headset")),
            DeviceChoice::FellBack { wanted: "Headset".into() }
        );
    }

    #[test]
    fn test_no_choice_means_default() {
        let available = names(&["Speakers"]);
        assert_eq!(resolve_device_choice(&available, None), DeviceChoice::Default);
        assert_eq!(resolve_device_choice(&available, Some("")), DeviceChoice::Default);
        assert_eq!(resolve_device_choice(&[], Some("Headset")), DeviceChoice::FellBack { wanted: "Headset".into() });
    }
}
