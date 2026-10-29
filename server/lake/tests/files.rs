//! A volume is a folder, and a caller's path never leaves it.
#![allow(clippy::unwrap_used)]

use loamz_lake::files::{FileError, Volumes};
use tokio::io::AsyncReadExt;

fn volumes() -> (Volumes, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "lake-files-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    (Volumes::new(&dir), dir)
}

fn rand_suffix() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos()
}

async fn put(v: &Volumes, vol: &str, rel: &str, body: &[u8]) -> Result<u64, FileError> {
    let mut up = v.begin_put(vol, rel, 1024).await?;
    up.write(body).await?;
    up.commit().await
}

async fn read(v: &Volumes, vol: &str, rel: &str) -> String {
    let (mut f, _) = v.open(vol, rel).await.unwrap();
    let mut s = String::new();
    f.read_to_string(&mut s).await.unwrap();
    s
}

#[tokio::test]
async fn a_file_can_be_written_read_listed_replaced_and_deleted() {
    let (v, _dir) = volumes();
    assert_eq!(put(&v, "vol-1", "a/b.txt", b"hello").await.unwrap(), 5);
    assert_eq!(read(&v, "vol-1", "a/b.txt").await, "hello");
    put(&v, "vol-1", "a/b.txt", b"hi").await.unwrap();
    assert_eq!(read(&v, "vol-1", "a/b.txt").await, "hi");
    put(&v, "vol-1", "top.txt", b"x").await.unwrap();
    let names: Vec<_> = v
        .list("vol-1")
        .await
        .unwrap()
        .into_iter()
        .map(|e| e.path)
        .collect();
    assert_eq!(names, ["a/b.txt", "top.txt"]);
    v.delete("vol-1", "a/b.txt").await.unwrap();
    assert!(matches!(
        v.open("vol-1", "a/b.txt").await,
        Err(FileError::NotFound)
    ));
}

#[tokio::test]
async fn paths_that_could_leave_the_volume_are_refused() {
    let (v, _dir) = volumes();
    for bad in [
        "../x",
        "a/../x",
        "/abs",
        "a//b",
        "a\\b",
        "",
        ".",
        "a/.upload-1",
        "a/\u{0}b",
    ] {
        assert!(
            matches!(
                put(&v, "vol-1", bad, b"x").await,
                Err(FileError::BadPath(_))
            ),
            "{bad:?} was accepted"
        );
    }
    for bad in ["../v", "a/b", ""] {
        assert!(
            matches!(put(&v, bad, "f", b"x").await, Err(FileError::BadPath(_))),
            "volume {bad:?} was accepted"
        );
    }
}

#[tokio::test]
async fn a_symlink_planted_in_a_volume_is_never_followed() {
    let (v, dir) = volumes();
    put(&v, "vol-1", "ok.txt", b"fine").await.unwrap();
    let secret = dir.join("secret.txt");
    std::fs::write(&secret, "secret").unwrap();
    std::os::unix::fs::symlink(&secret, dir.join("vol-1").join("link")).unwrap();
    std::os::unix::fs::symlink(&dir, dir.join("vol-1").join("dirlink")).unwrap();
    assert!(matches!(
        v.open("vol-1", "link").await,
        Err(FileError::BadPath(_))
    ));
    assert!(matches!(
        v.open("vol-1", "dirlink/secret.txt").await,
        Err(FileError::BadPath(_))
    ));
    assert!(matches!(
        put(&v, "vol-1", "dirlink/new.txt", b"x").await,
        Err(FileError::BadPath(_))
    ));
    assert!(matches!(
        put(&v, "vol-1", "link", b"x").await,
        Err(FileError::BadPath(_))
    ));
    let names: Vec<_> = v
        .list("vol-1")
        .await
        .unwrap()
        .into_iter()
        .map(|e| e.path)
        .collect();
    assert_eq!(names, ["ok.txt"], "links must not be listed");
    assert_eq!(std::fs::read_to_string(&secret).unwrap(), "secret");
}

#[tokio::test]
async fn an_upload_over_the_limit_leaves_nothing_behind() {
    let (v, dir) = volumes();
    let mut up = v.begin_put("vol-1", "big.bin", 4).await.unwrap();
    up.write(b"123").await.unwrap();
    assert!(matches!(up.write(b"45").await, Err(FileError::TooLarge)));
    drop(up);
    assert!(v.list("vol-1").await.unwrap().is_empty());
    assert_eq!(
        std::fs::read_dir(dir.join("vol-1")).unwrap().count(),
        0,
        "temporary file left behind"
    );
}

#[tokio::test]
async fn removing_a_volume_removes_every_file_and_can_be_repeated() {
    let (v, _dir) = volumes();
    put(&v, "vol-1", "a/b/c.txt", b"x").await.unwrap();
    v.remove_volume("vol-1").await.unwrap();
    assert!(!v.exists("vol-1").await.unwrap());
    v.remove_volume("vol-1").await.unwrap();
}
