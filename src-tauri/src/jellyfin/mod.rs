//! Jellyfin API client module.
//!
//! This module handles:
//! - Authentication with Jellyfin server
//! - Fetching library metadata (albums, artists, tracks)
//! - Generating streaming and artwork URLs
//! - Secure credential storage via system keyring

mod client;
mod library;
mod models;
mod streaming;

pub use client::{
    authenticate_by_name, clear_credentials, get_or_create_device_id, load_credentials,
    quick_connect_authenticate, quick_connect_enabled, quick_connect_initiate, quick_connect_poll,
    store_auth_method, store_credentials, JellyfinClient, JellyfinCredentials,
};
