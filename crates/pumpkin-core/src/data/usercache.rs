use rustc_hash::FxHashMap;
use std::cmp::Reverse;
use std::{env, fs};

use serde::{Deserialize, Serialize};
use time::format_description::FormatItem;
use time::format_description::well_known::Rfc3339;
use time::macros::format_description;
use time::{Duration, OffsetDateTime};
use tracing::warn;
use uuid::Uuid;

const USER_CACHE_PATH: &str = "usercache.json";
const USER_CACHE_MRU_LIMIT: usize = 1000;

#[derive(Clone, Debug)]
pub struct UserCacheEntry {
    pub uuid: Uuid,
    pub name: String,
    expiration_date: OffsetDateTime,
    last_access: u64,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct UserCacheEntryDisk {
    uuid: Uuid,
    name: String,
    expires_on: String,
}

#[derive(Default)]
pub struct UserCache {
    profiles_by_name: FxHashMap<String, UserCacheEntry>,
    profiles_by_uuid: FxHashMap<Uuid, UserCacheEntry>,
    operation_count: u64,
}

impl UserCache {
    fn path() -> std::path::PathBuf {
        env::current_dir()
            .unwrap_or_else(|_| ".".into())
            .join(super::DATA_FOLDER)
            .join(USER_CACHE_PATH)
    }

    #[must_use]
    pub fn load() -> Self {
        let mut cache = Self::default();
        let mut loaded = Self::load_entries();
        loaded.reverse();
        for entry in loaded {
            cache.safe_add(entry);
        }
        cache
    }

    pub fn save(&self) {
        let path = Self::path();
        if let Some(parent) = path.parent()
            && let Err(error) = fs::create_dir_all(parent)
        {
            warn!("Failed to create user cache directory: {error}");
            return;
        }

        let to_save: Vec<UserCacheEntryDisk> = self
            .top_mru_profiles(USER_CACHE_MRU_LIMIT)
            .into_iter()
            .map(|entry| UserCacheEntryDisk {
                uuid: entry.uuid,
                name: entry.name,
                expires_on: format_cache_date(entry.expiration_date),
            })
            .collect();

        let Ok(content) = serde_json::to_string(&to_save) else {
            return;
        };

        if let Err(error) = fs::write(path, content) {
            warn!("Failed to save user cache: {error}");
        }
    }

    pub fn upsert(&mut self, uuid: Uuid, name: String) {
        self.add_internal(uuid, name);
    }

    pub fn get_by_name(&mut self, name: &str) -> Option<UserCacheEntry> {
        let lowercase_name = name.to_ascii_lowercase();
        let mut profile = self.profiles_by_name.get(&lowercase_name).cloned();
        let mut needs_save = false;

        if let Some(entry) = &profile
            && is_expired(entry.expiration_date)
        {
            self.profiles_by_uuid.remove(&entry.uuid);
            self.profiles_by_name
                .remove(&entry.name.to_ascii_lowercase());
            needs_save = true;
            profile = None;
        }

        if let Some(mut entry) = profile {
            entry.last_access = self.next_operation();
            self.profiles_by_name
                .insert(entry.name.to_ascii_lowercase(), entry.clone());
            self.profiles_by_uuid.insert(entry.uuid, entry.clone());
            return Some(entry);
        }

        if needs_save {
            self.save();
        }

        None
    }

    pub fn get_by_uuid(&mut self, uuid: Uuid) -> Option<UserCacheEntry> {
        let mut entry = self.profiles_by_uuid.get(&uuid).cloned()?;
        entry.last_access = self.next_operation();
        self.profiles_by_name
            .insert(entry.name.to_ascii_lowercase(), entry.clone());
        self.profiles_by_uuid.insert(entry.uuid, entry.clone());
        Some(entry)
    }

    fn add_internal(&mut self, uuid: Uuid, name: String) -> UserCacheEntry {
        let expiration_date = one_month_from_now();
        let entry = UserCacheEntry {
            uuid,
            name,
            expiration_date,
            last_access: 0,
        };

        self.safe_add(entry.clone());
        self.save();
        entry
    }

