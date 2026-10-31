use std::{borrow::Cow, fmt, vec};

use serde::de::{MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Serialize};

use super::{TextComponent, TextComponentBase};

/// An item's data component patch in a `show_item` hover event, kept as the
/// JSON it was given (`{"minecraft:enchantments":{"minecraft:sharpness":5}}`).
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct ItemComponents(pub serde_json::Value);

impl<'de> Deserialize<'de> for ItemComponents {
    /// Only a JSON object is a component patch.
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        if value.is_object() {
            Ok(Self(value))
        } else {
            Err(serde::de::Error::custom(
                "show_item components must be an object",
            ))
        }
    }
}

impl std::hash::Hash for ItemComponents {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.to_string().hash(state);
    }
}

impl ItemComponents {
    /// The components as an NBT compound, as the network writes them.
    #[must_use]
    pub fn to_nbt(&self) -> pumpkin_nbt::tag::NbtTag {
        json_to_nbt(&self.0)
    }
}

/// JSON as NBT: whole numbers as the smallest integer tag that holds them.
fn json_to_nbt(value: &serde_json::Value) -> pumpkin_nbt::tag::NbtTag {
    use pumpkin_nbt::tag::NbtTag;
    match value {
        serde_json::Value::Null => NbtTag::Compound(pumpkin_nbt::compound::NbtCompound::new()),
        serde_json::Value::Bool(b) => NbtTag::Byte(i8::from(*b)),
        serde_json::Value::Number(n) => n.as_i64().map_or_else(
            || NbtTag::Double(n.as_f64().unwrap_or(0.0)),
            |i| {
                i8::try_from(i).map_or_else(
                    |_| {
                        i16::try_from(i).map_or_else(
                            |_| i32::try_from(i).map_or(NbtTag::Long(i), NbtTag::Int),
                            NbtTag::Short,
                        )
                    },
                    NbtTag::Byte,
                )
            },
        ),
        serde_json::Value::String(s) => NbtTag::String(s.clone().into_boxed_str()),
        serde_json::Value::Array(list) => NbtTag::List(list.iter().map(json_to_nbt).collect()),
        serde_json::Value::Object(map) => {
            let mut compound = pumpkin_nbt::compound::NbtCompound::new();
            for (key, value) in map {
                compound.put(key, json_to_nbt(value));
            }
            NbtTag::Compound(compound)
        }
    }
}

/// Vanilla reads a tooltip as one component (`HoverEvent.java:138`), and a
/// component is a string, a non-empty list of components or a component object
/// (`ComponentSerialization.java:105`). The model keeps a list of components.
fn deserialize_components<'de, D>(deserializer: D) -> Result<Vec<TextComponentBase>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct ComponentsVisitor;

    impl<'de> Visitor<'de> for ComponentsVisitor {
        type Value = Vec<TextComponentBase>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a text component or a list of text components")
        }

        fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Self::Value, E> {
            Ok(vec![TextComponent::text(value.to_string()).0])
        }

        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
            let mut components = Vec::new();
            while let Some(component) = seq.next_element::<TextComponent>()? {
                components.push(component.0);
            }
            if components.is_empty() {
                return Err(serde::de::Error::custom("List must have contents"));
            }
            Ok(components)
        }

        fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Self::Value, A::Error> {
            TextComponent::deserialize(serde::de::value::MapAccessDeserializer::new(map))
                .map(|component| vec![component.0])
        }
    }

    deserializer.deserialize_any(ComponentsVisitor)
}

/// The optional form of [`deserialize_components`], for `show_entity`'s `name`.
fn deserialize_optional_components<'de, D>(
    deserializer: D,
) -> Result<Option<Vec<TextComponentBase>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct OptionalComponentsVisitor;

    impl<'de> Visitor<'de> for OptionalComponentsVisitor {
        type Value = Option<Vec<TextComponentBase>>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a text component, a list of text components or nothing")
        }

        fn visit_none<E: serde::de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_some<D: serde::Deserializer<'de>>(
            self,
            deserializer: D,
        ) -> Result<Self::Value, D::Error> {
            deserialize_components(deserializer).map(Some)
        }
    }

    deserializer.deserialize_option(OptionalComponentsVisitor)
}

