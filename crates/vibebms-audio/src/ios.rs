//! iOS: the app's audio session, set before the output opens. cpal leaves
//! it alone, and the default one (`SoloAmbient`) goes silent with the
//! device's silent mode and keeps a long output buffer (about 20 ms): a
//! rhythm game's sound is the game. `Playback` plays in silent mode, and a
//! short buffer is asked for (the system may give another).

use objc2::msg_send;
use objc2::runtime::{AnyClass, AnyObject, Bool};
use objc2_foundation::NSString;

#[link(name = "AVFoundation", kind = "framework")]
extern "C" {}

/// Set the session up and make it active; `buffer_frames` (at 48 kHz) is
/// the buffer wanted, 0 for the default of this mixer (5 ms).
pub fn activate(buffer_frames: u32) {
    let Some(class) = AnyClass::get(c"AVAudioSession") else {
        log::warn!("audio session: no AVAudioSession");
        return;
    };
    let seconds = if buffer_frames > 0 {
        buffer_frames as f64 / 48_000.0
    } else {
        0.005
    };
    // SAFETY: AVAudioSession's documented class and instance methods, with
    // the argument and return types they are declared with.
    unsafe {
        let session: *mut AnyObject = msg_send![class, sharedInstance];
        if session.is_null() {
            log::warn!("audio session: none");
            return;
        }
        let mut err: *mut AnyObject = std::ptr::null_mut();
        let category = NSString::from_str("AVAudioSessionCategoryPlayback");
        let ok: Bool = msg_send![session, setCategory: &*category, error: &mut err];
        if !ok.as_bool() {
            log::warn!("audio session: the Playback category was refused");
        }
        let _: Bool = msg_send![session, setPreferredIOBufferDuration: seconds, error: &mut err];
        let ok: Bool = msg_send![session, setActive: Bool::YES, error: &mut err];
        if !ok.as_bool() {
            log::warn!("audio session: could not be made active");
        }
        let got: f64 = msg_send![session, IOBufferDuration];
        let rate: f64 = msg_send![session, sampleRate];
        log::info!("audio session: {rate} Hz, buffer {:.1} ms", got * 1000.0);
    }
}
