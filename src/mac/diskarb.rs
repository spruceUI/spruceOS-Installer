/// macOS DiskArbitration unmount helper
use std::ffi::CString;
use std::os::raw::{c_char, c_void};
use std::sync::atomic::{AtomicI32, Ordering};
use std::time::{Duration, Instant};

type CFAllocatorRef = *const c_void;
type CFRunLoopRef = *const c_void;
type CFStringRef = *const c_void;
type DASessionRef = *const c_void;
type DADiskRef = *const c_void;
type DADissenterRef = *const c_void;
type DAReturn = i32;
type DADiskUnmountOptions = u32;
type Boolean = u8;

const K_DA_RETURN_SUCCESS: DAReturn = 0;
const K_DA_UNMOUNT_OPTION_FORCE: DADiskUnmountOptions = 0x00080000;
const K_DA_UNMOUNT_OPTION_WHOLE: DADiskUnmountOptions = 0x00000001;
const RESULT_PENDING: DAReturn = i32::MIN;

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    static kCFAllocatorDefault: CFAllocatorRef;
    static kCFRunLoopDefaultMode: CFStringRef;

    fn CFRunLoopGetCurrent() -> CFRunLoopRef;
    fn CFRunLoopRunInMode(mode: CFStringRef, seconds: f64, return_after_source_handled: Boolean) -> i32;
    fn CFRunLoopStop(rl: CFRunLoopRef);
    fn CFRelease(cf: *const c_void);
}

#[link(name = "DiskArbitration", kind = "framework")]
extern "C" {
    fn DASessionCreate(allocator: CFAllocatorRef) -> DASessionRef;
    fn DASessionScheduleWithRunLoop(session: DASessionRef, runloop: CFRunLoopRef, mode: CFStringRef);
    fn DASessionUnscheduleFromRunLoop(session: DASessionRef, runloop: CFRunLoopRef, mode: CFStringRef);

    fn DADiskCreateFromBSDName(
        allocator: CFAllocatorRef,
        session: DASessionRef,
        name: *const c_char,
    ) -> DADiskRef;

    fn DADiskUnmount(
        disk: DADiskRef,
        options: DADiskUnmountOptions,
        callback: Option<extern "C" fn(DADiskRef, DADissenterRef, *mut c_void)>,
        context: *mut c_void,
    );

    fn DADissenterGetStatus(dissenter: DADissenterRef) -> DAReturn;
}

struct UnmountContext {
    result: AtomicI32,
    runloop: CFRunLoopRef,
}

extern "C" fn unmount_callback(_disk: DADiskRef, dissenter: DADissenterRef, context: *mut c_void) {
    if context.is_null() {
        return;
    }

    let ctx = unsafe { &*(context as *mut UnmountContext) };
    let status = if dissenter.is_null() {
        K_DA_RETURN_SUCCESS
    } else {
        unsafe { DADissenterGetStatus(dissenter) }
    };

    ctx.result.store(status, Ordering::Release);
    unsafe { CFRunLoopStop(ctx.runloop) };
}

pub fn unmount_disk(device_path: &str, timeout: Duration) -> Result<(), String> {
    let disk_name = device_path.trim_start_matches("/dev/");
    let disk_name_c = CString::new(disk_name)
        .map_err(|_| "Invalid device path for DiskArbitration".to_string())?;

    let session = unsafe { DASessionCreate(kCFAllocatorDefault) };
    if session.is_null() {
        return Err("DiskArbitration session creation failed".to_string());
    }

    let runloop = unsafe { CFRunLoopGetCurrent() };
    unsafe { DASessionScheduleWithRunLoop(session, runloop, kCFRunLoopDefaultMode) };

    let disk = unsafe { DADiskCreateFromBSDName(kCFAllocatorDefault, session, disk_name_c.as_ptr()) };
    if disk.is_null() {
        unsafe { DASessionUnscheduleFromRunLoop(session, runloop, kCFRunLoopDefaultMode) };
        unsafe { CFRelease(session as *const c_void) };
        return Err("DiskArbitration could not create disk reference".to_string());
    }

    let ctx = Box::new(UnmountContext {
        result: AtomicI32::new(RESULT_PENDING),
        runloop,
    });
    let ctx_ptr = Box::into_raw(ctx);

    unsafe {
        DADiskUnmount(
            disk,
            K_DA_UNMOUNT_OPTION_FORCE | K_DA_UNMOUNT_OPTION_WHOLE,
            Some(unmount_callback),
            ctx_ptr as *mut c_void,
        );
    }

    let start = Instant::now();
    let mut result = RESULT_PENDING;
    while start.elapsed() < timeout {
        let remaining = timeout.saturating_sub(start.elapsed()).as_secs_f64();
        let step = remaining.min(0.2);
        unsafe {
            CFRunLoopRunInMode(kCFRunLoopDefaultMode, step, 1);
        }

        result = unsafe { (*ctx_ptr).result.load(Ordering::Acquire) };
        if result != RESULT_PENDING {
            break;
        }
    }

    unsafe {
        DASessionUnscheduleFromRunLoop(session, runloop, kCFRunLoopDefaultMode);
        CFRelease(disk as *const c_void);
        CFRelease(session as *const c_void);
        drop(Box::from_raw(ctx_ptr));
    }

    if result == RESULT_PENDING {
        return Err("DiskArbitration unmount timed out".to_string());
    }

    if result == K_DA_RETURN_SUCCESS {
        Ok(())
    } else {
        Err(format!("DiskArbitration unmount failed: 0x{:08X}", result as u32))
    }
}
