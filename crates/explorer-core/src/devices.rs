//! Drives that are present but not mounted, mounting, unmounting and
//! ejecting — done in-process over D-Bus with the UDisks2 system service
//! (what GNOME Files and Dolphin use), no command-line tools.

use std::collections::HashMap;
use std::path::PathBuf;

use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

const UDISKS: &str = "org.freedesktop.UDisks2";
const BLOCK: &str = "org.freedesktop.UDisks2.Block";
const FS: &str = "org.freedesktop.UDisks2.Filesystem";
const DRIVE: &str = "org.freedesktop.UDisks2.Drive";

#[derive(Debug, Clone, PartialEq)]
pub struct Volume {
    pub label: String,
    /// UDisks2 block object path.
    pub id: String,
    pub device: String,
    pub size: u64,
    pub fs_type: String,
    pub removable: bool,
}

type Props = HashMap<String, OwnedValue>;
type Objects = HashMap<OwnedObjectPath, HashMap<String, Props>>;

fn objects() -> Result<(Connection, Objects), String> {
    let conn = Connection::system().map_err(|e| format!("D-Bus system bus: {e}"))?;
    let om = Proxy::new(&conn, UDISKS, "/org/freedesktop/UDisks2", "org.freedesktop.DBus.ObjectManager").map_err(|e| e.to_string())?;
    let objs: Objects = om.call("GetManagedObjects", &()).map_err(|e| format!("UDisks2 is not available: {e}"))?;
    Ok((conn, objs))
}

fn s(p: &Props, k: &str) -> String {
    p.get(k).and_then(|v| <&str>::try_from(&**v).ok().map(str::to_string)).unwrap_or_default()
}

fn b(p: &Props, k: &str) -> bool {
    p.get(k).and_then(|v| bool::try_from(&**v).ok()).unwrap_or(false)
}

fn u(p: &Props, k: &str) -> u64 {
    p.get(k).and_then(|v| u64::try_from(&**v).ok()).unwrap_or(0)
}

fn bytes(p: &Props, k: &str) -> Vec<u8> {
    let Some(v) = p.get(k) else { return vec![] };
    match &**v {
        Value::Array(a) => a.iter().filter_map(|x| u8::try_from(x).ok()).filter(|c| *c != 0).collect(),
        _ => vec![],
    }
}

fn mount_points(p: &Props) -> usize {
    match p.get("MountPoints").map(|v| &**v) {
        Some(Value::Array(a)) => a.len(),
        _ => 0,
    }
}

/// Pick unmounted, user-visible file systems out of UDisks2's object tree.
pub fn volumes_from(objs: &Objects) -> Vec<Volume> {
    let mut out = Vec::new();
    for (path, ifaces) in objs {
        let (Some(block), Some(fs)) = (ifaces.get(BLOCK), ifaces.get(FS)) else { continue };
        if mount_points(fs) > 0 || b(block, "HintIgnore") || s(block, "IdUsage") != "filesystem" {
            continue;
        }
        let drive_path = block.get("Drive").and_then(|v| OwnedObjectPath::try_from(v.clone()).ok());
        let drive = drive_path.as_ref().and_then(|d| objs.get(d)).and_then(|i| i.get(DRIVE));
        let removable = drive.is_some_and(|d| b(d, "Removable") || b(d, "MediaRemovable")) || !b(block, "HintSystem");
        let size = u(block, "Size");
        let label = [s(block, "HintName"), s(block, "IdLabel")]
            .into_iter()
            .find(|l| !l.is_empty())
            .unwrap_or_else(|| format!("{} Volume", crate::format::size_human(size)));
        out.push(Volume {
            label,
            id: path.to_string(),
            device: String::from_utf8_lossy(&bytes(block, "PreferredDevice")).into_owned(),
            size,
            fs_type: s(block, "IdType"),
            removable,
        });
    }
    out.sort_by(|a, b| a.device.cmp(&b.device));
    out
}

pub fn unmounted_block_volumes() -> Vec<Volume> {
    objects().map(|(_, o)| volumes_from(&o)).unwrap_or_default()
}

fn no_options() -> HashMap<&'static str, Value<'static>> {
    HashMap::new()
}

/// Mount through UDisks2 (polkit asks for a password only when policy requires).
pub fn mount(vol: &Volume) -> Result<PathBuf, String> {
    let conn = Connection::system().map_err(|e| e.to_string())?;
    let fs = Proxy::new(&conn, UDISKS, vol.id.as_str(), FS).map_err(|e| e.to_string())?;
    let path: String = fs.call("Mount", &(no_options(),)).map_err(|e| e.to_string())?;
    Ok(PathBuf::from(path))
}