    fn safe_add(&mut self, mut entry: UserCacheEntry) {
        entry.last_access = self.next_operation();
        self.profiles_by_name
            .insert(entry.name.to_ascii_lowercase(), entry.clone());
        self.profiles_by_uuid.insert(entry.uuid, entry);
    }

    #[allow(clippy::missing_const_for_fn)]
    fn next_operation(&mut self) -> u64 {
        self.operation_count += 1;
        self.operation_count
    }

    fn top_mru_profiles(&self, limit: usize) -> Vec<UserCacheEntry> {
        let mut entries: Vec<UserCacheEntry> = self.profiles_by_uuid.values().cloned().collect();
        entries.sort_by_key(|entry| Reverse(entry.last_access));
        entries.truncate(limit);
        entries
    }

    fn load_entries() -> Vec<UserCacheEntry> {
        let path = Self::path();
        let Ok(raw) = fs::read_to_string(path) else {
            return Vec::new();
        };

        let Ok(json) = serde_json::from_str::<serde_json::Value>(&raw) else {
            return Vec::new();
        };

        let Some(array) = json.as_array() else {
            return Vec::new();
        };

        let mut entries = Vec::new();
        for element in array {
            let Some(object) = element.as_object() else {
                continue;
            };

            let Some(name) = object.get("name").and_then(serde_json::Value::as_str) else {
                continue;
            };
            let Some(uuid_raw) = object.get("uuid").and_then(serde_json::Value::as_str) else {
                continue;
            };
            let Some(expires_on) = object.get("expiresOn").and_then(serde_json::Value::as_str)
            else {
                continue;
            };

            let Ok(uuid) = Uuid::parse_str(uuid_raw) else {
                continue;
            };

            let Some(expiration_date) = parse_cache_date(expires_on) else {
                continue;
            };

            entries.push(UserCacheEntry {
                uuid,
                name: name.to_string(),
                expiration_date,
                last_access: 0,
            });
        }

        entries
    }
}

/// Vanilla writes and reads `expiresOn` with `CachedUserNameToIdResolver.createDateFormat()`.
const USER_CACHE_DATE_FORMAT: &[FormatItem<'static>] = format_description!(
    "[year]-[month]-[day] [hour]:[minute]:[second] [offset_hour sign:mandatory][offset_minute]"
);

/// Older Pumpkin versions wrote RFC 3339 dates, so those files still have to load.
fn parse_cache_date(date: &str) -> Option<OffsetDateTime> {
    OffsetDateTime::parse(date, &Rfc3339)
        .or_else(|_| OffsetDateTime::parse(date, USER_CACHE_DATE_FORMAT))
        .ok()
}

fn format_cache_date(date: OffsetDateTime) -> String {
    date.format(USER_CACHE_DATE_FORMAT).unwrap_or_default()
}

fn is_expired(expiration_date: OffsetDateTime) -> bool {
    OffsetDateTime::now_utc() >= expiration_date
}

fn one_month_from_now() -> OffsetDateTime {
    OffsetDateTime::now_utc() + Duration::days(30)
}

#[cfg(test)]
mod tests {
    use super::*;

    // What vanilla 26.3 writes into usercache.json for the instant 2026-12-01T07:36:07Z.
    const VANILLA_EXPIRES_ON: &str = "2026-12-01 07:36:07 +0000";
    const VANILLA_EXPIRES_ON_UNIX: i64 = 1_796_110_567;

    #[test]
    fn parses_vanilla_and_rfc3339_expiration_dates() {
        for expires_on in [VANILLA_EXPIRES_ON, "2026-12-01T07:36:07Z"] {
            assert_eq!(
                parse_cache_date(expires_on).map(OffsetDateTime::unix_timestamp),
                Some(VANILLA_EXPIRES_ON_UNIX)
            );
        }
    }

    #[test]
    fn writes_vanilla_expiration_date() {
        assert_eq!(
            OffsetDateTime::from_unix_timestamp(VANILLA_EXPIRES_ON_UNIX)
                .ok()
                .map(format_cache_date)
                .as_deref(),
            Some(VANILLA_EXPIRES_ON)
        );
    }
}
