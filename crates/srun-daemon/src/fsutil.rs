use std::path::Path;

use anyhow::{Context, Result};

pub fn replace_file(temporary: &Path, target: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::{
            MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
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
        let succeeded = unsafe {
            MoveFileExW(
                temporary_wide.as_ptr(),
                target_wide.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        };
        if succeeded == 0 {
            return Err(std::io::Error::last_os_error())
                .with_context(|| format!("无法提交文件 {}", target_path.display()));
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        std::fs::rename(temporary, target)?;
        Ok(())
    }
}
