//! Which bytes already sit on a Space, and where: a file a call answered with
//! (a decomposition, the baked PSD, the keys) or one uploaded before is named
//! by its path there instead of being sent up again. The host's uplink is a
//! megabit or two; a decomposition is over ten megabytes and goes into several
//! calls.
//!
//! Kept across clients (each job builds its own) and across restarts of the
//! backend: the list is written to a file under the data directory and read
//! back when the process first uses it, so a job run again after a deploy
//! names what its last run left on the Space. The file holds, for each entry,
//! the Space, the digest of the bytes, the extension and the path; never the
//! bytes, a credential or a header.
//!
//! A Space's files are gone when it restarts: a caller asks the Space whether
//! the file is still there before naming it (`found`), and uploads when it is
//! not. A path read back from the file is asked about like any other.

use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, OnceLock},
    time::Duration,
};

use reqwest::{StatusCode, Url};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::validate_remote_temp_path;

/// Files remembered, over all Spaces; past it the oldest are forgotten. A
/// full-body job names about fifty.
const MAX_FILES: usize = 512;
/// The list's file, under the data directory.
const KEPT_FILE: &str = "merope/see-through/remote-files.json";
/// The layout of the list's file; a file of another version reads as empty.
const KEPT_VERSION: u64 = 1;
/// More than a list's file comes to (`MAX_FILES` entries of the paths Gradio
/// gives are about a hundred kilobytes); a larger file is not read.
const MAX_KEPT_BYTES: u64 = 4 * 1024 * 1024;
/// How long a change waits to be written, so that the changes of one burst (a
/// job's answers arriving together) make one write.
const WRITE_AFTER: Duration = Duration::from_secs(1);

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
#[derive(Clone)]
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

    /// The list as its file holds it: for each entry the Space, the bytes'
    /// SHA-256 in hex, the extension and the path, oldest first.
    fn as_kept(&self) -> Vec<u8> {
        let files: Vec<Value> = self
            .known
            .iter()
            .map(|(key, path)| {
                json!({
                    "space": key.space,
                    "sha256": hex::encode(key.digest),
                    "extension": key.extension,
                    "path": path,
                })
            })
            .collect();
        json!({ "version": KEPT_VERSION, "files": files })
            .to_string()
            .into_bytes()
    }

    /// The list a file holds. Each entry is remembered as one just seen would
    /// be, so one whose path or extension would not be kept is left out, and
    /// past the limit the oldest are. A path with a character a URL reads
    /// its own way (`%`, `?`, `#`) is left out too: asked about on the Space
    /// it would be another path than the one named to a call. Bytes that are
    /// not a list of this version are an empty list.
    fn from_kept(limit: usize, bytes: &[u8]) -> Self {
        let mut files = Self::new(limit);
        let Ok(listed) = serde_json::from_slice::<Value>(bytes) else {
            return files;
        };
        if listed.get("version").and_then(Value::as_u64) != Some(KEPT_VERSION) {
            return files;
        }
        let entries = listed.get("files").and_then(Value::as_array);
        for entry in entries.into_iter().flatten() {
            let text = |name: &str| entry.get(name).and_then(Value::as_str);
            let (Some(space), Some(sha256), Some(extension), Some(path)) = (
                text("space"),
                text("sha256"),
                text("extension"),
                text("path"),
            ) else {
                continue;
            };
            let mut digest = [0u8; 32];
            if hex::decode_to_slice(sha256, &mut digest).is_err() || path.contains(['%', '?', '#'])
            {
                continue;
            }
            let key = FileKey {
                space: space.to_string(),
                digest,
                extension: extension.to_string(),
            };
            files.remember(key, path);
        }
        files
    }

    fn len(&self) -> usize {
        self.known.len()
    }
}

/// A list and the file it is kept in.
struct Kept {
    /// None: in memory only.
    file: Option<PathBuf>,
    /// How long a change waits to be written with the ones just after it.
    write_after: Duration,
    state: Mutex<State>,
}

struct State {
    files: RemoteFiles,
    /// Changed since the list was last taken to be written.
    unwritten: bool,
    /// A task is writing the file or waiting to; it writes what changes meanwhile.
    writing: bool,
}

/// Held by the task that writes a list. A task dropped before it is done (its
/// runtime shut down) gives the turn back, so the next change starts another.
struct Writing {
    kept: Arc<Kept>,
    done: bool,
}

impl Writing {
    /// The list is written and the turn already given back.
    fn finished(mut self) {
        self.done = true;
    }
}