/// Represents the hover event action in a chat component.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum HoverEvent {
    /// Displays a tooltip with the given text.
    ShowText {
        #[serde(deserialize_with = "deserialize_components")]
        value: Vec<TextComponentBase>,
    },
    /// Shows an item.
    ShowItem {
        /// Resource identifier of the item.
        id: Cow<'static, str>,
        /// Number of the items in the stack.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        count: Option<i32>,
        /// The stack's data components (`minecraft:enchantments`, ...), as
        /// given.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        components: Option<ItemComponents>,
    },
    /// Shows an entity.
    ShowEntity {
        /// The entity's ID Entity Type.
        id: Cow<'static, str>,
        /// The entity's UUID
        /// The UUID cannot use `uuid::Uuid` because its serialization parses it into bytes, so its double bytes serialized.
        uuid: Cow<'static, str>,
        /// Optional custom name for the entity.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_optional_components"
        )]
        name: Option<Vec<TextComponentBase>>,
    },
}

impl HoverEvent {
    /// Creates a new hover event that displays text.
    ///
    /// # Arguments
    /// - `text` – The text component to display in the tooltip.
    ///
    /// # Returns
    /// A `HoverEvent::ShowText` variant containing the provided text.
    #[must_use]
    pub fn show_text(text: TextComponent) -> Self {
        Self::ShowText {
            value: vec![text.0],
        }
    }

    /// Creates a new hover event that displays entity information.
    ///
    /// # Arguments
    /// - `uuid` – The entity's UUID as a string.
    /// - `kind` – The entity type identifier (e.g., "minecraft:pig").
    /// - `name` – Optional custom name for the entity.
    ///
    /// # Returns
    /// A `HoverEvent::ShowEntity` variant containing the entity information.
    pub fn show_entity<P: Into<Cow<'static, str>>>(
        uuid: P,
        kind: P,
        name: Option<TextComponent>,
    ) -> Self {
        Self::ShowEntity {
            id: kind.into(),
            uuid: uuid.into(),
            name: match name {
                Some(name) => Some(vec![name.0]),
                None => None,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::translation::Locale;

    #[test]
    fn item_components_must_be_an_object() {
        assert!(serde_json::from_str::<ItemComponents>(r#"{"minecraft:damage":3}"#).is_ok());
        for bad in ["[]", r#""x""#, "3", "null", "true"] {
            assert!(
                serde_json::from_str::<ItemComponents>(bad).is_err(),
                "{bad}"
            );
        }
    }

    /// Vanilla writes a tooltip as one component and `shadow_color` as an int,
    /// so both have to come back in.
    #[test]
    fn vanilla_component_json_parses() {
        let show_text: TextComponent = serde_json::from_str(
            r#"{"text":"x","hover_event":{"action":"show_text","value":{"text":"h"}}}"#,
        )
        .unwrap();
        assert!(matches!(
            show_text.0.style.hover_event,
            Some(HoverEvent::ShowText { value })
                if value.len() == 1 && value[0].clone().get_text(Locale::EnUs) == "h"
        ));

        let literal: TextComponent = serde_json::from_str(
            r#"{"text":"x","hover_event":{"action":"show_text","value":"h"}}"#,
        )
        .unwrap();
        assert!(matches!(
            literal.0.style.hover_event,
            Some(HoverEvent::ShowText { value })
                if value.len() == 1 && value[0].clone().get_text(Locale::EnUs) == "h"
        ));

        let shadow: TextComponent =
            serde_json::from_str(r#"{"text":"x","shadow_color":-16777216}"#).unwrap();
        assert_eq!(
            shadow.0.style.shadow_color.map(|color| color.to_argb_int()),
            Some(-16777216)
        );
        assert_eq!(
            serde_json::to_string(&shadow).unwrap(),
            r#"{"text":"x","shadow_color":-16777216}"#
        );

        let channels: TextComponent =
            serde_json::from_str(r#"{"text":"x","shadow_color":[1.0,0.0,0.0,1.0]}"#).unwrap();
        assert_eq!(
            channels
                .0
                .style
                .shadow_color
                .map(|color| color.to_argb_int()),
            Some(-65536)
        );
    }
}
