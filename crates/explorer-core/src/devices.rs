//! Devices that are not mounted yet: block partitions (via `lsblk`) and
//! phones/cameras exposed by GVfs (via `gio mount -l`), plus network URIs.

use std::path::PathBuf;
use std::process::{Command, Stdio};

use serde::Deserialize;

#[derive(Debug, Clone, PartialEq)]
pub struct Volume {
    pub label: String,
    /// `/dev/sdb1` for block devices, a URI like `mtp://...` for GVfs volumes.
    pub id: String,
    pub size: u64,
    pub fs_type: String,
    pub removable: bool,
    pub kind: VolumeKind,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VolumeKind {
    Block,
    Phone,
}

#[derive(Deserialize)]
struct LsblkOut {
    blockdevices: Vec<LsblkDev>,
}

#[derive(Deserialize)]
struct LsblkDev {
    name: String,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    fstype: Option<String>,
    #[serde(default)]
    mountpoints: Option<Vec<Option<String>>>,
    #[serde(default)]
    mountpoint: Option<String>,
    #[serde(default, deserialize_with = "de_u64")]
    size: u64,
    #[serde(default, deserialize_with = "de_bool")]
    rm: bool,
    #[serde(default, deserialize_with = "de_bool")]
    hotplug: bool,
    #[serde(default)]
    children: Vec<LsblkDev>,
    #[serde(rename = "type", default)]
    kind: String,
}

fn de_u64<'de, D: serde::Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    Ok(match v {
        serde_json::Value::Number(n) => n.as_u64().unwrap_or(0),
        serde_json::Value::String(s) => s.parse().unwrap_or(0),
        _ => 0,
    })
}

fn de_bool<'de, D: serde::Deserializer<'de>>(d: D) -> Result<bool, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    Ok(match v {
        serde_json::Value::Bool(b) => b,
        serde_json::Value::String(s) => s == "1" || s == "true",
        serde_json::Value::Number(n) => n.as_u64() == Some(1),
        _ => false,
    })
}

/// Partitions with a mountable file system that are not mounted.
pub fn parse_lsblk(json: &str) -> Vec<Volume> {
    let Ok(out) = serde_json::from_str::<LsblkOut>(json) else { return vec![] };
    let mut v = Vec::new();
    fn walk(d: &LsblkDev, parent_removable: bool, v: &mut Vec<Volume>) {
        let removable = d.rm || d.hotplug || parent_removable;
        let mounted = d.mountpoint.is_some() || d.mountpoints.as_ref().is_some_and(|m| m.iter().any(|x| x.is_some()));
        let fs = d.fstype.clone().unwrap_or_default();
        let mountable = !fs.is_empty() && !["swap", "LVM2_member", "crypto_LUKS", "linux_raid_member", "squashfs"].contains(&fs.as_str());
        if mountable && !mounted && d.kind != "loop" {
            v.push(Volume {
                label: d.label.clone().filter(|l| !l.is_empty()).unwrap_or_else(|| format!("{} Volume", human(d.size))),
                id: format!("/dev/{}", d.name),
                size: d.size,
                fs_type: fs,
                removable,
                kind: VolumeKind::Block,
            });
        }
        for c in &d.children {
            walk(c, removable, v);
        }
    }
    for d in &out.blockdevices {
        walk(d, false, &mut v);
    }
    v
}

fn human(b: u64) -> String {
    crate::format::size_human(b)
}

pub fn unmounted_block_volumes() -> Vec<Volume> {
    let out = Command::new("lsblk").args(["-J", "-b", "-o", "NAME,LABEL,FSTYPE,MOUNTPOINTS,SIZE,RM,HOTPLUG,TYPE"]).output();
    match out {
        Ok(o) if o.status.success() => parse_lsblk(&String::from_utf8_lossy(&o.stdout)),
        _ => vec![],
    }
}

/// Unmounted GVfs volumes with an activation root (phones, cameras).
pub fn parse_gio_volumes(text: &str) -> Vec<Volume> {
    let mut v = Vec::new();
    let mut cur: Option<String> = None;
    for line in text.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("Volume(") {
            cur = rest.split_once("): ").map(|(_, n)| n.trim().to_string());
        } else if let (Some(name), Some(uri)) = (&cur, t.strip_prefix("activation_root=")) {
            if uri.starts_with("mtp://") || uri.starts_with("gphoto2://") || uri.starts_with("afc://") {
                v.push(Volume { label: name.clone(), id: uri.to_string(), size: 0, fs_type: "MTP".into(), removable: true, kind: VolumeKind::Phone });
            }
        } else if t.starts_with("Mount(") || t.starts_with("Drive(") {
            cur = None;
        }
    }
    v
}

pub fn phone_volumes() -> Vec<Volume> {
    match Command::new("gio").args(["mount", "-li"]).stderr(Stdio::null()).output() {
        Ok(o) if o.status.success() => parse_gio_volumes(&String::from_utf8_lossy(&o.stdout)),
        _ => vec![],
    }
}

