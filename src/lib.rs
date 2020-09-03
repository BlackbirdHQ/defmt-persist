//! `defmt` global logger saving to storage

#![no_std]

use core::{
    ptr::NonNull,
    sync::atomic::{AtomicBool, Ordering, AtomicUsize},
};

use cortex_m::{interrupt, register};
use embedded_hal::storage::*;

// TODO make configurable
// NOTE use a power of 2 for best performance
const SIZE: usize = 1024;

#[defmt::global_logger]
struct Logger;

impl defmt::Write for Logger {
    fn write(&mut self, bytes: &[u8]) {
        unsafe { handle().write_all(bytes) }
    }
}

static TAKEN: AtomicBool = AtomicBool::new(false);
static INTERRUPTS_ACTIVE: AtomicBool = AtomicBool::new(false);

unsafe impl defmt::Logger for Logger {
    fn acquire() -> Option<NonNull<dyn defmt::Write>> {
        let primask = register::primask::read();
        interrupt::disable();
        if !TAKEN.load(Ordering::Relaxed) {
            // no need for CAS because interrupts are disabled
            TAKEN.store(true, Ordering::Relaxed);

            INTERRUPTS_ACTIVE.store(primask.is_active(), Ordering::Relaxed);

            Some(NonNull::from(&Logger as &dyn defmt::Write))
        } else {
            if primask.is_active() {
                // re-enable interrupts
                unsafe { interrupt::enable() }
            }
            None
        }
    }

    unsafe fn release(_: NonNull<dyn defmt::Write>) {
        TAKEN.store(false, Ordering::Relaxed);
        if INTERRUPTS_ACTIVE.load(Ordering::Relaxed) {
            // re-enable interrupts
            interrupt::enable()
        }
    }
}


// make sure we only get shared references to the header/channel (avoid UB)
/// # Safety
/// `Channel` API is not re-entrant; this handle should not be held from different execution
/// contexts (e.g. thread-mode, interrupt context)
unsafe fn handle() -> &'static Channel {
    // NOTE the `rtt-target` API is too permissive. It allows writing arbitrary data to any
    // channel (`set_print_channel` + `rprint*`) and that can corrupt defmt log frames.
    // So we declare the RTT control block here and make it impossible to use `rtt-target` together
    // with this crate.
    #[no_mangle]
    static mut _SEGGER_RTT: Header = Header {
        id: *b"SEGGER RTT\0\0\0\0\0\0",
        max_up_channels: 1,
        max_down_channels: 0,
        up_channel: Channel {
            name: NAME as *const _ as *const u8,
            buffer: unsafe { &mut BUFFER as *mut _ as *mut u8 },
            size: SIZE,
            write: AtomicUsize::new(0),
            read: AtomicUsize::new(0),
            flags: 0b10, // mode = block-if-full
        },
    };

    #[link_section = ".uninit.defmt-rtt.BUFFER"]
    static mut BUFFER: [u8; SIZE] = [0; SIZE];

    static NAME: &[u8] = b"defmt\0";

    &_SEGGER_RTT.up_channel
}
