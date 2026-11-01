use std::net::IpAddr;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::net::GameProfile;

#[derive(Debug, Serialize, Deserialize)]
pub struct BannedPlayerEntry {
    pub uuid: Uuid,
    pub name: String,
    #[serde(with = "format::date")]
    pub created: time::OffsetDateTime,
    pub source: String,
    #[serde(with = "format::option_date")]
    pub expires: Option<time::OffsetDateTime>,
    pub reason: String,
}

impl BannedPlayerEntry {
    #[must_use]
    pub fn new(
        profile: &GameProfile,
        source: String,
        expires: Option<time::OffsetDateTime>,
        reason: String,
    ) -> Self {
        Self {
            uuid: profile.id,
            name: profile.name.clone(),
            created: OffsetDateTime::now_utc(),
            source,
            expires,
            reason,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BannedIpEntry {
    pub ip: IpAddr,
    #[serde(with = "format::date")]
    pub created: time::OffsetDateTime,
    pub source: String,
    #[serde(with = "format::option_date")]
    pub expires: Option<time::OffsetDateTime>,
    pub reason: String,
}

impl BannedIpEntry {
    #[must_use]
    pub fn new(
        ip: IpAddr,
        source: String,
        expires: Option<time::OffsetDateTime>,
        reason: String,
    ) -> Self {
        Self {
            ip,
            created: OffsetDateTime::now_utc(),
            source,
            expires,
            reason,
        }
    }
}

mod format {
    use time::OffsetDateTime;
    use time::format_description::FormatItem;
    use time::macros::format_description;

    /// Vanilla's `BanListEntry.DATE_FORMAT`, a `SimpleDateFormat("yyyy-MM-dd HH:mm:ss Z")`.
    const DATE_FORMAT: &[FormatItem<'static>] = format_description!(
        "[year]-[month]-[day] [hour]:[minute]:[second] [offset_hour sign:mandatory][offset_minute]"
    );

    /// Older Pumpkin versions wrote `2026-12-01 07:36:07+00:00`, so those files still have to load.
    const LEGACY_DATE_FORMAT: &[FormatItem<'static>] = format_description!(
        "[year]-[month]-[day] [hour]:[minute]:[second][offset_hour sign:mandatory]:[offset_minute]"
    );

    fn parse_date(date: &str) -> Result<OffsetDateTime, time::error::Parse> {
        OffsetDateTime::parse(date, DATE_FORMAT)
            .or_else(|_| OffsetDateTime::parse(date, LEGACY_DATE_FORMAT))
    }

    pub mod date {
        use serde::{self, Deserialize, Deserializer, Serializer};
        use time::OffsetDateTime;

        use super::{DATE_FORMAT, parse_date};

        pub fn serialize<S: Serializer>(
            date: &OffsetDateTime,
            serializer: S,
        ) -> Result<S::Ok, S::Error> {
            let s = date
                .format(DATE_FORMAT)
                .map_err(serde::ser::Error::custom)?;
            serializer.serialize_str(&s)
        }

        pub fn deserialize<'de, D: Deserializer<'de>>(
            deserializer: D,
        ) -> Result<OffsetDateTime, D::Error> {
            let s = String::deserialize(deserializer)?;
            parse_date(&s).map_err(serde::de::Error::custom)
        }
    }

    pub mod option_date {
        use serde::{self, Deserialize, Deserializer, Serializer};
        use time::OffsetDateTime;

        use super::{DATE_FORMAT, parse_date};

        #[expect(clippy::ref_option)]
        pub fn serialize<S: Serializer>(
            date: &Option<OffsetDateTime>,
            serializer: S,
        ) -> Result<S::Ok, S::Error> {
            if let Some(date) = date {
                let s = date
                    .format(DATE_FORMAT)
                    .map_err(serde::ser::Error::custom)?;
                serializer.serialize_str(&s)
            } else {
                serializer.serialize_str("forever")
            }
        }

        pub fn deserialize<'de, D: Deserializer<'de>>(
            deserializer: D,
        ) -> Result<Option<OffsetDateTime>, D::Error> {
            let s = String::deserialize(deserializer)?;
            if s == "forever" {
                Ok(None)
            } else {
                parse_date(&s).map(Some).map_err(serde::de::Error::custom)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // What vanilla 26.3's `BanListEntry.DATE_FORMAT` writes for 2026-12-01T07:36:07Z.
    const VANILLA_CREATED: &str = "2026-12-01 07:36:07 +0000";
    // What older Pumpkin versions wrote for the same instant.
    const LEGACY_CREATED: &str = "2026-12-01 07:36:07+00:00";
    const CREATED_UNIX_TIMESTAMP: i64 = 1_796_110_567;

    fn banned_player_json(created: &str) -> String {
        format!(
            r#"{{"uuid":"069a79f4-44e9-4726-a5be-fca90e38aaf5","name":"Notch","created":"{created}","source":"Server","expires":"forever","reason":"Banned by an operator."}}"#
        )
    }

    fn serialize_created(created: OffsetDateTime) -> Option<String> {
        let entry = BannedPlayerEntry {
            uuid: Uuid::nil(),
            name: "Notch".to_string(),
            created,
            source: "Server".to_string(),
            expires: None,
            reason: "Banned by an operator.".to_string(),
        };
        serde_json::to_value(&entry)
            .ok()?
            .get("created")?
            .as_str()
            .map(str::to_owned)
    }

    #[test]
    fn reads_vanilla_and_legacy_ban_dates() {
        for created in [VANILLA_CREATED, LEGACY_CREATED] {
            let entry =
                serde_json::from_str::<BannedPlayerEntry>(&banned_player_json(created)).ok();
            assert_eq!(
                entry.map(|entry| entry.created.unix_timestamp()),
                Some(CREATED_UNIX_TIMESTAMP),
                "reading {created}"
            );
        }
    }

    #[test]
    fn writes_vanilla_ban_dates() {
        assert_eq!(
            OffsetDateTime::from_unix_timestamp(CREATED_UNIX_TIMESTAMP)
                .ok()
                .and_then(serialize_created)
                .as_deref(),
            Some(VANILLA_CREATED)
        );
    }
}
