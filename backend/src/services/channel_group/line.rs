//! The group line every platform parses into.

use super::*;

/// One human line in a group, whatever the platform. Ids are the platform's,
/// as text; the name and text are attacker-controlled and bounded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupLine {
    pub platform: ChannelPlatform,
    pub chat: String,
    pub message_id: String,
    /// A Telegram forum topic, answered in the same topic.
    pub thread: Option<i64>,
    pub from: String,
    pub display_name: String,
    pub text: String,
    /// Whether it speaks to her.
    pub addressed: bool,
    /// The line it replies to, if any.
    pub reply_to: Option<QuotedLine>,
    /// Pictures in it.
    pub images: Vec<GroupImage>,
    /// How she shows in the group, when the platform tells (her group card
    /// on QQ).
    pub shown_as: Option<String>,
}

impl GroupLine {
    /// The group, as sessions and memory know it: `<platform>:<chat id>`.
    pub fn venue(&self) -> String {
        format!("{}:{}", self.platform.slug(), self.chat)
    }

    /// The line as her group mind takes it (which reads it, the line it
    /// replies to in front: a reply makes sense only with what it answers).
    pub(super) fn to_message(&self) -> myriad_merope::group::Message {
        myriad_merope::group::Message {
            platform: self.platform.slug().to_string(),
            chat: self.chat.clone(),
            message_id: self.message_id.clone(),
            thread: self.thread,
            from: self.from.clone(),
            display_name: self.display_name.clone(),
            text: self.text.clone(),
            addressed: self.addressed,
            reply_to: self.reply_to.clone(),
            images: self.images.clone(),
            shown_as: self.shown_as.clone(),
        }
    }
}

impl From<TelegramGroupMessage> for GroupLine {
    fn from(message: TelegramGroupMessage) -> Self {
        Self {
            platform: ChannelPlatform::Telegram,
            chat: message.chat_id.to_string(),
            message_id: message.message_id.to_string(),
            thread: message.message_thread_id,
            from: message.from_id.to_string(),
            display_name: message.display_name,
            text: message.text,
            addressed: message.addressed,
            reply_to: message.reply_to,
            images: message.images,
            shown_as: None,
        }
    }
}

impl From<myriad_agent_rules::onebot::decode::OneBotGroupLine> for GroupLine {
    fn from(message: myriad_agent_rules::onebot::decode::OneBotGroupLine) -> Self {
        Self {
            platform: ChannelPlatform::OneBot,
            chat: message.group_id,
            message_id: message.message_id,
            thread: None,
            from: message.user_id,
            display_name: message.display_name,
            text: message.text,
            addressed: message.addressed,
            reply_to: message.reply_to,
            images: message.images,
            shown_as: None,
        }
    }
}

impl From<DiscordGroupMessage> for GroupLine {
    fn from(message: DiscordGroupMessage) -> Self {
        Self {
            platform: ChannelPlatform::Discord,
            chat: message.channel_id,
            message_id: message.message_id,
            thread: None,
            from: message.author_id,
            display_name: message.display_name,
            text: message.text,
            addressed: message.addressed,
            reply_to: message.reply_to,
            images: message.images,
            shown_as: None,
        }
    }
}
