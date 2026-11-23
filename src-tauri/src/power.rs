// Being a polite background process.
//
// While nothing is happening, neptune asks Windows to treat it as background
// work: efficiency mode (EcoQoS: lower clocks, efficiency cores) and low
// memory priority (its pages are first to go under pressure). The moment a
// dictation starts, or the model has work, it asks for full speed again.
//
// Deliberately not used: PROCESS_MODE_BACKGROUND_BEGIN, which caps the
// working set at about 32 MB and makes memory access hundreds of times
// slower, and working-set trimming, which only moves pages to standby and
// faults them straight back.

use std::sync::atomic::{AtomicI32, Ordering};

/// Things in flight: a take, a model job. Zero means idle.
static BUSY: AtomicI32 = AtomicI32::new(0);

/// Something started. Leaves efficiency mode if this is the first.
pub fn busy() {
    if BUSY.fetch_add(1, Ordering::SeqCst) == 0 {
        set_background(false);
    }
}

/// Something finished. Enters efficiency mode if nothing else is running.
pub fn idle() {
    let before = BUSY.fetch_sub(1, Ordering::SeqCst);
    if before <= 1 {
        BUSY.store(0, Ordering::SeqCst);
        set_background(true);
    }
}

/// Called once at startup: neptune starts idle.
pub fn init() {
    set_background(true);
}

#[cfg(windows)]
fn set_background(on: bool) {
    use windows::Win32::System::Threading::{
        GetCurrentProcess, ProcessMemoryPriority, ProcessPowerThrottling, SetProcessInformation,
        MEMORY_PRIORITY_INFORMATION, MEMORY_PRIORITY_LOW, MEMORY_PRIORITY_NORMAL, PROCESS_POWER_THROTTLING_CURRENT_VERSION,
        PROCESS_POWER_THROTTLING_EXECUTION_SPEED, PROCESS_POWER_THROTTLING_STATE,
    };
    unsafe {
        let p = GetCurrentProcess();
        let throttle = PROCESS_POWER_THROTTLING_STATE {
            Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
            ControlMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
            StateMask: if on { PROCESS_POWER_THROTTLING_EXECUTION_SPEED } else { 0 },
        };
        let _ = SetProcessInformation(p, ProcessPowerThrottling, &throttle as *const _ as *const _, std::mem::size_of_val(&throttle) as u32);
        let prio = MEMORY_PRIORITY_INFORMATION { MemoryPriority: if on { MEMORY_PRIORITY_LOW } else { MEMORY_PRIORITY_NORMAL } };
        let _ = SetProcessInformation(p, ProcessMemoryPriority, &prio as *const _ as *const _, std::mem::size_of_val(&prio) as u32);
    }
}

#[cfg(not(windows))]
fn set_background(_on: bool) {}
