//! Which desktop session Find runs in. The overlay, the global shortcut and window placement each depend on it, so
//! they ask here instead of reading the environment and running `hyprctl` themselves.

/// A Wayland session (Linux), where placement is the compositor's and global keys need the portal or Hyprland.
pub fn wayland() -> bool {
    cfg!(target_os = "linux") && std::env::var_os("WAYLAND_DISPLAY").is_some()
}

/// Hyprland is the compositor: its `hyprctl` places windows and binds keys.
pub fn hyprland() -> bool {
    cfg!(target_os = "linux") && std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some()
}

/// The JSON answer to `hyprctl -j <query>`, or `None` when Hyprland isn't running or didn't answer.
pub fn hyprctl_json(query: &str) -> Option<serde_json::Value> {
    if !hyprland() {
        return None;
    }
    let output = std::process::Command::new("hyprctl").args(["-j", query]).output().ok()?;
    output.status.success().then(|| serde_json::from_slice(&output.stdout).ok()).flatten()
}
