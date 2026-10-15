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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_enumeration() {
        // Just verify it doesn't panic
        let _ = list_input_devices();
        let _ = list_output_devices();
    }
}
