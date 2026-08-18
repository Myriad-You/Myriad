// SeaORM entities will be generated here
// Run: sea-orm-cli generate entity -o src/models/entities

#![allow(clippy::empty_docs)]

pub mod activity_events;
pub mod metadata_history;
pub mod platform_metadata;
pub mod platform_reports;
pub mod platforms;

// Tapp 系统实体
pub mod tapp_storage;
pub mod tapp_store_sources;
pub mod tapp_user_activities;
pub mod tapp_widgets;
pub mod tapps;

// Tapp 定时任务系统
pub mod tapp_scheduled_tasks;
pub mod tapp_task_executions;

// Brew 阅读系统实体
pub mod brew_annotations;
pub mod brew_categories;
pub mod brew_comments;
pub mod brew_items;
pub mod brew_podcasts;
pub mod brew_sources;
pub mod brew_user_states;
pub mod rsshub_instances;

// Agent 任务系统实体
pub mod agent_addressee_state;
pub mod agent_diary;
pub mod agent_messages;
pub mod agent_notifications;
pub mod agent_persona;
pub mod agent_proactive_messages;
pub mod agent_sessions;
pub mod agent_task_presets;
pub mod agent_tasks;

// Federation (MFP) 联邦协议实体
pub mod federation_activities;
pub mod federation_channel_messages;
pub mod federation_channels;
pub mod federation_delivery_queue;
pub mod federation_file_transfers;
pub mod federation_follows;
pub mod federation_instances;
pub mod federation_keys;
pub mod federation_published_content;
pub mod federation_remote_actors;
pub mod federation_ring_memberships;
pub mod federation_room_members;
pub mod federation_room_messages;
pub mod federation_rooms;
pub mod federation_timeline;
