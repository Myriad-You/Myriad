//! Which bytes already sit on a Space, and where: a file a call answered with
//! (a decomposition, the baked PSD, the keys) or one uploaded before is named
//! by its path there instead of being sent up again. The host's uplink is a
//! megabit or two; a decomposition is over ten megabytes and goes into several
//! calls.
//!
//! Kept for the process, across clients (each job builds its own). A Space's
//! files are gone when it restarts: a caller asks the Space whether the file
//! is still there before naming it (`found`), and uploads when it is not.

use std::{collections::VecDeque, sync::Mutex};

use reqwest::{StatusCode, Url};
use sha2::{Digest, Sha256};

use super::validate_remote_temp_path;

/// Files remembered, over all Spaces; past it the oldest are forgotten. A
/// full-body job names about fifty.
const MAX_FILES: usize = 512;

/// What makes two files the same to a Space's endpoint: the Space, the bytes,
/// and the name's extension (a path is read by its suffix there, so a file is
/// only ever named by a path ending as its upload would).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct FileKey {
    space: String,
    digest: [u8; 32],
    extension: String,
}

impl FileKey {
    /// `name`: the file's name or path; only its extension counts.
    pub(super) fn new(space: &str, bytes: &[u8], name: &str) -> Self {
        Self {
            space: space.to_string(),
            digest: Sha256::digest(bytes).into(),
            extension: extension_of(name),
        }
    }
}

/// The extension of a file name or path, lower case; empty when it has none.
fn extension_of(name: &str) -> String {
    let file = name.rsplit('/').next().unwrap_or(name);
    file.rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase())
        .unwrap_or_default()
}

/// Paths on Spaces by what is at them, oldest first.
pub(super) struct RemoteFiles {
    limit: usize,
    known: VecDeque<(FileKey, String)>,
}

impl RemoteFiles {
    pub(super) const fn new(limit: usize) -> Self {
        Self {
            limit,
            known: VecDeque::new(),
        }
    }

    /// Remembers that `key`'s bytes are at `path`. A path that is not one of
    /// Gradio's temporary files, or whose extension is not the key's, is not
    /// kept. Whether it was.
    pub(super) fn remember(&mut self, key: FileKey, path: &str) -> bool {
        if validate_remote_temp_path(path).is_err() || extension_of(path) != key.extension {
            return false;
        }
        self.known.retain(|(known, _)| *known != key);
        self.known.push_back((key, path.to_string()));
        while self.known.len() > self.limit {
            self.known.pop_front();
        }
        true
    }

    pub(super) fn path(&self, key: &FileKey) -> Option<String> {
        self.known
            .iter()
            .find(|(known, _)| known == key)
            .map(|(_, path)| path.clone())
    }

    /// Forgets `key` while it still names `path` (a newer path stays).
    pub(super) fn forget(&mut self, key: &FileKey, path: &str) {
        self.known
            .retain(|(known, at)| !(known == key && at == path));
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.known.len()
    }
}

static FILES: Mutex<RemoteFiles> = Mutex::new(RemoteFiles::new(MAX_FILES));

fn files() -> std::sync::MutexGuard<'static, RemoteFiles> {
    FILES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub(super) fn remember(key: FileKey, path: &str) -> bool {
    files().remember(key, path)
}

pub(super) fn path(key: &FileKey) -> Option<String> {
    files().path(key)
}

pub(super) fn forget(key: &FileKey, path: &str) {
    files().forget(key, path)
}

/// A file's path on its Space, from the URL it is downloaded at
/// (`…/gradio_api/file=<path>`): for an answer that gives a URL and no path.
pub(super) fn path_of_url(url: &Url) -> Option<String> {
    let encoded = url.path().strip_prefix("/gradio_api/file=")?;
    let path = urlencoding::decode(encoded).ok()?;
    validate_remote_temp_path(&path).ok()?;
    Some(path.into_owned())
}

