//! macOS Now Playing / Media Remote integration.
//!
//! Registers with `MPRemoteCommandCenter` so that macOS delivers
//! next-track and previous-track media key events to Bragi.
//!
//! On macOS, `NX_KEYTYPE::Next` (17) and `NX_KEYTYPE::Previous` (18) are
//! only delivered via the Now Playing system, not via CGEventTap. This
//! module registers command handlers so macOS routes those keys here.
//!
//! Play/pause continues to work via the global-shortcut plugin's
//! CGEventTap because macOS treats that key specially.

use std::sync::Arc;
use tokio::sync::RwLock;

use crate::audio::{AudioCommand, AudioEngineHandle, RepeatMode};

/// Opaque handle that keeps the media key registration alive.
///
/// The Objective-C block pointers are prevented from being deallocated
/// by storing them in this struct. Drop it to unregister.
pub struct MediaKeyHandle {
    _blocks: Vec<objc2::rc::Retained<objc2::runtime::AnyObject>>,
}

// SAFETY: The retained Obj-C objects are opaque tokens returned by
// `addTargetWithHandler:`. They are reference-counted and only held
// here to prevent deallocation — we never mutate or read them after
// creation. The Obj-C runtime's reference counting is thread-safe.
unsafe impl Send for MediaKeyHandle {}
unsafe impl Sync for MediaKeyHandle {}

