//! Running scheduled jobs while MyLoDB is closed: the system's own scheduler starts `MyLoDB --run-due-jobs` every few minutes. On
//! macOS that is a LaunchAgent in the user's Library, on Windows a task in the Task Scheduler. Nothing needs administrator rights.

use std::path::{Path, PathBuf};

pub const RUN_ARG: &str = "--run-due-jobs";
/// How often the system starts the check. A job runs up to this long after its time.
pub const EVERY_MINUTES: u32 = 5;
const LABEL: &str = "dev.mylodb.app.jobs";
const TASK: &str = "MyLoDB Scheduled Jobs";

#[derive(Debug, Clone, serde::Serialize)]
pub struct Status {
    pub supported: bool,
    pub enabled: bool,
    pub every_minutes: u32,
}

fn xml(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// The LaunchAgent that starts the check at login and then every few minutes.
pub fn plist(exe: &Path) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key><string>{LABEL}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{}</string>
        <string>{RUN_ARG}</string>
    </array>
    <key>StartInterval</key><integer>{}</integer>
    <key>RunAtLoad</key><true/>
    <key>ProcessType</key><string>Background</string>
</dict>
</plist>
"#,
        xml(&exe.to_string_lossy()),
        EVERY_MINUTES * 60
    )
}

/// The arguments for `schtasks` that create the task.
pub fn task_args(exe: &Path) -> Vec<String> {
    ["/Create", "/F", "/SC", "MINUTE", "/MO", &EVERY_MINUTES.to_string(), "/TN", TASK, "/TR", &format!("\"{}\" {RUN_ARG}", exe.display())].iter().map(|s| s.to_string()).collect()
}

/// A copy opened straight from the disk image (or moved aside by macOS) will not be there later.
pub fn stable_location(exe: &Path) -> Result<(), String> {
    let p = exe.to_string_lossy();
    if p.starts_with("/Volumes/") || p.contains("/AppTranslocation/") {
        return Err("Move MyLoDB to the Applications folder first: this copy is running from a temporary place.".into());
    }
    Ok(())
}

fn agent_path() -> Result<PathBuf, String> {
    let home = std::env::var_os("HOME").ok_or("no home folder")?;
    Ok(PathBuf::from(home).join("Library/LaunchAgents").join(format!("{LABEL}.plist")))
}

fn run(program: &str, args: &[&str]) -> Result<(), String> {
    let mut command = std::process::Command::new(program);
    command.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // no console window
    }
    let out = command.output().map_err(|e| format!("{program}: {e}"))?;
    if out.status.success() { return Ok(()); }
    let text = String::from_utf8_lossy(if out.stderr.is_empty() { &out.stdout } else { &out.stderr }).trim().to_string();
    Err(format!("{program}: {text}"))
}

pub fn status() -> Status {
    let enabled = if cfg!(target_os = "macos") {
        agent_path().map(|p| p.exists()).unwrap_or(false)
    } else if cfg!(windows) {
        run("schtasks", &["/Query", "/TN", TASK]).is_ok()
    } else {
        false
    };
    Status { supported: cfg!(any(target_os = "macos", windows)), enabled, every_minutes: EVERY_MINUTES }
}

fn user_domain() -> Result<String, String> {
    let out = std::process::Command::new("id").arg("-u").output().map_err(|e| e.to_string())?;
    Ok(format!("gui/{}", String::from_utf8_lossy(&out.stdout).trim()))
}

pub fn set(enabled: bool) -> Result<Status, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    if cfg!(target_os = "macos") {
        let path = agent_path()?;
        let domain = user_domain()?;
        // unload what is there first, so that a changed file takes effect; it is fine if nothing was loaded
        let _ = run("launchctl", &["bootout", &format!("{domain}/{LABEL}")]);
        if enabled {
            stable_location(&exe)?;
            if let Some(dir) = path.parent() { std::fs::create_dir_all(dir).map_err(|e| e.to_string())?; }
            std::fs::write(&path, plist(&exe)).map_err(|e| e.to_string())?;
            if let Err(e) = run("launchctl", &["bootstrap", &domain, &path.to_string_lossy()]) {
                let _ = std::fs::remove_file(&path);
                return Err(e);
            }
        } else if path.exists() {
            std::fs::remove_file(&path).map_err(|e| e.to_string())?;
        }
    } else if cfg!(windows) {
        if enabled {
            stable_location(&exe)?;
            let args = task_args(&exe);
            run("schtasks", &args.iter().map(String::as_str).collect::<Vec<_>>())?;
        } else if status().enabled {
            run("schtasks", &["/Delete", "/F", "/TN", TASK])?;
        }
    } else {
        return Err("Running jobs while MyLoDB is closed is available on macOS and Windows.".into());
    }
    Ok(status())
}

/// After the app moved (an install in another folder), point the existing macOS agent at this copy.
pub fn repair() {
    if !cfg!(target_os = "macos") { return; }
    let (Ok(path), Ok(exe)) = (agent_path(), std::env::current_exe()) else { return };
    // a development build must not take over the installed app's agent
    if cfg!(debug_assertions) || stable_location(&exe).is_err() { return; }
    if std::fs::read_to_string(&path).is_ok_and(|have| have != plist(&exe)) {
        let _ = set(true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_agent_runs_this_program_with_the_argument() {
        let p = plist(Path::new("/Applications/My & Co/MyLoDB.app/Contents/MacOS/mylodb-desktop"));
        assert!(p.contains("<string>/Applications/My &amp; Co/MyLoDB.app/Contents/MacOS/mylodb-desktop</string>"));
        assert!(p.contains("<string>--run-due-jobs</string>") && p.contains("<integer>300</integer>"));
    }

    #[test]
    fn the_windows_task_quotes_the_path() {
        let a = task_args(Path::new("C:\\Program Files\\MyLoDB\\mylodb.exe"));
        assert_eq!(a.last().unwrap(), "\"C:\\Program Files\\MyLoDB\\mylodb.exe\" --run-due-jobs");
        assert!(a.windows(2).any(|w| w == ["/MO", "5"]));
    }

    #[test]
    fn temporary_copies_are_refused() {
        assert!(stable_location(Path::new("/Volumes/MyLoDB/MyLoDB.app/Contents/MacOS/x")).is_err());
        assert!(stable_location(Path::new("/private/var/folders/a/AppTranslocation/b/d/MyLoDB.app/x")).is_err());
        assert!(stable_location(Path::new("/Applications/MyLoDB.app/Contents/MacOS/x")).is_ok());
    }
}
