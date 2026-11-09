//! Conservative foreground-agent identification for user-requested handoffs.
//! Periodic inspection runs on the background executor, never while rendering;
//! the process is rechecked when the user explicitly starts a handoff.
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShellAgent {
    pub pid: u32,
    pub identity: String,
    pub cwd: PathBuf,
    pub name: &'static str,
}

/// Only match known executable names or known interpreter entry points. Never
/// scan arbitrary arguments: `echo codex` must not be mistaken for an agent.
fn agent_name(args: &[String]) -> Option<&'static str> {
    let first = Path::new(args.first()?).file_name()?.to_str()?;
    match first {
        "codex" => Some("codex"),
        "agy" => Some("agy"),
        "claude" => Some("claude"),
        "opencode" => Some("opencode"),
        "pi" => Some("pi"),
        "node" | "nodejs" | "bun" => {
            let script = args.get(1)?.replace('\\', "/");
            if script.ends_with("/@openai/codex/bin/codex.js") {
                Some("codex")
            } else if script.ends_with("/@anthropic-ai/claude-code/cli.js") {
                Some("claude")
            } else if script.contains("/pi-coding-agent/") && script.ends_with("/cli.js") {
                Some("pi")
            } else {
                None
            }
        }
        _ => None,
    }
}

fn resolved_agent_name(mut args: Vec<String>) -> Option<&'static str> {
    if let Some(name) = agent_name(&args) {
        return Some(name);
    }
    let executable = Path::new(args.first()?).file_name()?.to_str()?;
    if !matches!(executable, "node" | "nodejs" | "bun") {
        return None;
    }
    // Node often receives a symlink under node_modules/.bin as its entry point.
    let script = std::fs::canonicalize(args.get(1)?)
        .ok()?
        .to_str()?
        .to_owned();
    args[1] = script;
    agent_name(&args)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub fn inspect(pid: u32) -> Option<ShellAgent> {
    let (mut leader, wrapper) = inspect_process(pid)?;
    if !wrapper {
        return Some(leader);
    }
    // Some Node launchers spawn the native agent rather than replacing
    // themselves. The native process can change directory independently.
    // Only accept one recognizable child in the same foreground process group.
    let mut found = None;
    for child in child_pids(pid)? {
        if unsafe { libc::getpgid(i32::try_from(child).ok()?) } != i32::try_from(pid).ok()? {
            continue;
        }
        if let Some((agent, _)) = inspect_process(child)
            && agent.name == leader.name
        {
            if found.is_some() {
                return None;
            }
            found = Some(agent);
        }
    }
    if let Some(agent) = found {
        leader.identity = format!("{}:{}:{}", leader.identity, agent.pid, agent.identity);
        leader.cwd = agent.cwd;
    }
    Some(leader)
}

fn is_node_wrapper(args: &[String]) -> bool {
    args.first()
        .and_then(|arg| Path::new(arg).file_name())
        .and_then(|name| name.to_str())
        .is_some_and(|name| matches!(name, "node" | "nodejs" | "bun"))
}

#[cfg(target_os = "linux")]
fn child_pids(pid: u32) -> Option<Vec<u32>> {
    let path = format!("/proc/{pid}/task/{pid}/children");
    let children = std::fs::read_to_string(path).ok()?;
    let pids: Vec<u32> = children
        .split_whitespace()
        .map(str::parse)
        .collect::<Result<_, _>>()
        .ok()?;
    (pids.len() <= 64).then_some(pids)
}

#[cfg(target_os = "macos")]
fn child_pids(pid: u32) -> Option<Vec<u32>> {
    let mut pids = [0i32; 65];
    // SAFETY: the buffer is valid and its byte length is supplied to libproc.
    let count = unsafe {
        libc::proc_listchildpids(
            i32::try_from(pid).ok()?,
            pids.as_mut_ptr().cast(),
            std::mem::size_of_val(&pids) as i32,
        )
    };
    if count < 0 || pids[64] != 0 {
        return None;
    }
    Some(
        pids.into_iter()
            .filter_map(|pid| u32::try_from(pid).ok().filter(|p| *p != 0))
            .collect(),
    )
}

#[cfg(target_os = "linux")]
fn inspect_process(pid: u32) -> Option<(ShellAgent, bool)> {
    use std::io::Read;
    let base = PathBuf::from(format!("/proc/{pid}"));
    let mut raw = Vec::new();
    std::fs::File::open(base.join("cmdline"))
        .ok()?
        .take(64 * 1024)
        .read_to_end(&mut raw)
        .ok()?;
    let args: Vec<String> = raw
        .split(|b| *b == 0)
        .filter(|a| !a.is_empty())
        .map(|a| String::from_utf8_lossy(a).into_owned())
        .collect();
    let wrapper = is_node_wrapper(&args);
    let name = resolved_agent_name(args)?;
    let stat = std::fs::read_to_string(base.join("stat")).ok()?;
    let start = stat.rsplit_once(')')?.1.split_whitespace().nth(19)?;
    let boot = std::fs::read_to_string("/proc/sys/kernel/random/boot_id").ok()?;
    let cwd = std::fs::read_link(base.join("cwd")).ok()?;
    Some((
        ShellAgent {
            pid,
            identity: format!("{}:{start}", boot.trim()),
            cwd,
            name,
        },
        wrapper,
    ))
}