/// Register next-track, previous-track, and toggle-play-pause handlers
/// with macOS `MPRemoteCommandCenter`.
///
/// Must be called from the main thread (or at least before the run loop
/// spins — Tauri's setup closure satisfies this).
pub fn register_media_commands(
    audio: Arc<RwLock<Option<AudioEngineHandle>>>,
) -> Result<MediaKeyHandle, String> {
    use block2::RcBlock;
    use objc2::msg_send;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject, Bool};

    unsafe {
        // MPRemoteCommandCenter.sharedCommandCenter()
        let cls = AnyClass::get(c"MPRemoteCommandCenter")
            .ok_or("MPRemoteCommandCenter class not found — MediaPlayer framework not linked")?;
        let center: *mut AnyObject = msg_send![cls, sharedCommandCenter];
        if center.is_null() {
            return Err("sharedCommandCenter returned nil".into());
        }

        let mut blocks: Vec<Retained<AnyObject>> = Vec::new();

        // Helper: given a command object, enable it and attach a handler block.
        let mut attach_handler = |cmd_obj: *mut AnyObject,
                                  label: &str,
                                  audio: Arc<RwLock<Option<AudioEngineHandle>>>,
                                  command: AudioCommand|
         -> Result<(), String> {
            if cmd_obj.is_null() {
                return Err(format!("{} returned nil", label));
            }

            // Enable the command
            let _: () = msg_send![cmd_obj, setEnabled: Bool::YES];

            // Create handler block.
            // MPRemoteCommandHandlerStatus = 0 means success.
            let block = RcBlock::new(move |_event: *mut AnyObject| -> isize {
                tracing::debug!("MPRemoteCommand handler fired: {:?}", command);
                if let Ok(guard) = audio.try_read() {
                    if let Some(handle) = guard.as_ref() {
                        let _ = handle.send(command.clone());
                    }
                }
                0 // MPRemoteCommandHandlerStatusSuccess
            });

            // addTargetWithHandler: returns an opaque token we must retain
            let token: *mut AnyObject = msg_send![cmd_obj, addTargetWithHandler: &*block];
            if !token.is_null() {
                blocks.push(
                    Retained::retain(token).expect("Objective-C object should be retainable"),
                );
            }

            Ok(())
        };

        // Get command objects directly via property getter selectors
        let next_cmd: *mut AnyObject = msg_send![center, nextTrackCommand];
        let prev_cmd: *mut AnyObject = msg_send![center, previousTrackCommand];
        let toggle_cmd: *mut AnyObject = msg_send![center, togglePlayPauseCommand];

        attach_handler(
            next_cmd,
            "nextTrackCommand",
            audio.clone(),
            AudioCommand::SkipNext,
        )?;
        attach_handler(
            prev_cmd,
            "previousTrackCommand",
            audio.clone(),
            AudioCommand::SkipPrevious,
        )?;
        attach_handler(
            toggle_cmd,
            "togglePlayPauseCommand",
            audio.clone(),
            AudioCommand::TogglePlayPause,
        )?;

        // Distinct play/pause commands (Control Center shows both, and some
        // accessories send these rather than toggle).
        let play_cmd: *mut AnyObject = msg_send![center, playCommand];
        let pause_cmd: *mut AnyObject = msg_send![center, pauseCommand];
        attach_handler(play_cmd, "playCommand", audio.clone(), AudioCommand::Resume)?;
        attach_handler(
            pause_cmd,
            "pauseCommand",
            audio.clone(),
            AudioCommand::Pause,
        )?;

        // Event-carrying commands need bespoke blocks (attach_handler binds a
        // FIXED command; these read the requested value off the event).
        //
        // Helper closure mirroring attach_handler's enable + token-retention, but
        // taking a ready-made handler block.
        let mut attach_block = |cmd_obj: *mut AnyObject,
                                label: &str,
                                block: RcBlock<dyn Fn(*mut AnyObject) -> isize>|
         -> Result<(), String> {
            if cmd_obj.is_null() {
                return Err(format!("{} returned nil", label));
            }
            let _: () = msg_send![cmd_obj, setEnabled: Bool::YES];
            let token: *mut AnyObject = msg_send![cmd_obj, addTargetWithHandler: &*block];
            if !token.is_null() {
                blocks.push(
                    Retained::retain(token).expect("Objective-C object should be retainable"),
                );
            }
            Ok(())
        };

        // Scrub: MPChangePlaybackPositionCommandEvent.positionTime is seconds.
        let seek_cmd: *mut AnyObject = msg_send![center, changePlaybackPositionCommand];
        let audio_seek = audio.clone();
        let seek_block = RcBlock::new(move |event: *mut AnyObject| -> isize {
            let secs: f64 = if event.is_null() {
                0.0
            } else {
                msg_send![event, positionTime]
            };
            let ms = (secs.max(0.0) * 1000.0) as u64;
            if let Ok(guard) = audio_seek.try_read() {
                if let Some(handle) = guard.as_ref() {
                    let _ = handle.send(AudioCommand::Seek(ms));
                }
            }
            0
        });
        attach_block(seek_cmd, "changePlaybackPositionCommand", seek_block)?;

        // Shuffle: MPChangeShuffleModeCommandEvent.shuffleType (0=off,1=items,2=collections).
        let shuffle_cmd: *mut AnyObject = msg_send![center, changeShuffleModeCommand];
        let audio_shuffle = audio.clone();
        let shuffle_block = RcBlock::new(move |event: *mut AnyObject| -> isize {
            let ty: isize = if event.is_null() {
                0
            } else {
                msg_send![event, shuffleType]
            };
            if let Ok(guard) = audio_shuffle.try_read() {
                if let Some(handle) = guard.as_ref() {
                    let _ = handle.send(AudioCommand::SetShuffle(ty != 0));
                }
            }
            0
        });
        attach_block(shuffle_cmd, "changeShuffleModeCommand", shuffle_block)?;

        // Repeat: MPChangeRepeatModeCommandEvent.repeatType (0=off,1=one,2=all).
        let repeat_cmd: *mut AnyObject = msg_send![center, changeRepeatModeCommand];
        let audio_repeat = audio;
        let repeat_block = RcBlock::new(move |event: *mut AnyObject| -> isize {
            let ty: isize = if event.is_null() {
                0
            } else {
                msg_send![event, repeatType]
            };
            let mode = match ty {
                1 => RepeatMode::One,
                2 => RepeatMode::All,
                _ => RepeatMode::Off,
            };
            if let Ok(guard) = audio_repeat.try_read() {
                if let Some(handle) = guard.as_ref() {
                    let _ = handle.send(AudioCommand::SetRepeat(mode));
                }
            }
            0
        });
        attach_block(repeat_cmd, "changeRepeatModeCommand", repeat_block)?;

        // Set minimal Now Playing info so macOS recognises us as a media app.
        // Without this, the command center may not route events to us.
        let info_cls = AnyClass::get(c"MPNowPlayingInfoCenter")
            .ok_or("MPNowPlayingInfoCenter class not found")?;
        let info_center: *mut AnyObject = msg_send![info_cls, defaultCenter];
        if !info_center.is_null() {
            let dict_cls =
                AnyClass::get(c"NSMutableDictionary").ok_or("NSMutableDictionary not found")?;
            let empty_dict: *mut AnyObject = msg_send![dict_cls, dictionary];
            let _: () = msg_send![info_center, setNowPlayingInfo: empty_dict];

            // On macOS, playbackState MUST be set for remote commands to work.
            // Apple docs: "You must set this property every time the app begins
            // or halts playback, otherwise remote control functionality may not
            // work as expected."
            // MPNowPlayingPlaybackState: playing = 1
            let _: () = msg_send![info_center, setPlaybackState: 1usize];
        }

        tracing::info!("macOS MPRemoteCommandCenter handlers registered");
        Ok(MediaKeyHandle { _blocks: blocks })
    }
}