impl Drop for Writing {
    fn drop(&mut self) {
        if !self.done {
            self.kept.state().writing = false;
        }
    }
}

impl Kept {
    /// The list `file` holds, or an empty one: there is no file before the
    /// first write, and one that cannot be read or is not a list only costs
    /// the uploads it would have saved.
    fn open(file: Option<PathBuf>, limit: usize, write_after: Duration) -> Arc<Self> {
        let files = match file.as_deref().and_then(read_kept) {
            Some(bytes) => RemoteFiles::from_kept(limit, &bytes),
            None => RemoteFiles::new(limit),
        };
        if !files.known.is_empty() {
            tracing::info!(
                files = files.len(),
                "read back which files were on See-through Spaces"
            );
        }
        Arc::new(Self {
            file,
            write_after,
            state: Mutex::new(State {
                files,
                unwritten: false,
                writing: false,
            }),
        })
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn remember(self: &Arc<Self>, key: FileKey, path: &str) -> bool {
        let mut state = self.state();
        let remembered = state.files.remember(key, path);
        if remembered {
            self.changed(state);
        }
        remembered
    }

    fn path(&self, key: &FileKey) -> Option<String> {
        self.state().files.path(key)
    }

    fn forget(self: &Arc<Self>, key: &FileKey, path: &str) {
        let mut state = self.state();
        let before = state.files.len();
        state.files.forget(key, path);
        if state.files.len() != before {
            self.changed(state);
        }
    }

    /// Sees that a change reaches the file: by the task already writing, or
    /// by one started here. The caller does not wait for the disk, and the
    /// lock is let go before the task is started. Outside a runtime no task
    /// is started: the change stays in memory and is written with the next
    /// one made inside a runtime.
    fn changed(self: &Arc<Self>, mut state: MutexGuard<'_, State>) {
        state.unwritten = true;
        if state.writing {
            return;
        }
        let Some(file) = self.file.clone() else {
            return;
        };
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        state.writing = true;
        drop(state);
        let writing = Writing {
            kept: Arc::clone(self),
            done: false,
        };
        runtime.spawn(async move {
            writing.kept.write(file).await;
            writing.finished();
        });
    }

    /// Writes the list until the file holds it as it stands, then gives the
    /// turn back. A write that fails is left at that until the next change:
    /// the process goes by its memory, and what the file lacks after a
    /// restart is uploaded again.
    async fn write(&self, file: PathBuf) {
        loop {
            tokio::time::sleep(self.write_after).await;
            let files = {
                let mut state = self.state();
                state.unwritten = false;
                state.files.clone()
            };
            let to = file.clone();
            let written = tokio::task::spawn_blocking(move || write_whole(&to, &files.as_kept()))
                .await
                .unwrap_or_else(|lost| Err(std::io::Error::other(lost)));
            if let Err(error) = written {
                tracing::warn!(
                    %error,
                    file = %file.display(),
                    "the list of files on See-through Spaces was not written"
                );
            }
            let mut state = self.state();
            if !state.unwritten {
                state.writing = false;
                return;
            }
        }
    }
}

/// What the list's file holds; None when there is no reading it.
fn read_kept(file: &Path) -> Option<Vec<u8>> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(file)
        .ok()?
        .take(MAX_KEPT_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    (bytes.len() as u64 <= MAX_KEPT_BYTES).then_some(bytes)
}

/// Written whole or not at all: a write cut short leaves the file as it was.
fn write_whole(file: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let partial = file.with_extension("json.partial");
    let mut out = std::fs::File::create(&partial)?;
    out.write_all(bytes)?;
    out.sync_all()?;
    std::fs::rename(&partial, file)
}

fn kept_file() -> PathBuf {
    crate::services::data_paths::paths().root.join(KEPT_FILE)
}

/// The process's list, read from its file by the first call here: one small
/// read, once. Tests of other things share this list, and the data directory
/// is not theirs to write: theirs is in memory only.
fn files() -> &'static Arc<Kept> {
    static KEPT: OnceLock<Arc<Kept>> = OnceLock::new();
    KEPT.get_or_init(|| Kept::open((!cfg!(test)).then(kept_file), MAX_FILES, WRITE_AFTER))
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

/// One upload at a time, for the process. What an upload waits on is the
/// host's uplink: files sent together only share it, each taking as many
/// times longer and running into its request's time limit (four
/// decompositions at once on a megabit do). The wait here is not counted
/// against that limit. Calls sent together that carry the same bytes (a job's
/// four judgements each carry its front decomposition) also send them once:
/// the ones that waited find them on the Space.
pub(super) fn uploading() -> &'static tokio::sync::Mutex<()> {
    static UPLOADING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    &UPLOADING
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