/// Mount a volume; returns the mount point to navigate to.
pub fn mount(vol: &Volume) -> Result<PathBuf, String> {
    match vol.kind {
        VolumeKind::Block => {
            let o = Command::new("udisksctl")
                .args(["mount", "-b", &vol.id, "--no-user-interaction"])
                .output()
                .map_err(|e| format!("udisksctl: {e} (install udisks2)"))?;
            if !o.status.success() {
                return Err(String::from_utf8_lossy(&o.stderr).trim().to_string());
            }
            // "Mounted /dev/sdb1 at /media/user/LABEL"
            let s = String::from_utf8_lossy(&o.stdout);
            s.split(" at ").nth(1).map(|p| PathBuf::from(p.trim().trim_end_matches('.'))).ok_or_else(|| s.to_string())
        }
        VolumeKind::Phone => mount_uri(&vol.id, None),
    }
}

pub fn is_network_uri(s: &str) -> bool {
    ["smb://", "sftp://", "ssh://", "ftp://", "ftps://", "dav://", "davs://", "afp://", "nfs://", "mtp://", "gphoto2://", "webdav://"]
        .iter()
        .any(|p| s.starts_with(p))
}

/// `gio mount` a URI (optionally with user/domain/password answered on
/// stdin) and return the local FUSE path GVfs exposes for it.
pub fn mount_uri(uri: &str, creds: Option<(&str, &str, &str)>) -> Result<PathBuf, String> {
    let uri = uri.replace("ssh://", "sftp://").replace("webdav://", "dav://");
    let mut cmd = Command::new("gio");
    cmd.args(["mount", &uri]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| format!("gio: {e} (install glib2 / libglib2.0-bin and gvfs)"))?;
    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        let input = match creds {
            Some((user, domain, pass)) => format!("{user}\n{domain}\n{pass}\n"),
            None => "\n\n\n".into(),
        };
        let _ = stdin.write_all(input.as_bytes());
    }
    let o = child.wait_with_output().map_err(|e| e.to_string())?;
    let err = String::from_utf8_lossy(&o.stderr).to_string();
    if !o.status.success() && !err.contains("already mounted") {
        return Err(if err.trim().is_empty() { "The location could not be mounted.".into() } else { err.trim().to_string() });
    }
    local_path(&uri).ok_or_else(|| "Mounted, but GVfs exposes no local path (is gvfs-fuse installed?)".into())
}

pub fn local_path(uri: &str) -> Option<PathBuf> {
    let o = Command::new("gio").args(["info", "-a", "standard::name", uri]).stderr(Stdio::null()).output().ok()?;
    let s = String::from_utf8_lossy(&o.stdout);
    s.lines().find_map(|l| l.trim().strip_prefix("local path:").map(|p| PathBuf::from(p.trim())))
}

pub fn needs_password(err: &str) -> bool {
    let e = err.to_lowercase();
    e.contains("password") || e.contains("permission denied") || e.contains("authentication") || e.contains("login") || e.contains("access denied")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lsblk_unmounted_partitions() {
        let json = r#"{"blockdevices":[
          {"name":"sda","label":null,"fstype":null,"mountpoints":[null],"size":500107862016,"rm":false,"hotplug":false,"type":"disk","children":[
            {"name":"sda1","label":"EFI","fstype":"vfat","mountpoints":["/boot/efi"],"size":536870912,"rm":false,"hotplug":false,"type":"part"},
            {"name":"sda2","label":"Data","fstype":"ext4","mountpoints":[null],"size":1000000,"rm":false,"hotplug":false,"type":"part"},
            {"name":"sda3","label":null,"fstype":"swap","mountpoints":["[SWAP]"],"size":1,"rm":false,"hotplug":false,"type":"part"}]},
          {"name":"sdb","label":null,"fstype":null,"mountpoints":[null],"size":"16000000000","rm":"1","hotplug":"1","type":"disk","children":[
            {"name":"sdb1","label":"USB STICK","fstype":"exfat","mountpoints":[null],"size":"16000000000","rm":"1","hotplug":"1","type":"part"}]}]}"#;
        let v = parse_lsblk(json);
        assert_eq!(v.len(), 2);
        assert_eq!((v[0].id.as_str(), v[0].label.as_str(), v[0].removable), ("/dev/sda2", "Data", false));
        assert_eq!((v[1].id.as_str(), v[1].label.as_str(), v[1].removable), ("/dev/sdb1", "USB STICK", true));
    }

    #[test]
    fn gio_phone_volumes() {
        let text = "Drive(0): Samsung SSD\n  Type: GProxyDrive\nVolume(0): Pixel 9\n  Type: GProxyVolume (GProxyVolumeMonitorMTP)\n  activation_root=mtp://Google_Pixel_9_ABC/\nVolume(1): Data\n  uuid=1234\n";
        let v = parse_gio_volumes(text);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].label, "Pixel 9");
        assert_eq!(v[0].id, "mtp://Google_Pixel_9_ABC/");
    }

    #[test]
    fn uris() {
        assert!(is_network_uri("smb://nas/share"));
        assert!(is_network_uri("sftp://me@host/home"));
        assert!(!is_network_uri("/home/me"));
        assert!(needs_password("Error mounting location: Password dialog cancelled"));
    }
}
