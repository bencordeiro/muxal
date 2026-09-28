//! Reaping leftover AppImage squashfuse mounts.
//!
//! A type-2 AppImage mounts its squashfs at `$TMPDIR/.mount_<name>-XXXXXX` via
//! squashfuse and unmounts it when the process exits. If muxal is SIGKILLed or
//! crashes, that unmount never runs and the mount is orphaned; once the
//! squashfuse daemon later dies, the mount goes stale (`statfs` returns
//! `ENOTCONN`, "Transport endpoint is not connected"). Anything that then
//! enumerates filesystems — `df`, which some desktop system monitors run every
//! ~60s — stalls in the kernel FUSE layer on the dead mount, which on a Wayland
//! compositor surfaces as a periodic cursor stutter that worsens as more
//! leftovers accumulate across days of uptime.
//!
//! muxal can't catch SIGKILL, so it reaps these on the next launch. This module
//! is the pure selection half — which mounts are muxal's and not our own; the
//! app crate does the liveness probe and the actual lazy-unmount.

/// The `.AppImage` file this process is running from: `$APPIMAGE`, but only when
/// the executable (`exe`) really lives inside that AppImage's mount (`$APPDIR`).
///
/// The AppImage runtime exports both variables to everything the app starts, so
/// they also arrive in programs that merely descend from *another* AppImage — a
/// shell in an AppImage terminal emulator, a command an Electron AppImage (such as
/// an agent app) runs. A muxal installed from a .deb or .rpm and started that way
/// must not take the other app's AppImage for its own: the updater would overwrite
/// it, and `muxal ctl` would tell agents to run it.
pub fn own_appimage(
    appimage: Option<&str>,
    appdir: Option<&str>,
    exe: &std::path::Path,
) -> Option<std::path::PathBuf> {
    let appimage = appimage.filter(|a| !a.is_empty())?;
    let appdir = appdir.filter(|d| !d.is_empty())?;
    exe.starts_with(appdir)
        .then(|| std::path::PathBuf::from(appimage))
}

/// Given the contents of `/proc/self/mounts` and this process's own AppImage
/// mount directory (`$APPDIR`; `None` when muxal wasn't launched from an
/// AppImage), return the mountpoints of *other* muxal AppImage squashfuse mounts
/// — leftovers from prior instances. Our own mount is never included.
///
/// The caller probes each returned mount for liveness and lazy-unmounts only the
/// dead ones: a live mount still belongs to another running muxal instance and
/// must be left alone.
pub fn foreign_muxal_appimage_mounts(mounts: &str, self_appdir: Option<&str>) -> Vec<String> {
    mounts
        .lines()
        .filter_map(parse_muxal_mount)
        .filter(|mp| self_appdir != Some(mp.as_str()))
        .collect()
}

/// Parse one `/proc/self/mounts` line, returning the (unescaped) mountpoint iff
/// it is a muxal AppImage squashfuse mount. Lines look like:
///
/// ```text
/// muxal-linux-x86_64.AppImage /tmp/.mount_muxal-CDigJK fuse.muxal-…AppImage ro,… 0 0
/// ```
///
/// Three signals must all hold, so this never matches another app's FUSE mount
/// (`gvfsd-fuse`, `portal`, another AppImage): the fstype is FUSE, the source or
/// fstype names muxal, and the mountpoint is an AppImage mount dir (`.mount_…`).
fn parse_muxal_mount(line: &str) -> Option<String> {
    let mut fields = line.split(' ');
    let source = fields.next()?;
    let mountpoint = fields.next()?;
    let fstype = fields.next()?;

    let is_fuse = fstype == "fuse" || fstype.starts_with("fuse.");
    let names_muxal = fstype.to_ascii_lowercase().contains("muxal")
        || source.to_ascii_lowercase().contains("muxal");
    if !is_fuse || !names_muxal {
        return None;
    }

    let mp = unescape_mount_field(mountpoint);
    let is_appimage_mount = std::path::Path::new(&mp)
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with(".mount_"));

    is_appimage_mount.then_some(mp)
}

