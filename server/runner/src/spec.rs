//! What a sandbox is allowed, and the OCI runtime config that enforces it. Pure: no files, no processes.

use std::path::PathBuf;

use serde_json::{Value, json};

/// Everything a run may have. Nothing else exists inside: no network, no host files, no credentials.
#[derive(Clone, Debug)]
pub struct Sandbox {
    /// A root filesystem with the interpreter and libraries (read-only inside).
    pub rootfs: PathBuf,
    /// Folders the code may read, mounted read-only at `/inputs/<name>`.
    pub inputs: Vec<(String, PathBuf)>,
    /// The one folder the code may write, mounted at `/workspace`.
    pub workspace: PathBuf,
    pub args: Vec<String>,
    pub memory_bytes: u64,
    pub pids_max: u64,
    /// Share of one CPU in thousandths (1000 = one full CPU).
    pub cpu_millis: u64,
}

pub const MAX_MEMORY_BYTES: u64 = 8 << 30;
pub const MAX_PIDS: u64 = 1024;

impl Sandbox {
    /// # Errors
    /// What is wrong, if the request asks for something unsafe or meaningless.
    pub fn validate(&self) -> Result<(), String> {
        if self.args.is_empty() {
            return Err("there is nothing to run".into());
        }
        if self.memory_bytes == 0 || self.memory_bytes > MAX_MEMORY_BYTES {
            return Err("memory must be set, and at most 8 GiB".into());
        }
        if self.pids_max == 0 || self.pids_max > MAX_PIDS {
            return Err("the process limit must be set, and at most 1024".into());
        }
        if self.cpu_millis == 0 || self.cpu_millis > 8000 {
            return Err("cpu must be set, and at most 8 CPUs".into());
        }
        for (name, path) in &self.inputs {
            let plain = !name.is_empty()
                && name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
            if !plain || !path.is_absolute() {
                return Err(format!(
                    "input {name:?} must have a plain name and an absolute path"
                ));
            }
        }
        if !self.rootfs.is_absolute() || !self.workspace.is_absolute() {
            return Err("rootfs and workspace must be absolute paths".into());
        }
        Ok(())
    }
}

fn bind(source: &std::path::Path, destination: &str, mode: &str) -> Value {
    // `nosuid`/`nodev` on every mount; `noexec` keeps the workspace from running binaries the code wrote itself.
    let mut options = vec!["rbind", mode, "nosuid", "nodev"];
    if destination == "/workspace" {
        options.push("noexec");
    }
    json!({"destination": destination, "type": "bind", "source": source.display().to_string(), "options": options})
}

/// The OCI config for one run.
///
/// # Errors
/// If the sandbox itself is not valid.
pub fn oci_config(s: &Sandbox) -> Result<Value, String> {
    s.validate()?;
    let mut mounts = vec![
        json!({"destination": "/proc", "type": "proc", "source": "proc"}),
        json!({"destination": "/tmp", "type": "tmpfs", "source": "tmpfs", "options": ["nosuid", "nodev", "noexec", "size=64m"]}),
        bind(&s.workspace, "/workspace", "rw"),
    ];
    for (name, path) in &s.inputs {
        mounts.push(bind(path, &format!("/inputs/{name}"), "ro"));
    }
    Ok(json!({
        "ociVersion": "1.0.2",
        "process": {
            "terminal": false,
            "user": {"uid": 65534, "gid": 65534},
            "args": s.args,
            "env": ["PATH=/usr/local/bin:/usr/bin:/bin", "HOME=/workspace", "LANG=C.UTF-8"],
            "cwd": "/workspace",
            "noNewPrivileges": true,
            "capabilities": {"bounding": [], "effective": [], "inheritable": [], "permitted": [], "ambient": []},
            "rlimits": [{"type": "RLIMIT_NOFILE", "hard": 1024, "soft": 1024}, {"type": "RLIMIT_CORE", "hard": 0, "soft": 0}],
        },
        "root": {"path": s.rootfs.display().to_string(), "readonly": true},
        "hostname": "sandbox",
        "mounts": mounts,
        "linux": {
            // A new, empty network namespace (no `path`): the code has no network at all.
            "namespaces": [{"type": "pid"}, {"type": "network"}, {"type": "ipc"}, {"type": "uts"}, {"type": "mount"}],
            "resources": {
                "memory": {"limit": s.memory_bytes, "swap": s.memory_bytes},
                "pids": {"limit": s.pids_max},
                "cpu": {"quota": s.cpu_millis * 100, "period": 100_000},
            },
            "maskedPaths": ["/proc/kcore", "/proc/keys", "/proc/sched_debug", "/proc/timer_list"],
            "readonlyPaths": ["/proc/bus", "/proc/fs", "/proc/irq", "/proc/sys", "/proc/sysrq-trigger"],
        },
    }))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    pub fn sandbox() -> Sandbox {
        Sandbox {
            rootfs: "/srv/rootfs".into(),
            inputs: vec![("sales".into(), "/data/sales".into())],
            workspace: "/work/run1".into(),
            args: vec!["python3".into(), "analysis.py".into()],
            memory_bytes: 512 << 20,
            pids_max: 64,
            cpu_millis: 1000,
        }
    }

    #[test]
    fn the_config_has_every_limit_and_no_network() {
        let c = oci_config(&sandbox()).unwrap();
        assert_eq!(c["linux"]["resources"]["memory"]["limit"], 512u64 << 20);
        assert_eq!(c["linux"]["resources"]["pids"]["limit"], 64);
        assert_eq!(c["root"]["readonly"], true);
        assert_eq!(c["process"]["noNewPrivileges"], true);
        let net = c["linux"]["namespaces"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["type"] == "network")
            .unwrap();
        assert!(net.get("path").is_none(), "an empty network of its own");
    }

    #[test]
    fn inputs_are_read_only_and_the_workspace_is_the_one_writable_place() {
        let c = oci_config(&sandbox()).unwrap();
        let mounts = c["mounts"].as_array().unwrap();
        let opts = |dest: &str| -> Vec<String> {
            mounts.iter().find(|m| m["destination"] == dest).unwrap()["options"]
                .as_array()
                .unwrap()
                .iter()
                .map(|o| o.as_str().unwrap().to_owned())
                .collect()
        };
        assert!(opts("/inputs/sales").contains(&"ro".to_owned()));
        assert!(opts("/workspace").contains(&"rw".to_owned()));
        assert!(opts("/workspace").contains(&"noexec".to_owned()));
    }

    #[test]
    fn unsafe_or_meaningless_requests_are_refused() {
        let mut s = sandbox();
        s.memory_bytes = 0;
        assert!(oci_config(&s).is_err());
        let mut s = sandbox();
        s.pids_max = 100_000;
        assert!(oci_config(&s).is_err());
        let mut s = sandbox();
        s.args.clear();
        assert!(oci_config(&s).is_err());
        let mut s = sandbox();
        s.inputs.push(("../x".into(), "/data".into()));
        assert!(oci_config(&s).is_err());
        let mut s = sandbox();
        s.workspace = "relative".into();
        assert!(oci_config(&s).is_err());
    }
}
