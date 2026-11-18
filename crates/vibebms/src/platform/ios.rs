//! iOS input at the moments it happened.
//!
//! UIKit hands touches over once a display refresh, so a touch stamped
//! when winit passes it on is on the frame grid: on a chart whose notes are
//! on it too (BPM 200 at 120 Hz) every offset is a whole number of frames
//! off the others. Each touch carries its own time (`UITouch.timestamp`),
//! which winit drops: winit's view's touch methods are wrapped here to note
//! it, by touch, before winit sends the touch on ([`touch_time`]).
//!
//! winit has no hardware keyboard keys on iOS (only the text of a text
//! field's). The GameController framework's keyboard has them, on a queue
//! of their own as they come ([`poll_keyboard`], [`take_keys`]).

use crate::input::hid;
use crate::platform::{event_instant, Instant};
use block2::RcBlock;
use dispatch2::{DispatchQoS, DispatchQueue, DispatchQueueAttr, GlobalQueueIdentifier};
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Bool, Imp, Sel};
use objc2::sel;
use objc2_foundation::NSSet;
use objc2_game_controller::{
    GCControllerButtonInput, GCDevice, GCKeyCode, GCKeyboard, GCKeyboardInput,
};
use objc2_ui_kit::{UIEvent, UITouch, UITouchPhase};
use std::cell::RefCell;
use std::ptr::NonNull;
use std::sync::{Mutex, OnceLock};
use winit::event::TouchPhase;
use winit::keyboard::KeyCode;

#[link(name = "QuartzCore", kind = "framework")]
extern "C" {
    /// The clock of UIKit's event times (seconds since boot).
    fn CACurrentMediaTime() -> f64;
}

/// When an event of that clock's time `at` happened (now, when `at` is not
/// a moment of it lately: GameController's times are taken to be of it).
fn since_boot(at: f64) -> Instant {
    let now = Instant::now();
    event_instant(now, unsafe { CACurrentMediaTime() } - at)
}

type TouchesImp = unsafe extern "C-unwind" fn(&AnyObject, Sel, &NSSet<UITouch>, Option<&UIEvent>);

thread_local! {
    /// winit's touch methods, by selector.
    static WINIT_TOUCHES: RefCell<Vec<(Sel, TouchesImp)>> = const { RefCell::new(Vec::new()) };
    /// The touches handed over and not yet sent on by winit: winit's id
    /// (the `UITouch`'s address), phase, when.
    static STAMPS: RefCell<Vec<(u64, TouchPhase, Instant)>> = const { RefCell::new(Vec::new()) };
    static KEYBOARD: RefCell<Option<Retained<GCKeyboard>>> = const { RefCell::new(None) };
}

/// Hardware keyboard keys come in (from the keyboard's queue): wakes the
/// event loop to take them.
static WAKE: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();
/// Keys come in and not taken yet: key, pressed, when.
static KEYS: Mutex<Vec<(KeyCode, bool, Instant)>> = Mutex::new(Vec::new());

/// Note touches' times from now on (the window made: its view's class is
/// winit's), and keys from a hardware keyboard, with `wake` called when
/// some come.
pub fn install(wake: impl Fn() + Send + Sync + 'static) {
    // (once: winit's methods are replaced by ours)
    if WAKE.set(Box::new(wake)).is_err() {
        return;
    }
    let Some(view) = AnyClass::get(c"WinitUIView") else {
        log::warn!("touch times: no winit view");
        return;
    };
    for s in [
        sel!(touchesBegan:withEvent:),
        sel!(touchesMoved:withEvent:),
        sel!(touchesEnded:withEvent:),
        sel!(touchesCancelled:withEvent:),
    ] {
        let Some(m) = view.instance_method(s) else {
            log::warn!("touch times: winit's view has no {s}");
            continue;
        };
        // SAFETY: `touches` has the method's signature (that of UIResponder)
        let winit = unsafe {
            let ours: TouchesImp = touches;
            m.set_implementation(std::mem::transmute::<TouchesImp, Imp>(ours))
        };
        WINIT_TOUCHES.with_borrow_mut(|w| {
            // SAFETY: winit's method, of this signature
            w.push((s, unsafe { std::mem::transmute::<Imp, TouchesImp>(winit) }))
        });
    }
}

