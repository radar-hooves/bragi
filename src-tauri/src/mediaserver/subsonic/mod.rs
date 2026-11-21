//! OpenSubsonic backend.
//!
//! An API client for OpenSubsonic-compatible servers (Navidrome, Gonic,
//! Airsonic, …). Query-param token auth (`auth`), `subsonic-response`-wrapped
//! JSON, and wire→domain mappers (`models`) producing the same domain models
//! as the Jellyfin backend.

pub mod auth;
pub mod client;
pub mod models;
pub mod streaming;

pub use client::{
    clear_credentials, load_credentials, store_credentials, SubsonicClient, SubsonicCredentials,
};