/// Block object for a device node such as `/dev/sdb1`.
fn block_for_device(objs: &Objects, device: &str) -> Option<(OwnedObjectPath, Option<OwnedObjectPath>)> {
    objs.iter().find_map(|(path, ifaces)| {
        let block = ifaces.get(BLOCK)?;
        let names = [bytes(block, "Device"), bytes(block, "PreferredDevice")];
        names.iter().any(|n| n == device.as_bytes()).then(|| {
            let drive = block.get("Drive").and_then(|v| OwnedObjectPath::try_from(v.clone()).ok()).filter(|d| d.as_str() != "/");
            (path.clone(), drive)
        })
    })
}

/// Unmount `device`; with `eject`, also power the drive off so it can be unplugged.
pub fn unmount(device: &str, eject: bool) -> Result<(), String> {
    let (conn, objs) = objects()?;
    let (block, drive) = block_for_device(&objs, device).ok_or_else(|| format!("{device} is not managed by UDisks2"))?;
    let fs = Proxy::new(&conn, UDISKS, block.as_str(), FS).map_err(|e| e.to_string())?;
    let _: () = fs.call("Unmount", &(no_options(),)).map_err(|e| e.to_string())?;
    if eject {
        if let Some(d) = drive {
            let drv = Proxy::new(&conn, UDISKS, d.as_str(), DRIVE).map_err(|e| e.to_string())?;
            let _: () = drv.call("PowerOff", &(no_options(),)).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// URIs of remote locations; mounting them needs the desktop's own
/// network service, so the UI only explains where mounted shares appear.
pub fn is_network_uri(s: &str) -> bool {
    ["smb://", "sftp://", "ssh://", "ftp://", "ftps://", "dav://", "davs://", "afp://", "nfs://", "mtp://", "webdav://"]
        .iter()
        .any(|p| s.starts_with(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn val<'a>(v: impl Into<Value<'a>>) -> OwnedValue {
        v.into().try_to_owned().unwrap()
    }

    fn dev(s: &str) -> OwnedValue {
        let mut v = s.as_bytes().to_vec();
        v.push(0);
        val(v)
    }

    #[test]
    fn picks_unmounted_filesystems() {
        let mut objs: Objects = HashMap::new();
        let drive_path = OwnedObjectPath::try_from("/org/freedesktop/UDisks2/drives/USB").unwrap();
        objs.insert(drive_path.clone(), HashMap::from([(DRIVE.to_string(), HashMap::from([("Removable".to_string(), val(true))]))]));
        let block = |label: &str, device: &str, usage: &str, system: bool| -> Props {
            HashMap::from([
                ("IdLabel".to_string(), val(label)),
                ("IdType".to_string(), val("exfat")),
                ("IdUsage".to_string(), val(usage)),
                ("Size".to_string(), val(16_000_000_000u64)),
                ("HintIgnore".to_string(), val(false)),
                ("HintSystem".to_string(), val(system)),
                ("HintName".to_string(), val("")),
                ("PreferredDevice".to_string(), dev(device)),
                ("Device".to_string(), dev(device)),
                ("Drive".to_string(), val(drive_path.clone())),
            ])
        };
        let empty: Vec<Vec<u8>> = vec![];
        let mounted: Vec<Vec<u8>> = vec![b"/media/x\0".to_vec()];
        let fs = |m: &Vec<Vec<u8>>| HashMap::from([("MountPoints".to_string(), val(m.clone()))]);
        objs.insert(
            OwnedObjectPath::try_from("/org/freedesktop/UDisks2/block_devices/sdb1").unwrap(),
            HashMap::from([(BLOCK.to_string(), block("USB STICK", "/dev/sdb1", "filesystem", false)), (FS.to_string(), fs(&empty))]),
        );
        objs.insert(
            OwnedObjectPath::try_from("/org/freedesktop/UDisks2/block_devices/sda1").unwrap(),
            HashMap::from([(BLOCK.to_string(), block("Boot", "/dev/sda1", "filesystem", true)), (FS.to_string(), fs(&mounted))]),
        );
        objs.insert(
            OwnedObjectPath::try_from("/org/freedesktop/UDisks2/block_devices/sda2").unwrap(),
            HashMap::from([(BLOCK.to_string(), block("swap", "/dev/sda2", "other", true)), (FS.to_string(), fs(&empty))]),
        );
        let v = volumes_from(&objs);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].label, "USB STICK");
        assert_eq!(v[0].device, "/dev/sdb1");
        assert!(v[0].removable);
        let (path, drive) = block_for_device(&objs, "/dev/sdb1").unwrap();
        assert!(path.as_str().ends_with("sdb1"));
        assert_eq!(drive.unwrap(), drive_path);
    }

    #[test]
    fn uris() {
        assert!(is_network_uri("smb://nas/share"));
        assert!(!is_network_uri("/home/me"));
    }
}
