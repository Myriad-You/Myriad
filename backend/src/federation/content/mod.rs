//! Federation content publish, media upload, and author timeline helpers.
//!
//! Real submodules (not `include!`) so each file owns its imports and visibility.

mod ap_object;
mod media;
mod publish;
mod timeline;
mod types;

pub use media::{federation_media_root, store_federation_media};
pub use publish::{create_note, list_published, publish_content, unpublish_content};
pub use types::{
    CreateNoteRequest, MediaUploadResponse, NoteAttachmentInput, PublishRequest, PublishResponse,
    PublishedAttachment, PublishedItem,
};

pub(crate) use ap_object::fan_out_to_followers;
