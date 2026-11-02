use std::ffi::c_void;

use anyhow::{Context, Result};
use tokio::sync::mpsc::UnboundedSender;
use windows_sys::Win32::{
    Foundation::NO_ERROR,
    NetworkManagement::IpHelper::{
        NotifyIpInterfaceChange, MIB_IPINTERFACE_ROW, MIB_NOTIFICATION_TYPE,
    },
};

pub fn start(events: UnboundedSender<()>) -> Result<()> {
    let context = Box::into_raw(Box::new(events)).cast::<c_void>();
    let mut notification_handle = std::ptr::null_mut();
    let status = unsafe {
        NotifyIpInterfaceChange(
            0,
            Some(interface_changed),
            context,
            false,
            &mut notification_handle,
        )
    };
    if status != NO_ERROR {
        unsafe {
            drop(Box::from_raw(context.cast::<UnboundedSender<()>>()));
        }
        return Err(std::io::Error::from_raw_os_error(status as i32))
            .context("无法监听 Windows 网络接口变化");
    }
    Ok(())
}

unsafe extern "system" fn interface_changed(
    context: *const c_void,
    _row: *const MIB_IPINTERFACE_ROW,
    _notification_type: MIB_NOTIFICATION_TYPE,
) {
    if context.is_null() {
        return;
    }
    let sender = unsafe { &*context.cast::<UnboundedSender<()>>() };
    let _ = sender.send(());
}