unsafe extern "C-unwind" fn touches(
    view: &AnyObject,
    cmd: Sel,
    set: &NSSet<UITouch>,
    event: Option<&UIEvent>,
) {
    for t in set.iter() {
        let phase = match t.phase() {
            UITouchPhase::Began => TouchPhase::Started,
            UITouchPhase::Moved => TouchPhase::Moved,
            UITouchPhase::Ended => TouchPhase::Ended,
            UITouchPhase::Cancelled => TouchPhase::Cancelled,
            _ => continue,
        };
        // a touch is its latest sample; one that began, its first since the
        // last hand-over (the finger down)
        let first = match phase {
            TouchPhase::Started => event
                .and_then(|e| e.coalescedTouchesForTouch(&t))
                .and_then(|c| c.iter().map(|c| c.timestamp()).reduce(f64::min)),
            _ => None,
        };
        let time = since_boot(first.unwrap_or_else(|| t.timestamp()));
        let id = Retained::as_ptr(&t) as u64;
        STAMPS.with_borrow_mut(|s| {
            s.retain(|e| e.0 != id);
            // (touches winit never sent on, e.g. while there was no app)
            if s.len() >= 32 {
                s.remove(0);
            }
            s.push((id, phase, time));
        });
    }
    let winit = WINIT_TOUCHES.with_borrow(|w| w.iter().find(|e| e.0 == cmd).map(|e| e.1));
    if let Some(winit) = winit {
        // SAFETY: the method replaced, with its own arguments
        unsafe { winit(view, cmd, set, event) }
    }
}

/// When winit's touch `id` (`Touch::id`) in `phase` happened.
pub fn touch_time(id: u64, phase: TouchPhase) -> Option<Instant> {
    STAMPS.with_borrow_mut(|s| {
        let i = s.iter().position(|e| e.0 == id && e.1 == phase)?;
        Some(s.swap_remove(i).2)
    })
}

/// A keyboard connected since the last call: its keys go to [`take_keys`].
/// (GameController has a keyboard object, all keyboards in one, while one
/// is connected; it tells keys on the queue set here, as they come, not
/// once a frame.)
pub fn poll_keyboard() {
    // SAFETY: a class method without arguments
    let kb = unsafe { GCKeyboard::coalescedKeyboard() };
    KEYBOARD.with_borrow_mut(|cur| {
        if cur.as_ref().map(Retained::as_ptr) == kb.as_ref().map(Retained::as_ptr) {
            return;
        }
        if let Some((k, input)) = kb
            .as_ref()
            .and_then(|k| Some((k, unsafe { k.keyboardInput() }?)))
        {
            let queue = DispatchQueue::new_with_target(
                "vibebms.keyboard",
                DispatchQueueAttr::SERIAL,
                Some(&DispatchQueue::global_queue(
                    GlobalQueueIdentifier::QualityOfService(DispatchQoS::UserInteractive),
                )),
            );
            let handler = RcBlock::new(
                |input: NonNull<GCKeyboardInput>,
                 _: NonNull<GCControllerButtonInput>,
                 code: GCKeyCode,
                 pressed: Bool| {
                    let Some(key) = hid::key_code(code) else {
                        return;
                    };
                    // SAFETY: GameController passes its keyboard input
                    let at = unsafe { input.as_ref().lastEventTimestamp() };
                    let time = since_boot(at);
                    if let Ok(mut keys) = KEYS.lock() {
                        keys.push((key, pressed.as_bool(), time));
                    }
                    if let Some(wake) = WAKE.get() {
                        wake();
                    }
                },
            );
            // SAFETY: the handler has the type GameController calls (it
            // copies it), and is called on that queue only
            unsafe {
                k.setHandlerQueue(&queue);
                input.setKeyChangedHandler(RcBlock::as_ptr(&handler));
            }
            log::info!("hardware keyboard connected");
        }
        *cur = kb;
    });
}

/// The hardware keyboard's keys since the last call: key, pressed, when.
pub fn take_keys() -> Vec<(KeyCode, bool, Instant)> {
    KEYS.lock()
        .map(|mut k| std::mem::take(&mut *k))
        .unwrap_or_default()
}
