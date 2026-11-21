//! Audio device change watcher.
//!
//! Uses CoreAudio's AudioObjectAddPropertyListener to receive notifications
//! when audio devices are connected, disconnected, or the default changes.
//!
//! Notifications are debounced (500ms quiet period) because CoreAudio fires
//! multiple rapid callbacks per device change and Bluetooth devices need time
//! to fully register before enumeration.
//!
//! This module is macOS-specific and uses the coreaudio-sys bindings.

use std::ffi::c_void;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use coreaudio_sys::{
    kAudioHardwarePropertyDefaultOutputDevice, kAudioHardwarePropertyDevices,
    kAudioObjectPropertyElementMain, kAudioObjectPropertyScopeGlobal, kAudioObjectSystemObject,
    AudioObjectAddPropertyListener, AudioObjectID, AudioObjectPropertyAddress,
    AudioObjectRemovePropertyListener, OSStatus, UInt32,
};
use tracing::{debug, error, info};

use super::devices::query_mpv_devices;
use super::mpv_process::get_socket_path;
use super::types::AudioDevice;

/// Debounce window: wait for this period of quiet after the last notification
/// before re-enumerating devices. Bluetooth devices can take several hundred
/// milliseconds to fully register with CoreAudio after the first notification.
const DEBOUNCE_DURATION: Duration = Duration::from_millis(500);

/// Handle to the device watcher thread.
pub struct DeviceWatcher {
    /// Sender to signal shutdown.
    shutdown_tx: mpsc::Sender<()>,
    /// Join handle for the watcher thread.
    thread_handle: Option<thread::JoinHandle<()>>,
}

