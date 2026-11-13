//! `#[tauri::command]` handlers. Keep them thin: validate input, call `disksieve-core`,
//! map the result to a DTO from [`crate::dto`].

use crate::dto::AppInfoDto;

/// Returns static information about the running application.
#[tauri::command]
pub fn app_info() -> AppInfoDto {
    AppInfoDto::current()
}