// MediaPlayer.framework `nowPlayingInfo` dictionary key constants. `build.rs`
// links MediaPlayer, so these exported `NSString*` constants resolve at link
// time — we use the REAL constants rather than guessing raw key strings, which
// is what makes the system read our metadata correctly (a wrong key silently
// shows nothing and is only observable on a Mac).
extern "C" {
    static MPMediaItemPropertyTitle: &'static objc2_foundation::NSString;
    static MPMediaItemPropertyArtist: &'static objc2_foundation::NSString;
    static MPMediaItemPropertyAlbumTitle: &'static objc2_foundation::NSString;
    static MPMediaItemPropertyPlaybackDuration: &'static objc2_foundation::NSString;
    static MPMediaItemPropertyArtwork: &'static objc2_foundation::NSString;
    static MPNowPlayingInfoPropertyElapsedPlaybackTime: &'static objc2_foundation::NSString;
    static MPNowPlayingInfoPropertyPlaybackRate: &'static objc2_foundation::NSString;
    static MPNowPlayingInfoPropertyMediaType: &'static objc2_foundation::NSString;
}

/// Update the macOS Now Playing surface (Control Center / menu-bar widget / lock
/// screen / AirPods) with the current track's metadata, artwork, and playback
/// position.
///
/// Call on every `AudioEvent::StateChanged` (track change / play / pause / seek):
/// per Apple, setting elapsed time + rate at each state change lets the OS
/// interpolate the scrubber, so per-second updates aren't needed. Pass
/// `track: None` to clear the info when playback stops.
///
/// `artwork_path` is the caller-resolved on-disk cached image
/// (`artwork::cached_artwork_path`); `None` omits artwork (the widget falls back
/// to the app icon) and it appears on the next update once the cache is warm.
pub fn update_now_playing(
    track: Option<&crate::models::Track>,
    is_playing: bool,
    elapsed_ms: u64,
    duration_ms: u64,
    artwork_path: Option<&std::path::Path>,
) {
    use block2::RcBlock;
    use objc2::msg_send;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject};
    use objc2_foundation::NSString;

    unsafe {
        let Some(info_cls) = AnyClass::get(c"MPNowPlayingInfoCenter") else {
            return;
        };
        let info_center: *mut AnyObject = msg_send![info_cls, defaultCenter];
        if info_center.is_null() {
            return;
        }

        // No track → clear the now-playing info and mark stopped (state 3).
        let Some(track) = track else {
            let empty: *mut AnyObject = std::ptr::null_mut();
            let _: () = msg_send![info_center, setNowPlayingInfo: empty];
            let _: () = msg_send![info_center, setPlaybackState: 3usize];
            return;
        };

        let Some(dict_cls) = AnyClass::get(c"NSMutableDictionary") else {
            return;
        };
        let dict: *mut AnyObject = msg_send![dict_cls, dictionary];
        if dict.is_null() {
            return;
        }

        // String metadata.
        let set_str = |key: &NSString, value: &str| {
            let ns = NSString::from_str(value);
            let _: () = msg_send![dict, setObject: &*ns, forKey: key];
        };
        set_str(MPMediaItemPropertyTitle, &track.name);
        set_str(MPMediaItemPropertyArtist, &track.artist_name);
        set_str(MPMediaItemPropertyAlbumTitle, &track.album_name);

        // Numeric metadata (seconds / rate) boxed as NSNumber.
        if let Some(num_cls) = AnyClass::get(c"NSNumber") {
            let set_num = |key: &NSString, value: f64| {
                let n: *mut AnyObject = msg_send![num_cls, numberWithDouble: value];
                let _: () = msg_send![dict, setObject: n, forKey: key];
            };
            set_num(
                MPMediaItemPropertyPlaybackDuration,
                duration_ms as f64 / 1000.0,
            );
            set_num(
                MPNowPlayingInfoPropertyElapsedPlaybackTime,
                elapsed_ms as f64 / 1000.0,
            );
            set_num(
                MPNowPlayingInfoPropertyPlaybackRate,
                if is_playing { 1.0 } else { 0.0 },
            );
            // MPNowPlayingInfoMediaType.audio == 1
            let media_type: *mut AnyObject = msg_send![num_cls, numberWithUnsignedLongLong: 1u64];
            let _: () =
                msg_send![dict, setObject: media_type, forKey: MPNowPlayingInfoPropertyMediaType];
        }

        // Album artwork from the on-disk cache.
        //
        // VERIFY ON MAC: this (the NSImage load + MPMediaItemArtwork
        // requestHandler block + CGSize) is the part most likely to need
        // adjustment. It's fully guarded so a failure only omits artwork, never
        // crashes. `Retained::from_raw` takes ownership of the NSImage so it's
        // freed with the artwork block (no per-update leak).
        if let Some(path) = artwork_path.and_then(|p| p.to_str()) {
            if let Some(img_cls) = AnyClass::get(c"NSImage") {
                let alloc: *mut AnyObject = msg_send![img_cls, alloc];
                let ns_path = NSString::from_str(path);
                let image: *mut AnyObject = msg_send![alloc, initWithContentsOfFile: &*ns_path];
                if let Some(image) = Retained::from_raw(image) {
                    if let Some(art_cls) = AnyClass::get(c"MPMediaItemArtwork") {
                        let size: objc2_foundation::NSSize = msg_send![&*image, size];
                        let handler =
                            RcBlock::new(move |_req: objc2_foundation::NSSize| -> *mut AnyObject {
                                // Same image for any requested size; the moved
                                // Retained keeps it alive for the block's lifetime.
                                Retained::as_ptr(&image) as *mut AnyObject
                            });
                        let art_alloc: *mut AnyObject = msg_send![art_cls, alloc];
                        let artwork: *mut AnyObject = msg_send![art_alloc, initWithBoundsSize: size, requestHandler: &*handler];
                        if !artwork.is_null() {
                            let _: () = msg_send![dict, setObject: artwork, forKey: MPMediaItemPropertyArtwork];
                        }
                    }
                }
            }
        }

        let _: () = msg_send![info_center, setNowPlayingInfo: dict];
        // MPNowPlayingPlaybackState: playing = 1, paused = 2.
        let _: () =
            msg_send![info_center, setPlaybackState: if is_playing { 1usize } else { 2usize }];
    }
}