/// Whether the Space's answer to a one-byte ranged GET of a file says the file
/// is there with `length` bytes. 206 with the whole size in `Content-Range`,
/// or 200 with `Content-Length` (a server that ignores ranges). An answer that
/// tells no size is not a yes: sending the file again costs an upload, naming
/// a file that is not there costs the call.
pub(super) fn found(
    status: StatusCode,
    content_range: Option<&str>,
    content_length: Option<&str>,
    length: u64,
) -> bool {
    let told = match status {
        StatusCode::PARTIAL_CONTENT => {
            content_range.and_then(|range| range.rsplit_once('/').map(|(_, total)| total.trim()))
        }
        StatusCode::OK => content_length.map(str::trim),
        _ => return false,
    };
    told.is_some_and(|total| total.parse::<u64>().is_ok_and(|total| total == length))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPACE: &str = "owner/space";

    fn key(bytes: &[u8]) -> FileKey {
        FileKey::new(SPACE, bytes, "decomposition-0.psd")
    }

    #[test]
    fn bytes_seen_on_a_space_are_found_again_by_content() {
        let mut files = RemoteFiles::new(8);
        assert!(files.path(&key(b"front")).is_none());
        assert!(files.remember(key(b"front"), "/tmp/gradio/a1/output.psd"));
        // The same bytes under another name of the same kind: the same file.
        let again = FileKey::new(SPACE, b"front", "figure.psd");
        assert_eq!(
            files.path(&again).as_deref(),
            Some("/tmp/gradio/a1/output.psd")
        );
        // Other bytes, another Space, another kind of file: not it.
        assert!(files.path(&key(b"turned")).is_none());
        assert!(
            files
                .path(&FileKey::new("owner/other", b"front", "a.psd"))
                .is_none()
        );
        assert!(
            files
                .path(&FileKey::new(SPACE, b"front", "a.png"))
                .is_none()
        );
        // Seen again elsewhere: the newer path, once.
        assert!(files.remember(key(b"front"), "/tmp/gradio/b2/front.psd"));
        assert_eq!(
            files.path(&key(b"front")).as_deref(),
            Some("/tmp/gradio/b2/front.psd")
        );
        assert_eq!(files.len(), 1);
    }

    #[test]
    fn only_gradio_temporary_files_of_the_same_kind_are_remembered() {
        let mut files = RemoteFiles::new(8);
        for unsafe_path in [
            "/etc/passwd",
            "/tmp/gradio/../secret.psd",
            "relative/output.psd",
            "",
        ] {
            assert!(!files.remember(key(b"front"), unsafe_path), "{unsafe_path}");
        }
        // A path the endpoint would read as another kind of file.
        assert!(!files.remember(key(b"front"), "/tmp/gradio/a1/output.png"));
        assert!(!files.remember(key(b"front"), "/tmp/gradio/a1/output"));
        assert_eq!(files.len(), 0);
        assert!(files.remember(key(b"front"), "/tmp/gradio/a1/Output.PSD"));
    }

    #[test]
    fn the_oldest_are_forgotten_past_the_limit() {
        let mut files = RemoteFiles::new(3);
        for n in 0..5u8 {
            assert!(files.remember(key(&[n]), &format!("/tmp/gradio/{n}/output.psd")));
        }
        assert_eq!(files.len(), 3);
        assert!(files.path(&key(&[0])).is_none());
        assert!(files.path(&key(&[1])).is_none());
        assert_eq!(
            files.path(&key(&[4])).as_deref(),
            Some("/tmp/gradio/4/output.psd")
        );
        // Seen again, a file is as new as the last time it was seen.
        assert!(files.remember(key(&[2]), "/tmp/gradio/2/output.psd"));
        assert!(files.remember(key(&[5]), "/tmp/gradio/5/output.psd"));
        assert!(files.path(&key(&[2])).is_some());
        assert!(files.path(&key(&[3])).is_none());
    }

    #[test]
    fn a_file_gone_from_the_space_is_forgotten_unless_seen_anew() {
        let mut files = RemoteFiles::new(8);
        files.remember(key(b"front"), "/tmp/gradio/a1/output.psd");
        files.forget(&key(b"front"), "/tmp/gradio/other/output.psd");
        assert!(files.path(&key(b"front")).is_some());
        files.forget(&key(b"front"), "/tmp/gradio/a1/output.psd");
        assert!(files.path(&key(b"front")).is_none());
    }

    #[test]
    fn the_process_keeps_one_list_for_every_client() {
        let key = FileKey::new("remote-files-test/space", b"shared", "keyforms.json");
        assert!(remember(key.clone(), "/tmp/gradio/k/keyforms.json"));
        assert_eq!(path(&key).as_deref(), Some("/tmp/gradio/k/keyforms.json"));
        forget(&key, "/tmp/gradio/k/keyforms.json");
        assert!(path(&key).is_none());
    }

    #[test]
    fn a_download_url_tells_the_path_on_the_space() {
        let url = |text: &str| Url::parse(text).unwrap();
        assert_eq!(
            path_of_url(&url(
                "https://owner-space.hf.space/gradio_api/file=/tmp/gradio/a1/output.psd"
            ))
            .as_deref(),
            Some("/tmp/gradio/a1/output.psd")
        );
        // Encoded as a URL writes it.
        assert_eq!(
            path_of_url(&url(
                "https://owner-space.hf.space/gradio_api/file=/tmp/gradio/a1/my%20figure.psd"
            ))
            .as_deref(),
            Some("/tmp/gradio/a1/my figure.psd")
        );
        for none in [
            "https://owner-space.hf.space/gradio_api/file=/etc/passwd",
            "https://owner-space.hf.space/gradio_api/file=/tmp/gradio/%2E%2E/secret",
            "https://owner-space.hf.space/file=/tmp/gradio/a1/output.psd",
            "https://owner-space.hf.space/gradio_api/file=",
        ] {
            assert!(path_of_url(&url(none)).is_none(), "{none}");
        }
    }

    #[test]
    fn a_ranged_answer_tells_whether_the_file_is_still_there() {
        use StatusCode as S;
        assert!(found(
            S::PARTIAL_CONTENT,
            Some("bytes 0-0/1200"),
            Some("1"),
            1200
        ));
        // A server that ignores the range answers with the whole file.
        assert!(found(S::OK, None, Some("1200"), 1200));
        // No size told: not known to be the file.
        assert!(!found(S::PARTIAL_CONTENT, Some("bytes 0-0/*"), None, 1200));
        assert!(!found(S::PARTIAL_CONTENT, None, Some("1"), 1200));
        assert!(!found(S::OK, None, None, 1200));
        // Another file at the path: not the one remembered.
        assert!(!found(
            S::PARTIAL_CONTENT,
            Some("bytes 0-0/1199"),
            None,
            1200
        ));
        assert!(!found(S::OK, None, Some("7"), 1200));
        assert!(!found(S::OK, None, Some("many"), 1200));
        // Gone with a restart, refused, moved, or the Space is not answering.
        for status in [
            S::NOT_FOUND,
            S::FORBIDDEN,
            S::FOUND,
            S::RANGE_NOT_SATISFIABLE,
            S::SERVICE_UNAVAILABLE,
        ] {
            assert!(!found(status, Some("bytes 0-0/1200"), Some("1200"), 1200));
        }
    }
}