    fn sha256(bytes: &[u8]) -> String {
        hex::encode(Sha256::digest(bytes))
    }

    /// A directory of its own for one test's file, removed with the test.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Self {
            Self(std::env::temp_dir().join(format!(
                "remote-files-test-{}",
                uuid::Uuid::new_v4().simple()
            )))
        }

        fn file(&self) -> PathBuf {
            self.0.join(KEPT_FILE)
        }

        /// A list kept in this directory, as a process starting would open it.
        fn open(&self, limit: usize) -> Arc<Kept> {
            Kept::open(Some(self.file()), limit, Duration::ZERO)
        }

        fn put(&self, content: &[u8]) {
            std::fs::create_dir_all(self.file().parent().unwrap()).unwrap();
            std::fs::write(self.file(), content).unwrap();
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Waits until the file holds the list as it stands.
    async fn written(kept: &Kept) {
        for _ in 0..4000 {
            {
                let state = kept.state();
                if !state.unwritten && !state.writing {
                    return;
                }
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        panic!("the list was not written");
    }

    fn a_runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap()
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
    fn the_processes_list_is_kept_under_the_data_directory() {
        let file = kept_file();
        assert!(file.starts_with(crate::services::data_paths::paths().root.as_path()));
        assert!(file.ends_with("merope/see-through/remote-files.json"));
        // The list the tests share is not written there.
        assert!(files().file.is_none());
    }

    #[tokio::test]
    async fn the_list_comes_back_from_its_file_oldest_first() {
        let scratch = Scratch::new();
        let kept = scratch.open(3);
        for n in 0..5u8 {
            assert!(kept.remember(key(&[n]), &format!("/tmp/gradio/{n}/output.psd")));
        }
        written(&kept).await;

        // A process starting: the three newest, in the order they were seen.
        let again = scratch.open(3);
        let known = |kept: &Kept| kept.state().files.known.clone();
        assert_eq!(known(&again), known(&kept));
        assert!(again.path(&key(&[0])).is_none());
        assert!(again.path(&key(&[1])).is_none());
        for n in 2..5u8 {
            assert_eq!(
                again.path(&key(&[n])),
                Some(format!("/tmp/gradio/{n}/output.psd"))
            );
        }
        // One more pushes out the oldest of those read back.
        assert!(again.remember(key(&[5]), "/tmp/gradio/5/output.psd"));
        assert!(again.path(&key(&[2])).is_none());
        assert!(again.path(&key(&[3])).is_some());
        written(&again).await;

        // A list with less room keeps the newest of what the file holds.
        let fewer = scratch.open(2);
        assert_eq!(fewer.state().files.len(), 2);
        assert!(fewer.path(&key(&[3])).is_none());
        assert!(fewer.path(&key(&[4])).is_some());
        assert!(fewer.path(&key(&[5])).is_some());
    }

    #[tokio::test]
    async fn a_file_forgotten_is_gone_from_the_lists_file() {
        let scratch = Scratch::new();
        let kept = scratch.open(8);
        assert!(kept.remember(key(b"front"), "/tmp/gradio/a1/front.psd"));
        assert!(kept.remember(key(b"turned"), "/tmp/gradio/b2/turned.psd"));
        written(&kept).await;
        let text = std::fs::read_to_string(scratch.file()).unwrap();
        assert!(text.contains("/tmp/gradio/a1/front.psd"));
        assert!(text.contains(&sha256(b"front")));

        // Forgetting what the list does not hold changes nothing to write.
        kept.forget(&key(b"front"), "/tmp/gradio/other/front.psd");
        assert!(!kept.state().unwritten);

        kept.forget(&key(b"front"), "/tmp/gradio/a1/front.psd");
        written(&kept).await;
        let text = std::fs::read_to_string(scratch.file()).unwrap();
        assert!(!text.contains("/tmp/gradio/a1/front.psd"));
        assert!(!text.contains(&sha256(b"front")));
        assert!(text.contains("/tmp/gradio/b2/turned.psd"));

        let again = scratch.open(8);
        assert!(again.path(&key(b"front")).is_none());
        assert_eq!(
            again.path(&key(b"turned")).as_deref(),
            Some("/tmp/gradio/b2/turned.psd")
        );
    }

    #[tokio::test]
    async fn the_file_holds_digests_and_paths_and_no_bytes() {
        let scratch = Scratch::new();
        let kept = scratch.open(8);
        let pixels = b"PIXELS-OF-THE-FIGURE-0123456789-abcdefghijklmnopqrstuvwxyz";
        let figure = FileKey::new(SPACE, pixels, "character.png");
        assert!(kept.remember(figure, "/tmp/gradio/c3/character.png"));
        written(&kept).await;

        let bytes = std::fs::read(scratch.file()).unwrap();
        assert!(
            !bytes
                .windows(8)
                .any(|window| pixels.windows(8).any(|part| part == window))
        );
        let listed: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            listed,
            json!({
                "version": 1,
                "files": [{
                    "space": SPACE,
                    "sha256": sha256(pixels),
                    "extension": "png",
                    "path": "/tmp/gradio/c3/character.png",
                }],
            })
        );
        // Nothing is left beside it.
        let beside: Vec<_> = std::fs::read_dir(scratch.file().parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(beside, ["remote-files.json"]);
    }

    #[test]
    fn a_file_that_is_not_a_list_reads_as_an_empty_one() {
        let scratch = Scratch::new();
        // No file: a first start.
        assert_eq!(scratch.open(8).state().files.len(), 0);

        let entry = json!({
            "space": SPACE,
            "sha256": sha256(b"front"),
            "extension": "psd",
            "path": "/tmp/gradio/a1/front.psd",
        });
        let listed = json!({ "version": 1, "files": [entry] }).to_string();
        scratch.put(listed.as_bytes());
        assert_eq!(
            scratch.open(8).path(&key(b"front")).as_deref(),
            Some("/tmp/gradio/a1/front.psd")
        );

        // Larger than a list can be, though it would parse.
        let mut padded = listed.clone().into_bytes();
        padded.resize(MAX_KEPT_BYTES as usize + 1, b' ');
        let text = |value: Value| value.to_string().into_bytes();
        let not_lists: Vec<Vec<u8>> = vec![
            Vec::new(),
            b"\xff\xfe\x00 not text".to_vec(),
            b"not json".to_vec(),
            listed.as_bytes()[..listed.len() / 2].to_vec(),
            text(json!(null)),
            text(json!([])),
            text(json!({ "version": 1 })),
            text(json!({ "version": 1, "files": "none" })),
            text(json!({ "files": [entry] })),
            text(json!({ "version": "1", "files": [entry] })),
            text(json!({ "version": 2, "files": [entry] })),
            text(json!({ "version": 0, "files": [entry] })),
            padded,
        ];
        for content in &not_lists {
            scratch.put(content);
            let kept = scratch.open(8);
            let shown = String::from_utf8_lossy(&content[..content.len().min(60)]);
            assert_eq!(kept.state().files.len(), 0, "{shown}");
            // The list works on from empty.
            assert!(kept.remember(key(b"turned"), "/tmp/gradio/b2/turned.psd"));
            assert!(kept.path(&key(b"turned")).is_some());
        }

        // Something that is not a file where the file goes.
        std::fs::remove_file(scratch.file()).unwrap();
        std::fs::create_dir_all(scratch.file()).unwrap();
        assert_eq!(scratch.open(8).state().files.len(), 0);
    }

    #[test]
    fn an_entry_that_would_not_be_remembered_is_left_out() {
        let entry = |bytes: &[u8], extension: &str, path: &str| {
            json!({
                "space": SPACE,
                "sha256": sha256(bytes),
                "extension": extension,
                "path": path,
            })
        };
        let odd = |sha256: Value, path: Value| {
            json!({
                "space": SPACE,
                "sha256": sha256,
                "extension": "psd",
                "path": path,
            })
        };
        let listed = json!({
            "version": 1,
            "files": [
                entry(b"front", "psd", "/tmp/gradio/a1/front.psd"),
                // Not one of Gradio's temporary files.
                entry(b"1", "psd", "/etc/secret.psd"),
                entry(b"2", "psd", "/tmp/gradio/../secret.psd"),
                entry(b"3", "psd", "relative/output.psd"),
                entry(b"4", "psd", ""),
                // A path a URL would read as another one.
                entry(b"16", "psd", "/tmp/gradio/%2e%2e/%2e%2e/etc/x.psd"),
                entry(b"17", "psd", "/tmp/gradio/a1/output?x=.psd"),
                entry(b"18", "psd", "/tmp/gradio/a1/output#.psd"),
                // A path the endpoint would read as another kind of file.
                entry(b"5", "psd", "/tmp/gradio/a1/output.png"),
                entry(b"6", "psd", "/tmp/gradio/a1/output"),
                entry(b"7", "PSD", "/tmp/gradio/a1/output.psd"),
                // Not a SHA-256.
                odd(json!("abc123"), json!("/tmp/gradio/8/output.psd")),
                odd(json!("z".repeat(64)), json!("/tmp/gradio/9/output.psd")),
                odd(json!(sha256(b"10") + "00"), json!("/tmp/gradio/10/output.psd")),
                // Not an entry.
                { "space": SPACE, "sha256": sha256(b"11"), "extension": "psd" },
                odd(json!(12), json!("/tmp/gradio/12/output.psd")),
                odd(json!(sha256(b"13")), json!(["/tmp/gradio/13/output.psd"])),
                "/tmp/gradio/14/output.psd",
                15,
                null,
                [],
                entry(b"keys", "json", "/tmp/gradio/k/keyforms.json"),
            ],
        });
        let files = RemoteFiles::from_kept(8, listed.to_string().as_bytes());
        let paths: Vec<&str> = files.known.iter().map(|(_, path)| path.as_str()).collect();
        assert_eq!(
            paths,
            ["/tmp/gradio/a1/front.psd", "/tmp/gradio/k/keyforms.json"]
        );
        assert_eq!(
            files.path(&key(b"front")).as_deref(),
            Some("/tmp/gradio/a1/front.psd")
        );
        assert_eq!(
            files
                .path(&FileKey::new(SPACE, b"keys", "keyforms.json"))
                .as_deref(),
            Some("/tmp/gradio/k/keyforms.json")
        );
        // What a list writes, it reads back the same.
        assert_eq!(
            RemoteFiles::from_kept(8, &files.as_kept()).known,
            files.known
        );
    }

    #[test]
    fn outside_a_runtime_a_change_stays_in_memory_until_the_next_inside_one() {
        let scratch = Scratch::new();
        let kept = scratch.open(8);
        assert!(kept.remember(key(b"front"), "/tmp/gradio/a1/front.psd"));
        kept.forget(&key(b"front"), "/tmp/gradio/a1/front.psd");
        assert!(kept.remember(key(b"front"), "/tmp/gradio/a2/front.psd"));
        assert_eq!(
            kept.path(&key(b"front")).as_deref(),
            Some("/tmp/gradio/a2/front.psd")
        );
        assert!(!scratch.file().exists());

        a_runtime().block_on(async {
            assert!(kept.remember(key(b"turned"), "/tmp/gradio/b2/turned.psd"));
            written(&kept).await;
        });
        let again = scratch.open(8);
        assert_eq!(
            again.path(&key(b"front")).as_deref(),
            Some("/tmp/gradio/a2/front.psd")
        );
        assert!(again.path(&key(b"turned")).is_some());
    }

    #[test]
    fn a_write_lost_with_its_runtime_is_made_after_the_next_change() {
        let scratch = Scratch::new();
        let kept = Kept::open(Some(scratch.file()), 8, Duration::from_millis(100));
        let first = a_runtime();
        first.block_on(async {
            assert!(kept.remember(key(b"front"), "/tmp/gradio/a1/front.psd"));
        });
        assert!(kept.state().writing);
        // Shut down while the write waits its turn.
        drop(first);
        assert!(!kept.state().writing);
        assert!(kept.state().unwritten);
        assert!(!scratch.file().exists());

        a_runtime().block_on(async {
            assert!(kept.remember(key(b"turned"), "/tmp/gradio/b2/turned.psd"));
            written(&kept).await;
        });
        let again = scratch.open(8);
        assert!(again.path(&key(b"front")).is_some());
        assert!(again.path(&key(b"turned")).is_some());
    }

    #[tokio::test]
    async fn changes_made_while_the_file_is_written_are_written_after_it() {
        let scratch = Scratch::new();
        let kept = scratch.open(64);
        for n in 0..40u8 {
            assert!(kept.remember(key(&[n]), &format!("/tmp/gradio/{n}/output.psd")));
            if n % 8 == 0 {
                kept.forget(&key(&[n]), &format!("/tmp/gradio/{n}/output.psd"));
            }
            tokio::task::yield_now().await;
        }
        written(&kept).await;
        let again = scratch.open(64);
        assert_eq!(again.state().files.known, kept.state().files.known);
        assert_eq!(again.state().files.len(), 35);
    }

    #[test]
    fn files_go_up_one_at_a_time() {
        let first = uploading().try_lock().expect("free");
        assert!(uploading().try_lock().is_err());
        drop(first);
        assert!(uploading().try_lock().is_ok());
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
