use std::path::Path;

use anyhow::Result;

pub fn replace_file(temporary: &Path, target: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::{
            MoveFileExW, ReplaceFileW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
            REPLACEFILE_IGNORE_MERGE_ERRORS, REPLACEFILE_WRITE_THROUGH,
        };

        let target_path = target.to_path_buf();
        let temporary_wide: Vec<u16> = temporary
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let target_wide: Vec<u16> = target
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let mut replace_error = None;
        if target.exists() {
            let succeeded = unsafe {
                ReplaceFileW(
                    target_wide.as_ptr(),
                    temporary_wide.as_ptr(),
                    std::ptr::null(),
                    REPLACEFILE_WRITE_THROUGH | REPLACEFILE_IGNORE_MERGE_ERRORS,
                    std::ptr::null(),
                    std::ptr::null(),
                )
            };
            if succeeded != 0 {
                return Ok(());
            }
            replace_error = Some(std::io::Error::last_os_error());
        }
        let succeeded = unsafe {
            MoveFileExW(
                temporary_wide.as_ptr(),
                target_wide.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        };
        if succeeded == 0 {
            let move_error = std::io::Error::last_os_error();
            if target.exists() && std::fs::copy(temporary, target).is_ok() {
                let _ = std::fs::remove_file(temporary);
                return Ok(());
            }
            let replace_detail = replace_error
                .map(|error| format!("；ReplaceFile: {error}"))
                .unwrap_or_default();
            return Err(anyhow::anyhow!(
                "无法提交文件 {}：{move_error}{replace_detail}",
                target_path.display()
            ));
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        std::fs::rename(temporary, target)?;
        Ok(())
    }
}