/// Decode the octal escapes the kernel writes into `/proc/self/mounts` path
/// fields — space (`\040`), tab (`\011`), newline (`\012`), backslash (`\134`) —
/// so a `$TMPDIR` containing those characters compares correctly. AppImage mount
/// dirs live under `/tmp` and rarely need this, but the transform is cheap and
/// keeps the parser correct.
fn unescape_mount_field(field: &str) -> String {
    if !field.contains('\\') {
        return field.to_string();
    }
    let bytes = field.as_bytes();
    let mut out = String::with_capacity(field.len());
    let mut i = 0;
    while i < bytes.len() {
        // A `\ooo` triple is a valid octal escape only when three octal digits
        // follow; anything else is copied through verbatim.
        if bytes[i] == b'\\'
            && i + 4 <= bytes.len()
            && let Ok(code) = u8::from_str_radix(&field[i + 1..i + 4], 8)
        {
            out.push(code as char);
            i += 4;
            continue;
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{foreign_muxal_appimage_mounts, own_appimage};
    use std::path::{Path, PathBuf};

    #[test]
    fn own_appimage_only_when_running_inside_it() {
        let image = Some("/home/me/Applications/muxal-linux-x86_64.AppImage");
        // Launched from the AppImage (mounted, or extracted-and-run).
        assert_eq!(
            own_appimage(
                image,
                Some("/tmp/.mount_muxal-AbC123"),
                Path::new("/tmp/.mount_muxal-AbC123/usr/bin/muxal")
            ),
            Some(PathBuf::from(
                "/home/me/Applications/muxal-linux-x86_64.AppImage"
            ))
        );
        assert!(
            own_appimage(
                image,
                Some("/tmp/appimage_extracted_0f1e"),
                Path::new("/tmp/appimage_extracted_0f1e/usr/bin/muxal")
            )
            .is_some()
        );
        // A .deb/.rpm muxal started by another AppImage inherits its variables.
        assert_eq!(
            own_appimage(
                Some("/opt/Grok-Bot.AppImage"),
                Some("/tmp/.mount_Grok-xYz"),
                Path::new("/usr/bin/muxal")
            ),
            None
        );
        // Path components, not string prefixes.
        assert_eq!(
            own_appimage(
                image,
                Some("/tmp/.mount_mux"),
                Path::new("/tmp/.mount_muxal-AbC123/usr/bin/muxal")
            ),
            None
        );
        assert_eq!(own_appimage(None, None, Path::new("/usr/bin/muxal")), None);
        assert_eq!(
            own_appimage(image, Some(""), Path::new("/usr/bin/muxal")),
            None
        );
    }

    // A realistic /proc/self/mounts slice: unrelated FUSE mounts, our own muxal
    // mount, and a leftover muxal mount from a prior instance.
    const MOUNTS: &str = "\
sysfs /sys sysfs rw,nosuid,nodev,noexec,relatime 0 0
gvfsd-fuse /run/user/1000/gvfs fuse.gvfsd-fuse rw,nosuid,nodev,relatime,user_id=1000,group_id=1000 0 0
portal /run/user/1000/doc fuse.portal rw,nosuid,nodev,relatime,user_id=1000,group_id=1000 0 0
muxal-linux-x86_64.AppImage /tmp/.mount_muxal-CDigJK fuse.muxal-linux-x86_64.AppImage ro,nosuid,nodev,relatime,user_id=1000,group_id=1000 0 0
muxal-linux-x86_64.AppImage /tmp/.mount_muxal-gMGfBF fuse.muxal-linux-x86_64.AppImage ro,nosuid,nodev,relatime,user_id=1000,group_id=1000 0 0
some.AppImage /tmp/.mount_someAB fuse.some.AppImage ro,relatime 0 0";

    #[test]
    fn returns_foreign_muxal_mounts_excluding_our_own() {
        // Running from gMGfBF: only the leftover CDigJK is a reap candidate —
        // not our own mount, not gvfsd/portal, not the unrelated AppImage.
        let got = foreign_muxal_appimage_mounts(MOUNTS, Some("/tmp/.mount_muxal-gMGfBF"));
        assert_eq!(got, vec!["/tmp/.mount_muxal-CDigJK".to_string()]);
    }

    #[test]
    fn without_own_appdir_returns_all_muxal_mounts() {
        // Not launched from an AppImage ($APPDIR unset): both muxal mounts are
        // leftovers to consider; the liveness probe sorts dead from live.
        let mut got = foreign_muxal_appimage_mounts(MOUNTS, None);
        got.sort();
        assert_eq!(
            got,
            vec![
                "/tmp/.mount_muxal-CDigJK".to_string(),
                "/tmp/.mount_muxal-gMGfBF".to_string(),
            ]
        );
    }

    #[test]
    fn ignores_non_muxal_and_non_fuse_mounts() {
        // No muxal mounts present at all → nothing selected (gvfsd/portal/other
        // AppImage/sysfs are all rejected).
        let other = "\
sysfs /sys sysfs rw 0 0
gvfsd-fuse /run/user/1000/gvfs fuse.gvfsd-fuse rw 0 0
some.AppImage /tmp/.mount_someAB fuse.some.AppImage ro 0 0";
        assert!(foreign_muxal_appimage_mounts(other, None).is_empty());
    }

    #[test]
    fn requires_appimage_mount_dir_shape() {
        // A muxal-named FUSE mount that isn't a `.mount_…` AppImage dir (e.g. a
        // bind of the project) is not an AppImage leftover and is left alone.
        let odd = "muxal /home/ryan/muxal fuse.muxal rw 0 0";
        assert!(foreign_muxal_appimage_mounts(odd, None).is_empty());
    }

    #[test]
    fn decodes_octal_escaped_mountpoint() {
        // $TMPDIR with a space: the kernel escapes it as \040; the returned path
        // is decoded so the caller can unmount it by its real name.
        let escaped = "muxal.AppImage /tmp/a\\040b/.mount_muxalXY fuse.muxal.AppImage ro 0 0";
        assert_eq!(
            foreign_muxal_appimage_mounts(escaped, None),
            vec!["/tmp/a b/.mount_muxalXY".to_string()]
        );
    }
}