#[cfg(target_os = "macos")]
fn inspect_process(pid: u32) -> Option<(ShellAgent, bool)> {
    use std::os::unix::ffi::OsStringExt;
    let pid = i32::try_from(pid).ok()?;
    let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid];
    let mut length = 0usize;
    // SAFETY: all pointers reference valid buffers; sysctl writes at most length.
    if unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            3,
            std::ptr::null_mut(),
            &mut length,
            std::ptr::null_mut(),
            0,
        )
    } != 0
        || length > 1024 * 1024
        || length < 5
    {
        return None;
    }
    let mut bytes = vec![0u8; length];
    if unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            3,
            bytes.as_mut_ptr().cast(),
            &mut length,
            std::ptr::null_mut(),
            0,
        )
    } != 0
    {
        return None;
    }
    bytes.truncate(length);
    let argc = i32::from_ne_bytes(bytes.get(..4)?.try_into().ok()?);
    if !(1..=4096).contains(&argc) {
        return None;
    }
    let rest = bytes.get(4..)?;
    let executable_end = rest.iter().position(|b| *b == 0)?;
    let rest = rest.get(executable_end + 1..)?;
    let first = rest.iter().position(|b| *b != 0)?;
    let args: Vec<String> = rest[first..]
        .split(|b| *b == 0)
        .take(argc as usize)
        .map(|a| String::from_utf8_lossy(a).into_owned())
        .collect();
    if args.len() != argc as usize {
        return None;
    }
    let wrapper = is_node_wrapper(&args);
    let name = resolved_agent_name(args)?;
    // SAFETY: both structs are plain C data, initialized to zero, and the API
    // receives their exact sizes. A short result is rejected.
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let info_size = std::mem::size_of_val(&info);
    if unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            (&mut info as *mut libc::proc_bsdinfo).cast(),
            info_size as i32,
        )
    } != info_size as i32
    {
        return None;
    }
    let mut vnode: libc::proc_vnodepathinfo = unsafe { std::mem::zeroed() };
    let vnode_size = std::mem::size_of_val(&vnode);
    if unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDVNODEPATHINFO,
            0,
            (&mut vnode as *mut libc::proc_vnodepathinfo).cast(),
            vnode_size as i32,
        )
    } != vnode_size as i32
    {
        return None;
    }
    let path = vnode.pvi_cdir.vip_path.as_flattened();
    let end = path.iter().position(|c| *c == 0)?;
    let cwd = PathBuf::from(std::ffi::OsString::from_vec(
        path[..end].iter().map(|c| *c as u8).collect(),
    ));
    Some((
        ShellAgent {
            pid: pid as u32,
            identity: format!("{}:{}", info.pbi_start_tvsec, info.pbi_start_tvusec),
            cwd,
            name,
        },
        wrapper,
    ))
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub fn inspect(_pid: u32) -> Option<ShellAgent> {
    None
}

#[cfg(test)]
mod tests {
    use super::agent_name;