impl DeviceWatcher {
    /// Start the device watcher.
    ///
    /// The provided callback will be called whenever audio devices change,
    /// after a debounce period to coalesce rapid CoreAudio notifications.
    pub fn start<F>(on_change: F) -> Self
    where
        F: Fn(Vec<AudioDevice>) + Send + 'static,
    {
        let (shutdown_tx, shutdown_rx) = mpsc::channel();

        let thread_handle = thread::spawn(move || {
            info!("Device watcher thread started");

            // Channel to receive device change notifications from CoreAudio callbacks.
            // Both property listeners share this channel.
            let (notify_tx, notify_rx) = mpsc::channel::<()>();

            // Property addresses to watch
            let devices_address = AudioObjectPropertyAddress {
                mSelector: kAudioHardwarePropertyDevices,
                mScope: kAudioObjectPropertyScopeGlobal,
                mElement: kAudioObjectPropertyElementMain,
            };
            let default_address = AudioObjectPropertyAddress {
                mSelector: kAudioHardwarePropertyDefaultOutputDevice,
                mScope: kAudioObjectPropertyScopeGlobal,
                mElement: kAudioObjectPropertyElementMain,
            };

            // Box the sender once; both listeners share the same pointer.
            // The sender is Clone-safe internally, and we only read through the pointer.
            let notify_tx_box = Box::new(notify_tx);
            let client_data = Box::into_raw(notify_tx_box) as *mut c_void;

            // Register listener for device list changes (connect/disconnect)
            let status_devices = unsafe {
                AudioObjectAddPropertyListener(
                    kAudioObjectSystemObject,
                    &devices_address,
                    Some(device_change_callback),
                    client_data,
                )
            };

            if status_devices != 0 {
                error!(
                    "Failed to register device list property listener: OSStatus {}",
                    status_devices
                );
                unsafe {
                    let _ = Box::from_raw(client_data as *mut mpsc::Sender<()>);
                }
                return;
            }

            // Register listener for default output device changes
            let status_default = unsafe {
                AudioObjectAddPropertyListener(
                    kAudioObjectSystemObject,
                    &default_address,
                    Some(device_change_callback),
                    client_data,
                )
            };

            if status_default != 0 {
                // Non-fatal: device list changes still work
                error!(
                    "Failed to register default device property listener: OSStatus {}",
                    status_default
                );
            }

            debug!("Audio device property listeners registered");

            // Main loop: wait for notifications, debounce, then enumerate
            loop {
                match notify_rx.recv_timeout(Duration::from_millis(100)) {
                    Ok(()) => {
                        debug!("Device change notification received, debouncing");

                        // Debounce: wait for a quiet period before re-enumerating.
                        // Drain any further notifications that arrive within the window.
                        loop {
                            match notify_rx.recv_timeout(DEBOUNCE_DURATION) {
                                Ok(()) => {
                                    // Another notification arrived; restart the timer
                                    debug!("Additional device notification during debounce, resetting timer");
                                }
                                Err(mpsc::RecvTimeoutError::Timeout) => {
                                    // Quiet period elapsed; proceed to enumerate
                                    break;
                                }
                                Err(mpsc::RecvTimeoutError::Disconnected) => {
                                    error!(
                                        "Device notification channel disconnected during debounce"
                                    );
                                    return;
                                }
                            }
                        }

                        let socket_path = get_socket_path();
                        match query_mpv_devices(&socket_path) {
                            Ok(devices) => {
                                debug!("Device list updated: {} devices", devices.len());
                                on_change(devices);
                            }
                            Err(e) => {
                                debug!("Failed to query mpv devices (may be restarting): {}", e);
                            }
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        if shutdown_rx.try_recv().is_ok() {
                            debug!("Device watcher shutdown signal received");
                            break;
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => {
                        error!("Device change notification channel disconnected");
                        break;
                    }
                }
            }

            // Unregister both property listeners
            let status = unsafe {
                AudioObjectRemovePropertyListener(
                    kAudioObjectSystemObject,
                    &devices_address,
                    Some(device_change_callback),
                    client_data,
                )
            };
            if status != 0 {
                error!(
                    "Failed to unregister device list listener: OSStatus {}",
                    status
                );
            }

            if status_default == 0 {
                let status = unsafe {
                    AudioObjectRemovePropertyListener(
                        kAudioObjectSystemObject,
                        &default_address,
                        Some(device_change_callback),
                        client_data,
                    )
                };
                if status != 0 {
                    error!(
                        "Failed to unregister default device listener: OSStatus {}",
                        status
                    );
                }
            }

            debug!("Audio device property listeners unregistered");

            // Clean up the boxed sender
            unsafe {
                let _ = Box::from_raw(client_data as *mut mpsc::Sender<()>);
            }

            info!("Device watcher thread stopped");
        });

        Self {
            shutdown_tx,
            thread_handle: Some(thread_handle),
        }
    }

    /// Stop the device watcher.
    pub fn stop(&mut self) {
        debug!("Stopping device watcher");
        let _ = self.shutdown_tx.send(());

        if let Some(handle) = self.thread_handle.take() {
            if let Err(e) = handle.join() {
                error!("Device watcher thread panicked: {:?}", e);
            }
        }
    }
}

impl Drop for DeviceWatcher {
    fn drop(&mut self) {
        self.stop();
    }
}

/// CoreAudio property listener callback.
///
/// Called by CoreAudio when audio devices change or the default device changes.
/// Runs on a CoreAudio-managed thread, so we keep it minimal and just send a
/// notification through a channel. The watcher thread handles debouncing and
/// enumeration.
unsafe extern "C" fn device_change_callback(
    _object_id: AudioObjectID,
    _num_addresses: UInt32,
    _addresses: *const AudioObjectPropertyAddress,
    client_data: *mut c_void,
) -> OSStatus {
    if client_data.is_null() {
        return 0;
    }

    // Reconstruct the sender reference (don't take ownership)
    let notify_tx = &*(client_data as *const mpsc::Sender<()>);

    // Send notification - ignore errors (receiver might be gone during shutdown)
    let _ = notify_tx.send(());

    0 // noErr
}