    #[test]
    fn only_recognizes_agent_executables_and_known_node_entry_points() {
        for name in ["codex", "agy", "claude", "opencode", "pi"] {
            assert_eq!(agent_name(&[format!("/usr/bin/{name}")]), Some(name));
        }
        assert_eq!(
            agent_name(&[
                "node".into(),
                "/lib/node_modules/@openai/codex/bin/codex.js".into()
            ]),
            Some("codex")
        );
        assert_eq!(
            agent_name(&[
                "node".into(),
                "/lib/node_modules/@anthropic-ai/claude-code/cli.js".into()
            ]),
            Some("claude")
        );
        assert_eq!(
            agent_name(&[
                "node".into(),
                "/node_modules/@earendil-works/pi-coding-agent/dist/cli.js".into()
            ]),
            Some("pi")
        );
        for args in [
            vec!["bash", "-c", "codex"],
            vec!["echo", "pi"],
            vec!["node", "/tmp/codex.js"],
            vec!["vim", "claude"],
        ] {
            assert_eq!(
                agent_name(&args.into_iter().map(str::to_string).collect::<Vec<_>>()),
                None
            );
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn identifies_real_process_cwd_and_rejects_unrelated_commands() {
        use std::os::unix::fs::symlink;
        use std::process::Command;
        let root = std::env::temp_dir().join(format!("muxal-shell-agent-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("working")).unwrap();
        for name in ["codex", "agy", "claude", "opencode", "pi"] {
            symlink("/bin/sleep", root.join(name)).unwrap();
            let mut child = Command::new(root.join(name))
                .arg("30")
                .current_dir(root.join("working"))
                .spawn()
                .unwrap();
            // /proc may briefly expose an empty command line during exec.
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
            let found = loop {
                if let Some(found) = super::inspect(child.id()) {
                    break found;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "agent {name} did not start"
                );
                std::thread::sleep(std::time::Duration::from_millis(10));
            };
            assert_eq!(found.name, name);
            assert_eq!(found.cwd, root.join("working"));
            assert!(!found.identity.is_empty());
            child.kill().unwrap();
            child.wait().unwrap();
            assert!(super::inspect(child.id()).is_none());
        }
        let mut unrelated = Command::new("/bin/sleep").arg("30").spawn().unwrap();
        assert!(super::inspect(unrelated.id()).is_none());
        unrelated.kill().unwrap();
        unrelated.wait().unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn node_launcher_uses_native_child_directory_and_identity() {
        use std::os::unix::{fs::symlink, process::CommandExt};
        use std::process::Command;
        use std::time::{Duration, Instant};
        let root =
            std::env::temp_dir().join(format!("muxal-agent-wrapper-{}", uuid::Uuid::new_v4()));
        let package = root.join("@openai/codex/bin");
        let working = root.join("working");
        std::fs::create_dir_all(&package).unwrap();
        std::fs::create_dir_all(&working).unwrap();
        symlink("/bin/bash", root.join("node")).unwrap();
        symlink("/bin/sleep", root.join("codex")).unwrap();
        std::fs::write(package.join("codex.js"), "cd \"$1\"; \"$2\" 30 & wait\n").unwrap();
        let mut wrapper = Command::new(root.join("node"))
            .arg(package.join("codex.js"))
            .arg(&working)
            .arg(root.join("codex"))
            .current_dir(&root)
            .process_group(0)
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let found = loop {
            if let Some(agent) = super::inspect(wrapper.id())
                && agent.cwd == working
            {
                break agent;
            }
            assert!(Instant::now() < deadline, "native agent did not start");
            std::thread::sleep(Duration::from_millis(10));
        };
        assert_eq!(found.pid, wrapper.id());
        assert_eq!(found.name, "codex");
        let children = super::child_pids(wrapper.id()).unwrap();
        for pid in children {
            unsafe {
                libc::kill(pid as i32, libc::SIGKILL);
            }
        }
        wrapper.kill().ok();
        wrapper.wait().unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn resolves_node_launcher_symlinks_without_trusting_unrelated_scripts() {
        use std::os::unix::fs::symlink;
        let root =
            std::env::temp_dir().join(format!("muxal-shell-agent-links-{}", uuid::Uuid::new_v4()));
        let package = root.join("node_modules/@openai/codex/bin");
        std::fs::create_dir_all(&package).unwrap();
        std::fs::write(package.join("codex.js"), "").unwrap();
        symlink(package.join("codex.js"), root.join("codex")).unwrap();
        assert_eq!(
            super::resolved_agent_name(vec![
                "node".into(),
                root.join("codex").display().to_string()
            ]),
            Some("codex")
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn detects_an_agent_started_from_a_real_shell_pty() {
        use muxal_terminal::{CommandSpec, TerminalSession};
        use std::os::unix::fs::symlink;
        use std::time::{Duration, Instant};
        let root =
            std::env::temp_dir().join(format!("muxal-shell-agent-pty-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        symlink("/bin/sleep", root.join("codex")).unwrap();
        let mut spec = CommandSpec::program(
            "/bin/bash",
            vec!["--noprofile".into(), "--norc".into(), "-i".into()],
        )
        .with_cwd(root.display().to_string());
        spec.env.push(("HOME".into(), root.display().to_string()));
        spec.env.push(("HISTFILE".into(), "/dev/null".into()));
        let (session, _output) = TerminalSession::spawn(spec, 80, 24).unwrap();
        let shell_pid = session.child_pid().unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while session.foreground_pid() != Some(shell_pid) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(session.foreground_pid(), Some(shell_pid));
        assert!(super::inspect(shell_pid).is_none());
        session.write_input(format!("{} 30\r", root.join("codex").display()).as_bytes());
        let deadline = Instant::now() + Duration::from_secs(10);
        let agent = loop {
            if let Some(pid) = session.foreground_pid()
                && pid != shell_pid
                && let Some(agent) = super::inspect(pid)
            {
                break agent;
            }
            assert!(
                Instant::now() < deadline,
                "agent did not take the foreground"
            );
            std::thread::sleep(Duration::from_millis(10));
        };
        assert_eq!(agent.name, "codex");
        assert_eq!(agent.cwd, root);
        session.write_input(b"\x03");
        let deadline = Instant::now() + Duration::from_secs(10);
        while session.foreground_pid() != Some(shell_pid) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(session.foreground_pid(), Some(shell_pid));
        assert!(super::inspect(shell_pid).is_none());
        session.kill();
        std::fs::remove_dir_all(root).unwrap();
    }
}
